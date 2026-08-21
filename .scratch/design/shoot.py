#!/usr/bin/env python3
"""Design self-check screenshots for the ball (64x64) and panel (360x500).

Mocks window.__TAURI_INTERNALS__ so App.svelte renders either window in a
plain browser. The ball page gets a clip-path circle to simulate the
SetWindowRgn hard clip.
"""
import asyncio
import json
import sys
from playwright.async_api import async_playwright

BASE = "http://127.0.0.1:1420"
OUT = "/home/qingz/projects/agent-activity-dock/.scratch/design"

SESSIONS = [
    {
        "source": "claude", "session_id": "a1b2c3d4-5e6f-7788-99aa-bbccddeeff00",
        "state": "working", "mark": "", "attention_reason": None,
        "summary": "重构 placement 模块并补充测试", "deep_link": None,
        "project_path": "/home/qingz/projects/agent-activity-dock",
        "window_title": None, "requires_user_action": False,
        "acknowledged": True, "occurred_at": "2026-08-21T08:21:00Z",
    },
    {
        "source": "codex", "session_id": "f0e1d2c3-b4a5-6677-8899-001122334455",
        "state": "needs_attention", "mark": "?", "attention_reason": "permission",
        "summary": "等待批准运行 cargo build --release", "deep_link": None,
        "project_path": "/home/qingz/projects/agent-activity-dock",
        "window_title": None, "requires_user_action": True,
        "acknowledged": False, "occurred_at": "2026-08-21T08:19:00Z",
    },
    {
        "source": "grok", "session_id": "12345678-aaaa-bbbb-cccc-dddddddddddd",
        "state": "failed", "mark": "!", "attention_reason": None,
        "summary": "vitest 有 2 个断言失败，需要人工确认", "deep_link": None,
        "project_path": "/home/qingz/projects/side-quest",
        "window_title": None, "requires_user_action": True,
        "acknowledged": False, "occurred_at": "2026-08-21T08:10:00Z",
    },
    {
        "source": "claude", "session_id": "99999999-8888-7777-6666-555555555555",
        "state": "completed", "mark": "*", "attention_reason": None,
        "summary": None, "deep_link": None,
        "project_path": "/home/qingz/projects/side-quest",
        "window_title": None, "requires_user_action": False,
        "acknowledged": False, "occurred_at": "2026-08-21T07:58:00Z",
    },
]

AUDIT = [
    {"source": "claude", "session_id": "a1b2c3d4-5e6f-7788-99aa-bbccddeeff00",
     "state": "working", "attention_reason": None, "occurred_at": "2026-08-21T08:21:00Z"},
    {"source": "codex", "session_id": "f0e1d2c3-b4a5-6677-8899-001122334455",
     "state": "needs_attention", "attention_reason": "permission", "occurred_at": "2026-08-21T08:19:00Z"},
    {"source": "grok", "session_id": "12345678-aaaa-bbbb-cccc-dddddddddddd",
     "state": "failed", "attention_reason": None, "occurred_at": "2026-08-21T08:10:00Z"},
]

PANEL_SNAPSHOT = {
    "working_count": 1, "tracked_count": 4, "pending_count": 3,
    "pending_mark": "?", "count_label": "1/4", "border_state": "working",
    "sessions": SESSIONS, "audit": AUDIT,
}

INVENTORY = {
    "discovered": [
        {"name": "claude", "path": "/home/qingz/.local/bin/claude", "origin": "wsl", "connectable": True},
        {"name": "codex", "path": "/usr/local/bin/codex", "origin": "wsl", "connectable": True},
        {"name": "grok", "path": "C:/Users/qingz/bin/grok.exe", "origin": "windows", "connectable": False},
    ],
    "connected": [
        {"name": "claude", "original": "/home/qingz/.local/bin/claude", "method": "ClaudeHook",
         "wrapper": None, "hook_script": "~/.claude/hooks/dock.sh", "settings_backup": None,
         "capabilities": ["state"], "limitation": "仅上报状态变化，不读取会话内容",
         "installed_at": "2026-08-20T10:00:00Z"},
    ],
}


def ball_snapshot(count_label, working, mark):
    return {
        "working_count": working, "tracked_count": 12, "pending_count": 2,
        "pending_mark": mark, "count_label": count_label,
        "border_state": "working" if working else "idle",
        "sessions": [], "audit": [],
    }


MOCK_TEMPLATE = """
window.__TAURI_INTERNALS__ = {
  metadata: { currentWindow: { label: __LABEL__ }, currentWebview: { label: __LABEL__ } },
  transformCallback: () => Math.floor(Math.random() * 1e9),
  unregisterCallback: () => {},
  invoke: (cmd) => {
    if (cmd === 'snapshot') return Promise.resolve(__SNAPSHOT__);
    if (cmd === 'agent_inventory' || cmd === 'refresh_agents') return Promise.resolve(__INVENTORY__);
    if (cmd === 'preview_connect') return Promise.resolve({
      name: 'codex', original: '/usr/local/bin/codex', method: 'Wrapper', dry_run: true,
      files: [{ path: '~/.local/bin/codex', action: 'create', entries: ['wrapper 调用原命令并透传参数', '上报 start/stop 事件'] }],
      will_not: ['不修改原可执行文件', '不读取会话内容'],
      notes: ['断开连接会移除 wrapper 并还原 PATH 顺序'],
    });
    if (cmd === 'plugin:event|listen') return Promise.resolve(0);
    if (cmd === 'plugin:autostart|is_enabled') return Promise.resolve(true);
    if (cmd === 'plugin:global-shortcut|is_registered') return Promise.resolve(true);
    return Promise.resolve(null);
  },
};
window.localStorage.setItem('onboarding-complete', __ONBOARDED__);
"""

# Simulate the OS circular region clip plus a desktop behind the window.
BALL_PREVIEW_CSS = """
body { background: __DESKTOP__; }
#app { clip-path: circle(32px at 32px 32px); }
"""

PANEL_PREVIEW_CSS = "body { background: __DESKTOP__; }"

DESKTOPS = {
    "dark": "linear-gradient(135deg, #10151c, #232c38)",
    "light": "linear-gradient(135deg, #cfd8e3, #eef2f7)",
}


def mock_js(label, snapshot, onboarded=True):
    return (
        MOCK_TEMPLATE
        .replace("__LABEL__", json.dumps(label))
        .replace("__SNAPSHOT__", json.dumps(snapshot))
        .replace("__INVENTORY__", json.dumps(INVENTORY))
        .replace("__ONBOARDED__", json.dumps("true" if onboarded else "false"))
    )


async def shoot_ball(browser, name, snapshot, desktop="dark"):
    ctx = await browser.new_context(viewport={"width": 64, "height": 64}, device_scale_factor=4)
    page = await ctx.new_page()
    await page.add_init_script(mock_js("ball", snapshot))
    await page.goto(BASE)
    await page.wait_for_timeout(400)
    await page.add_style_tag(content=BALL_PREVIEW_CSS.replace("__DESKTOP__", DESKTOPS[desktop]))
    await page.wait_for_timeout(100)
    path = f"{OUT}/ball-{name}.png"
    await page.screenshot(path=path)
    print(path)
    await ctx.close()


async def shoot_panel(browser, name, actions=None, onboarded=True):
    ctx = await browser.new_context(viewport={"width": 360, "height": 500}, device_scale_factor=2)
    page = await ctx.new_page()
    await page.add_init_script(mock_js("panel", PANEL_SNAPSHOT, onboarded=onboarded))
    await page.goto(BASE)
    await page.wait_for_timeout(500)
    await page.add_style_tag(content=PANEL_PREVIEW_CSS.replace("__DESKTOP__", DESKTOPS["dark"]))
    if actions:
        await actions(page)
        await page.wait_for_timeout(300)
    path = f"{OUT}/panel-{name}.png"
    await page.screenshot(path=path)
    print(path)
    await ctx.close()


async def main():
    only = sys.argv[1] if len(sys.argv) > 1 else None
    async with async_playwright() as p:
        browser = await p.chromium.launch()
        if only in (None, "ball"):
            await shoot_ball(browser, "idle", ball_snapshot("0/0", 0, ""))
            await shoot_ball(browser, "working", ball_snapshot("3/5", 3, ""))
            await shoot_ball(browser, "attention", ball_snapshot("2/6", 2, "?"))
            await shoot_ball(browser, "fail", ball_snapshot("0/4", 0, "!"))
            await shoot_ball(browser, "wide", ball_snapshot("10/12", 10, "?"))
            await shoot_ball(browser, "wide-light", ball_snapshot("10/12", 10, "?"), desktop="light")
            await shoot_ball(browser, "idle-light", ball_snapshot("0/0", 0, ""), desktop="light")
        if only in (None, "panel"):
            await shoot_panel(browser, "activity")
            await shoot_panel(browser, "audit", lambda pg: pg.click("text=审计"))
            await shoot_panel(browser, "connections", lambda pg: pg.click("text=连接"), onboarded=False)

            async def open_dialog(pg):
                await pg.click("nav >> text=连接")
                await pg.click("button.primary-button")

            await shoot_panel(browser, "dialog", open_dialog)
            await shoot_panel(browser, "settings", lambda pg: pg.click("nav >> text=设置"))
        await browser.close()


asyncio.run(main())
