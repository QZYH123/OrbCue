"""Non-blocking local sound sinks.

Sound is an external boundary.  The production sink starts a short system
sound process and returns immediately; failure is reported by raising from
:meth:`play`, and :class:`AttentionDispatcher` turns that into a no-op so the
visible state is never rolled back.
"""
from __future__ import annotations

import math
import os
import shutil
import struct
import subprocess
import tempfile
import wave
from pathlib import Path

_SAMPLE_RATE = 44100
_BEEP_SECONDS = 0.09
_BEEP_HZ = 880


def _make_beep_wav(path: Path) -> None:
    frames = int(_SAMPLE_RATE * _BEEP_SECONDS)
    with wave.open(str(path), "wb") as wav:
        wav.setnchannels(1)
        wav.setsampwidth(2)
        wav.setframerate(_SAMPLE_RATE)
        data = bytearray()
        for index in range(frames):
            fade = min(1.0, index / 80, (frames - index) / 80)
            value = int(
                0.22 * 32767 * fade * math.sin(2 * math.pi * _BEEP_HZ * index / _SAMPLE_RATE)
            )
            data.extend(struct.pack("<h", value))
        wav.writeframes(bytes(data))


class NullSoundSink:
    """Silent sink used by tests and ``--no-sound``."""

    def play(self, reason: str) -> None:  # noqa: ARG002
        return None

    def close(self) -> None:
        return None


class PaplaySoundSink:
    """Play one short beep through Pulse/PipeWire without blocking state."""

    def __init__(self, command: str | None = None) -> None:
        self.command = command or self._find_player()
        self._wav_path: Path | None = None
        if self.command:
            self._prepare_wav()

    @staticmethod
    def _find_player() -> str | None:
        for candidate in ("paplay", "pw-play", "aplay", "ffplay"):
            found = shutil.which(candidate)
            if found:
                return found
        return None

    def _prepare_wav(self) -> None:
        fd, name = tempfile.mkstemp(prefix="agent-activity-dock-", suffix=".wav")
        os.close(fd)
        self._wav_path = Path(name)
        _make_beep_wav(self._wav_path)

    def play(self, reason: str) -> None:  # noqa: ARG002
        if self.command is None or self._wav_path is None:
            raise RuntimeError("no local sound player is available")
        command = [self.command]
        if Path(self.command).name in ("paplay", "pw-play"):
            command.append(str(self._wav_path))
        elif Path(self.command).name == "aplay":
            command.extend(["-q", str(self._wav_path)])
        else:  # ffplay or other compatible player
            command.extend(["-nodisp", "-autoexit", "-loglevel", "quiet", str(self._wav_path)])
        try:
            subprocess.Popen(
                command,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                stdin=subprocess.DEVNULL,
            )
        except OSError as exc:
            raise RuntimeError(f"cannot start sound player {self.command}: {exc}") from exc

    def close(self) -> None:
        if self._wav_path is not None:
            try:
                self._wav_path.unlink()
            except FileNotFoundError:
                pass
            self._wav_path = None
