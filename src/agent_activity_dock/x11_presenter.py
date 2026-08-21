"""Small Xlib/cairo-free X11 presenter for the collapsed ball.

This module intentionally depends only on libX11, which is already present in
the WSL2 test environment.  It creates a fixed 44x44 override-redirect window,
draws a colored frame and count text with core X fonts, and opens a compact
task list on click.  No toolkit-private object leaks into presenter tests.
"""
from __future__ import annotations

import ctypes
import ctypes.util
import os
import re
import subprocess
from typing import Callable, Optional

from .core import Snapshot
from .presenter import (
    BALL_BACKGROUND_COLOR,
    BALL_SIZE,
    FLASH_BORDER_COLOR,
    FRAME_WIDTH,
    IDLE_BORDER_COLOR,
    TEXT_COLOR,
    BallView,
)

# ---------------------------------------------------------------------------
# libX11 bindings

_XLIB_NAME = ctypes.util.find_library("X11")
if not _XLIB_NAME:
    raise RuntimeError("libX11 is required for the floating-ball presenter")
_x11 = ctypes.CDLL(_XLIB_NAME)

Display = ctypes.c_void_p
Window = ctypes.c_ulong
XID = ctypes.c_ulong
Atom = ctypes.c_ulong
Bool = ctypes.c_int
Status = ctypes.c_int


class XColor(ctypes.Structure):
    _fields_ = [
        ("pixel", ctypes.c_ulong),
        ("red", ctypes.c_ushort),
        ("green", ctypes.c_ushort),
        ("blue", ctypes.c_ushort),
        ("flags", ctypes.c_char),
        ("pad", ctypes.c_char),
    ]


class XSetWindowAttributes(ctypes.Structure):
    _fields_ = [
        ("background_pixmap", XID),
        ("background_pixel", ctypes.c_ulong),
        ("border_pixmap", XID),
        ("border_pixel", ctypes.c_ulong),
        ("bit_gravity", ctypes.c_int),
        ("win_gravity", ctypes.c_int),
        ("backing_store", ctypes.c_int),
        ("backing_planes", ctypes.c_ulong),
        ("backing_pixel", ctypes.c_ulong),
        ("save_under", Bool),
        ("event_mask", ctypes.c_long),
        ("do_not_propagate_mask", ctypes.c_long),
        ("override_redirect", Bool),
        ("colormap", XID),
        ("cursor", XID),
    ]


class XCharStruct(ctypes.Structure):
    _fields_ = [
        ("lbearing", ctypes.c_short),
        ("rbearing", ctypes.c_short),
        ("width", ctypes.c_short),
        ("ascent", ctypes.c_short),
        ("descent", ctypes.c_short),
        ("attributes", ctypes.c_ushort),
    ]


class XFontStruct(ctypes.Structure):
    _fields_ = [
        ("ext_data", ctypes.c_void_p),
        ("fid", XID),
        ("direction", ctypes.c_uint),
        ("min_char_or_byte2", ctypes.c_uint),
        ("max_char_or_byte2", ctypes.c_uint),
        ("min_byte1", ctypes.c_uint),
        ("max_byte1", ctypes.c_uint),
        ("all_chars_exist", Bool),
        ("default_char", ctypes.c_uint),
        ("n_properties", ctypes.c_int),
        ("properties", ctypes.c_void_p),
        ("min_bounds", XCharStruct),
        ("max_bounds", XCharStruct),
        ("per_char", XCharStruct),
        ("ascent", ctypes.c_int),
        ("descent", ctypes.c_int),
    ]


class XAnyEvent(ctypes.Structure):
    _fields_ = [
        ("type", ctypes.c_int),
        ("serial", ctypes.c_ulong),
        ("send_event", Bool),
        ("display", Display),
        ("window", Window),
    ]


class XExposeEvent(ctypes.Structure):
    _fields_ = [
        ("type", ctypes.c_int),
        ("serial", ctypes.c_ulong),
        ("send_event", Bool),
        ("display", Display),
        ("window", Window),
        ("x", ctypes.c_int),
        ("y", ctypes.c_int),
        ("width", ctypes.c_int),
        ("height", ctypes.c_int),
        ("count", ctypes.c_int),
    ]


class XButtonEvent(ctypes.Structure):
    _fields_ = [
        ("type", ctypes.c_int),
        ("serial", ctypes.c_ulong),
        ("send_event", Bool),
        ("display", Display),
        ("window", Window),
        ("root", Window),
        ("subwindow", Window),
        ("time", ctypes.c_ulong),
        ("x", ctypes.c_int),
        ("y", ctypes.c_int),
        ("x_root", ctypes.c_int),
        ("y_root", ctypes.c_int),
        ("state", ctypes.c_uint),
        ("button", ctypes.c_uint),
        ("same_screen", Bool),
    ]


class XKeyEvent(ctypes.Structure):
    _fields_ = [
        ("type", ctypes.c_int),
        ("serial", ctypes.c_ulong),
        ("send_event", Bool),
        ("display", Display),
        ("window", Window),
        ("root", Window),
        ("subwindow", Window),
        ("time", ctypes.c_ulong),
        ("x", ctypes.c_int),
        ("y", ctypes.c_int),
        ("x_root", ctypes.c_int),
        ("y_root", ctypes.c_int),
        ("state", ctypes.c_uint),
        ("keycode", ctypes.c_uint),
        ("same_screen", Bool),
    ]


class XClientMessageData(ctypes.Union):
    _fields_ = [
        ("b", ctypes.c_char * 20),
        ("s", ctypes.c_short * 10),
        ("l", ctypes.c_long * 5),
    ]


class XClientMessageEvent(ctypes.Structure):
    _fields_ = [
        ("type", ctypes.c_int),
        ("serial", ctypes.c_ulong),
        ("send_event", Bool),
        ("display", Display),
        ("window", Window),
        ("message_type", Atom),
        ("format", ctypes.c_int),
        ("data", XClientMessageData),
    ]


class XEvent(ctypes.Union):
    # Xlib guarantees 192 bytes even though the MVP only reads the first few
    # event types.  Without this padding XNextEvent writes past the Python
    # allocation and corrupts the process.
    _fields_ = [
        ("type", ctypes.c_int),
        ("xany", XAnyEvent),
        ("xexpose", XExposeEvent),
        ("xbutton", XButtonEvent),
        ("xkey", XKeyEvent),
        ("xclient", XClientMessageEvent),
        ("_pad", ctypes.c_byte * 192),
    ]


XEventPointer = ctypes.POINTER(XEvent)

_x11.XOpenDisplay.argtypes = [ctypes.c_char_p]
_x11.XOpenDisplay.restype = Display
_x11.XCloseDisplay.argtypes = [Display]
_x11.XCloseDisplay.restype = ctypes.c_int
_x11.XDefaultScreen.argtypes = [Display]
_x11.XDefaultScreen.restype = ctypes.c_int
_x11.XDefaultRootWindow.argtypes = [Display]
_x11.XDefaultRootWindow.restype = Window
_x11.XDefaultColormap.argtypes = [Display, ctypes.c_int]
_x11.XDefaultColormap.restype = XID
_x11.XDefaultDepth.argtypes = [Display, ctypes.c_int]
_x11.XDefaultDepth.restype = ctypes.c_int
_x11.XDefaultVisual.argtypes = [Display, ctypes.c_int]
_x11.XDefaultVisual.restype = ctypes.c_void_p
_x11.XDisplayWidth.argtypes = [Display, ctypes.c_int]
_x11.XDisplayWidth.restype = ctypes.c_int
_x11.XDisplayHeight.argtypes = [Display, ctypes.c_int]
_x11.XDisplayHeight.restype = ctypes.c_int
_x11.XAllocNamedColor.argtypes = [
    Display, XID, ctypes.c_char_p,
    ctypes.POINTER(XColor), ctypes.POINTER(XColor),
]
_x11.XAllocNamedColor.restype = Status
_x11.XCreateWindow.argtypes = [
    Display, Window, ctypes.c_int, ctypes.c_int,
    ctypes.c_uint, ctypes.c_uint, ctypes.c_uint,
    ctypes.c_int, ctypes.c_uint, ctypes.c_void_p,
    ctypes.c_ulong, ctypes.POINTER(XSetWindowAttributes),
]
_x11.XCreateWindow.restype = Window
_x11.XDestroyWindow.argtypes = [Display, Window]
_x11.XDestroyWindow.restype = ctypes.c_int
_x11.XMapWindow.argtypes = [Display, Window]
_x11.XMapWindow.restype = ctypes.c_int
_x11.XMapRaised.argtypes = [Display, Window]
_x11.XMapRaised.restype = ctypes.c_int
_x11.XUnmapWindow.argtypes = [Display, Window]
_x11.XUnmapWindow.restype = ctypes.c_int
_x11.XStoreName.argtypes = [Display, Window, ctypes.c_char_p]
_x11.XStoreName.restype = ctypes.c_int
_x11.XFlush.argtypes = [Display]
_x11.XFlush.restype = ctypes.c_int
_x11.XSync.argtypes = [Display, Bool]
_x11.XSync.restype = ctypes.c_int
_x11.XPending.argtypes = [Display]
_x11.XPending.restype = ctypes.c_int
_x11.XNextEvent.argtypes = [Display, XEventPointer]
_x11.XNextEvent.restype = ctypes.c_int
_x11.XConnectionNumber.argtypes = [Display]
_x11.XConnectionNumber.restype = ctypes.c_int
_x11.XInternAtom.argtypes = [Display, ctypes.c_char_p, Bool]
_x11.XInternAtom.restype = Atom
_x11.XChangeProperty.argtypes = [
    Display, Window, Atom, Atom, ctypes.c_int, ctypes.c_int,
    ctypes.c_void_p, ctypes.c_int,
]
_x11.XChangeProperty.restype = ctypes.c_int
_x11.XSendEvent.argtypes = [
    Display, Window, Bool, ctypes.c_long, XEventPointer,
]
_x11.XSendEvent.restype = Status
_x11.XCreateGC.argtypes = [Display, Window, ctypes.c_ulong, ctypes.c_void_p]
_x11.XCreateGC.restype = ctypes.c_void_p
_x11.XFreeGC.argtypes = [Display, ctypes.c_void_p]
_x11.XFreeGC.restype = ctypes.c_int
_x11.XSetForeground.argtypes = [Display, ctypes.c_void_p, ctypes.c_ulong]
_x11.XSetForeground.restype = ctypes.c_int
_x11.XFillRectangle.argtypes = [
    Display, Window, ctypes.c_void_p,
    ctypes.c_int, ctypes.c_int, ctypes.c_uint, ctypes.c_uint,
]
_x11.XFillRectangle.restype = ctypes.c_int
_x11.XDrawString.argtypes = [
    Display, Window, ctypes.c_void_p,
    ctypes.c_int, ctypes.c_int, ctypes.c_char_p, ctypes.c_int,
]
_x11.XDrawString.restype = ctypes.c_int
_x11.XLoadQueryFont.argtypes = [Display, ctypes.c_char_p]
_x11.XLoadQueryFont.restype = ctypes.POINTER(XFontStruct)
_x11.XFreeFont.argtypes = [Display, ctypes.POINTER(XFontStruct)]
_x11.XFreeFont.restype = ctypes.c_int
_x11.XSetFont.argtypes = [Display, ctypes.c_void_p, XID]
_x11.XSetFont.restype = ctypes.c_int
_x11.XTextWidth.argtypes = [
    ctypes.POINTER(XFontStruct), ctypes.c_char_p, ctypes.c_int,
]
_x11.XTextWidth.restype = ctypes.c_int
_x11.XClearWindow.argtypes = [Display, Window]
_x11.XClearWindow.restype = ctypes.c_int
_x11.XKeysymToKeycode.argtypes = [Display, ctypes.c_ulong]
_x11.XKeysymToKeycode.restype = ctypes.c_uint

CWBackPixel = 1 << 1
CWBorderPixel = 1 << 3
CWEventMask = 1 << 11
CWOverrideRedirect = 1 << 9
CWColormap = 1 << 13

Expose = 12
ButtonPress = 4
ButtonRelease = 5
KeyPress = 2
ClientMessage = 33
StructureNotifyMask = 1 << 17
SubstructureRedirectMask = 1 << 20
SubstructureNotifyMask = 1 << 19
ExposureMask = 1 << 15
ButtonPressMask = 1 << 2
ButtonReleaseMask = 1 << 3
KeyPressMask = 1 << 0

XK_Escape = 0xFF1B
XA_ATOM = 4
PropModeReplace = 0

_NET_WM_STATE_REMOVE = 0
_NET_WM_STATE_ADD = 1
_NET_WM_STATE_TOGGLE = 2


def _parse_color(display: Display, colormap: XID, name: str) -> int:
    color = XColor()
    exact = XColor()
    if not _x11.XAllocNamedColor(
        display, colormap, name.encode("ascii"),
        ctypes.byref(color), ctypes.byref(exact),
    ):
        # Every palette color above is standard; fall back to monochrome
        # rather than crashing the presenter.
        return int(_x11.XBlackPixel(display, _x11.XDefaultScreen(display))) \
            if False else _white_pixel(display)
    return int(color.pixel)


def _white_pixel(display: Display) -> int:
    colormap = _x11.XDefaultColormap(display, _x11.XDefaultScreen(display))
    color = XColor()
    exact = XColor()
    _x11.XAllocNamedColor(
        display, colormap, b"#ffffff", ctypes.byref(color), ctypes.byref(exact),
    )
    return int(color.pixel)


_x11.XBlackPixel.argtypes = [Display, ctypes.c_int]
_x11.XBlackPixel.restype = ctypes.c_ulong
_x11.XWhitePixel.argtypes = [Display, ctypes.c_int]
_x11.XWhitePixel.restype = ctypes.c_ulong

TASK_LIST_WIDTH = 420
TASK_LIST_ROW_HEIGHT = 18
TASK_LIST_HEADER_HEIGHT = 28
TASK_LIST_PADDING = 8

_XRANDR_MONITOR_RE = re.compile(
    r"^\s*\d+:\s*\+?\*?\S+\s+"
    r"(?P<w>\d+)/(?P<wmm>\d+)x(?P<h>\d+)/(?P<hmm>\d+)"
    r"\+(?P<x>\d+)\+(?P<y>\d+)"
)


def parse_monitor_rect_from_xrandr_line(line: str):
    match = _XRANDR_MONITOR_RE.search(line)
    if not match:
        return None
    return (
        int(match.group("x")),
        int(match.group("y")),
        int(match.group("w")),
        int(match.group("h")),
    )


class X11BallPresenter:
    """Renders :class:`BallView` in a tiny always-on-top X11 window."""

    def __init__(
        self,
        initial_view: BallView,
        on_ball_click: Optional[Callable[[], None]] = None,
        on_list_close: Optional[Callable[[], None]] = None,
        *,
        override_redirect: bool = True,
    ) -> None:
        self.view = initial_view
        self.on_ball_click = on_ball_click
        self.on_list_close = on_list_close
        self.override_redirect = override_redirect
        self._flash_border: Optional[str] = None
        self._list_mapped = False

        self.display = _x11.XOpenDisplay(None)
        if not self.display:
            raise RuntimeError(
                "cannot open X display; run in a desktop session or WSLg"
            )
        self.screen = _x11.XDefaultScreen(self.display)
        self.root = _x11.XDefaultRootWindow(self.display)
        self._monitor_rect = self._detect_visible_monitor()
        self.colormap = _x11.XDefaultColormap(self.display, self.screen)
        self.gc = _x11.XCreateGC(self.display, self.root, 0, None)
        if not self.gc:
            self._x11.XCloseDisplay(self.display)
            raise RuntimeError("cannot allocate X graphics context")

        self.colors = {
            name: _parse_color(self.display, self.colormap, name)
            for name in (
                BALL_BACKGROUND_COLOR,
                TEXT_COLOR,
                "#22c55e",
                "#64748b",
                FLASH_BORDER_COLOR,
            )
        }
        self.font = _x11.XLoadQueryFont(self.display, b"fixed")
        if not self.font:
            _x11.XCloseDisplay(self.display)
            raise RuntimeError("X core font 'fixed' is unavailable")

        self._set_up_atoms()
        self.ball_window = self._create_ball_window()
        self.list_window = self._create_list_window()
        self._set_window_type_dock(self.ball_window)
        self._set_motif_no_decorations(self.ball_window)
        self._set_always_on_top_property(self.ball_window)
        self._set_always_on_top_property(self.list_window)
        _x11.XMapRaised(self.display, self.ball_window)
        _x11.XFlush(self.display)
        self._request_always_on_top(self.ball_window)
        self._request_always_on_top(self.list_window)
        self.redraw_ball()

    # -- public API -----------------------------------------------------

    def fileno(self) -> int:
        return _x11.XConnectionNumber(self.display)

    def update(self, view: BallView) -> None:
        self.view = view
        self.redraw_ball()
        if self._list_mapped:
            self.redraw_list()

    def flash(self, border_color: str = FLASH_BORDER_COLOR) -> None:
        """Set one flash color; call update() to return to normal border."""
        self._flash_border = border_color
        self.redraw_ball()

    def clear_flash(self) -> None:
        """Revert a completed one-shot flash to the normal snapshot border."""
        if self._flash_border is not None:
            self._flash_border = None
            self.redraw_ball()

    def process_pending_events(self) -> None:
        while _x11.XPending(self.display):
            event = XEvent()
            _x11.XNextEvent(self.display, ctypes.byref(event))
            self._dispatch_event(event)
        _x11.XFlush(self.display)

    def close(self) -> None:
        try:
            if getattr(self, "display", None):
                _x11.XDestroyWindow(self.display, self.ball_window)
                _x11.XDestroyWindow(self.display, self.list_window)
                _x11.XFreeGC(self.display, self.gc)
                _x11.XFreeFont(self.display, self.font)
                _x11.XCloseDisplay(self.display)
        except Exception:
            pass

    # -- window creation ------------------------------------------------

    def _set_up_atoms(self) -> None:
        self.atom_net_wm_state = _x11.XInternAtom(
            self.display, b"_NET_WM_STATE", 0
        )
        self.atom_net_wm_state_above = _x11.XInternAtom(
            self.display, b"_NET_WM_STATE_ABOVE", 0
        )
        self.atom_net_wm_state_skip_taskbar = _x11.XInternAtom(
            self.display, b"_NET_WM_STATE_SKIP_TASKBAR", 0
        )
        self.atom_net_wm_window_type = _x11.XInternAtom(
            self.display, b"_NET_WM_WINDOW_TYPE", 0
        )
        self.atom_net_wm_window_type_dock = _x11.XInternAtom(
            self.display, b"_NET_WM_WINDOW_TYPE_DOCK", 0
        )
        self.atom_motif_wm_hints = _x11.XInternAtom(
            self.display, b"_MOTIF_WM_HINTS", 0
        )

    def _create_window(
        self,
        title: str,
        width: int,
        height: int,
        x: int,
        y: int,
        override_redirect: bool,
    ) -> Window:
        attrs = XSetWindowAttributes()
        attrs.background_pixel = self.colors[BALL_BACKGROUND_COLOR]
        attrs.border_pixel = self.colors.get(IDLE_BORDER_COLOR, 0)
        attrs.event_mask = (
            ExposureMask | ButtonPressMask | ButtonReleaseMask | KeyPressMask
        )
        attrs.override_redirect = 1 if override_redirect else 0
        attrs.colormap = self.colormap
        mask = CWBackPixel | CWBorderPixel | CWEventMask | CWColormap
        if override_redirect:
            mask |= CWOverrideRedirect
        window = _x11.XCreateWindow(
            self.display,
            self.root,
            x,
            y,
            width,
            height,
            0,
            _x11.XDefaultDepth(self.display, self.screen),
            1,  # InputOutput
            _x11.XDefaultVisual(self.display, self.screen),
            mask,
            ctypes.byref(attrs),
        )
        _x11.XStoreName(self.display, window, title.encode("utf-8"))
        return window

    def _create_ball_window(self) -> Window:
        monitor = self._monitor_rect
        x = max(0, monitor[0] + monitor[2] - BALL_SIZE - 12)
        y = monitor[1] + 12
        return self._create_window(
            "Agent Activity Dock",
            BALL_SIZE,
            BALL_SIZE,
            x,
            y,
            self.override_redirect,
        )

    def _create_list_window(self) -> Window:
        height = TASK_LIST_HEADER_HEIGHT + max(1, len(self.view.tasks)) * TASK_LIST_ROW_HEIGHT + TASK_LIST_PADDING
        height = min(height, 560)
        monitor = self._monitor_rect
        x = max(0, monitor[0] + monitor[2] - TASK_LIST_WIDTH - 16)
        y = monitor[1] + BALL_SIZE + 20
        return self._create_window(
            "Agent Activity Dock tasks", TASK_LIST_WIDTH, height, x, y, False
        )

    def _detect_visible_monitor(self) -> tuple[int, int, int, int]:
        """Return an actually-visible monitor rectangle as (x, y, w, h).

        WSLg/XWayland can expose a root screen larger than the physical
        monitor and place the output at a non-zero offset (for example
        2560x1440+1600+512).  xrandr --listmonitors is the most reliable
        lightweight source; fall back to the full X screen when unavailable.
        """
        screen_w = _x11.XDisplayWidth(self.display, self.screen)
        screen_h = _x11.XDisplayHeight(self.display, self.screen)
        try:
            proc = subprocess.run(
                ["xrandr", "--listmonitors"],
                stdout=subprocess.PIPE,
                stderr=subprocess.DEVNULL,
                text=True,
                timeout=2,
            )
            for line in proc.stdout.splitlines():
                rect = parse_monitor_rect_from_xrandr_line(line)
                if rect is not None:
                    return rect
        except (OSError, subprocess.SubprocessError):
            pass
        return (0, 0, screen_w, screen_h)

    def _set_window_type_dock(self, window: Window) -> None:
        value = (Atom * 1)(self.atom_net_wm_window_type_dock)
        _x11.XChangeProperty(
            self.display,
            window,
            self.atom_net_wm_window_type,
            XA_ATOM,
            32,
            PropModeReplace,
            ctypes.cast(value, ctypes.c_void_p),
            1,
        )

    def _set_motif_no_decorations(self, window: Window) -> None:
        hints = (ctypes.c_ulong * 5)()
        hints[0] = 2  # MWM_HINTS_DECORATIONS
        hints[2] = 0  # no decorations
        _x11.XChangeProperty(
            self.display,
            window,
            self.atom_motif_wm_hints,
            self.atom_motif_wm_hints,
            32,
            PropModeReplace,
            ctypes.cast(hints, ctypes.c_void_p),
            5,
        )

    def _set_always_on_top_property(self, window: Window) -> None:
        states = (Atom * 2)(
            self.atom_net_wm_state_above,
            self.atom_net_wm_state_skip_taskbar,
        )
        _x11.XChangeProperty(
            self.display,
            window,
            self.atom_net_wm_state,
            XA_ATOM,
            32,
            PropModeReplace,
            ctypes.cast(states, ctypes.c_void_p),
            2,
        )

    def _request_always_on_top(self, window: Window) -> None:
        event = XEvent()
        event.xclient.type = ClientMessage
        event.xclient.window = window
        event.xclient.message_type = self.atom_net_wm_state
        event.xclient.format = 32
        event.xclient.data.l[0] = _NET_WM_STATE_ADD
        event.xclient.data.l[1] = self.atom_net_wm_state_above
        event.xclient.data.l[2] = self.atom_net_wm_state_skip_taskbar
        event.xclient.data.l[3] = 1  # normal application source
        _x11.XSendEvent(
            self.display,
            self.root,
            0,
            SubstructureRedirectMask | SubstructureNotifyMask,
            ctypes.byref(event),
        )

    # -- drawing --------------------------------------------------------

    def _set_color(self, color_name: str) -> None:
        _x11.XSetForeground(self.display, self.gc, self.colors[color_name])

    def _fill(self, window: Window, color_name: str, x: int, y: int, w: int, h: int) -> None:
        self._set_color(color_name)
        _x11.XFillRectangle(self.display, window, self.gc, x, y, w, h)

    def _draw_text(
        self,
        window: Window,
        color_name: str,
        x: int,
        y: int,
        text: str,
    ) -> None:
        self._set_color(color_name)
        _x11.XSetFont(self.display, self.gc, self.font.contents.fid)
        data = text.encode("utf-8")
        _x11.XDrawString(
            self.display, window, self.gc, x, y, data, len(data)
        )

    def _text_width(self, text: str) -> int:
        data = text.encode("utf-8")
        return _x11.XTextWidth(self.font, data, len(data))

    def redraw_ball(self) -> None:
        width, height = BALL_SIZE, BALL_SIZE
        border = self._flash_border or self.view.border_color
        self._fill(self.ball_window, border, 0, 0, width, height)
        self._fill(
            self.ball_window,
            BALL_BACKGROUND_COLOR,
            FRAME_WIDTH,
            FRAME_WIDTH,
            width - 2 * FRAME_WIDTH,
            height - 2 * FRAME_WIDTH,
        )
        label = self.view.count_label
        self._draw_text(
            self.ball_window,
            TEXT_COLOR,
            (width - self._text_width(label)) // 2,
            (height + self.font.contents.ascent) // 2,
            label,
        )
        if self.view.show_bang:
            self._draw_text(
                self.ball_window,
                FLASH_BORDER_COLOR,
                width - 8 - self._text_width("!"),
                3 + self.font.contents.ascent,
                "!",
            )
        _x11.XFlush(self.display)

    def redraw_list(self) -> None:
        width = TASK_LIST_WIDTH
        height = TASK_LIST_HEADER_HEIGHT + max(1, len(self.view.tasks)) * TASK_LIST_ROW_HEIGHT + TASK_LIST_PADDING
        height = min(height, 560)
        self._fill(self.list_window, BALL_BACKGROUND_COLOR, 0, 0, width, height)
        self._draw_text(
            self.list_window,
            TEXT_COLOR,
            8,
            18,
            "Agent Activity Dock — tasks (click to close)",
        )
        rows = min(len(self.view.tasks), 12)
        for index, task in enumerate(self.view.tasks[:rows]):
            mark = "!" if task.needs_attention else " "
            state = "working" if task.working else task.last_action
            terminal = f" -> {task.terminal}" if task.terminal else ""
            line = f"[{mark}] {task.task_id} ({task.source}): {state}{terminal}"
            line = line[:78]
            self._draw_text(
                self.list_window,
                TEXT_COLOR,
                8,
                TASK_LIST_HEADER_HEIGHT + 8 + index * TASK_LIST_ROW_HEIGHT,
                line,
            )
        if len(self.view.tasks) > rows:
            self._draw_text(
                self.list_window,
                TEXT_COLOR,
                8,
                TASK_LIST_HEADER_HEIGHT + 8 + rows * TASK_LIST_ROW_HEIGHT,
                f"... {len(self.view.tasks) - rows} more",
            )
        _x11.XFlush(self.display)

    # -- events ---------------------------------------------------------

    def _dispatch_event(self, event: XEvent) -> None:
        event_type = event.type
        if event_type == Expose:
            if event.xexpose.window == self.ball_window:
                self.redraw_ball()
            elif event.xexpose.window == self.list_window and self._list_mapped:
                self.redraw_list()
        elif event_type == ButtonPress:
            if event.xbutton.window == self.ball_window:
                if self._list_mapped:
                    self._hide_list()
                else:
                    self._show_list()
            elif event.xbutton.window == self.list_window:
                self._hide_list()
        elif event_type == KeyPress:
            if event.xkey.window == self.list_window:
                self._hide_list()

    def _show_list(self) -> None:
        self._list_mapped = True
        _x11.XMapRaised(self.display, self.list_window)
        self.redraw_list()
        if self.on_ball_click is not None:
            self.on_ball_click()

    def _hide_list(self) -> None:
        if not self._list_mapped:
            return
        self._list_mapped = False
        _x11.XUnmapWindow(self.display, self.list_window)
        if self.on_list_close is not None:
            self.on_list_close()

    @property
    def task_list_visible(self) -> bool:
        return self._list_mapped
