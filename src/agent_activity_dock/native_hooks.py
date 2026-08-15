"""Revocable native hook configuration for Agents that publish hooks.

The only MVP native-hook source is Claude Code's settings.json hook surface.
Hook commands receive structured metadata on stdin; they never open or read a
transcript path, prompt, tool input, or terminal output.
"""
from __future__ import annotations

import json
import os
import shutil
import sys
from pathlib import Path
from typing import Any, Optional

CLAUDE_HOOK_ACTION = {
    "SessionStart": "start",
    "PreToolUse": "start",
    "PermissionRequest": "waiting",
    "SessionEnd": "stop",
    "StopFailure": "error",
}


def claude_settings_path() -> Path:
    config_dir = os.environ.get("CLAUDE_CONFIG_DIR")
    base = Path(config_dir) if config_dir else Path.home() / ".claude"
    return base / "settings.json"


def _load_settings(path: Path) -> dict[str, Any]:
    if not path.exists():
        return {}
    try:
        value = json.loads(path.read_text())
    except (OSError, json.JSONDecodeError) as exc:
        raise RuntimeError(f"cannot read existing Claude settings {path}: {exc}") from exc
    if not isinstance(value, dict):
        raise RuntimeError(f"Claude settings {path} must contain a JSON object")
    return value


def _save_settings(path: Path, value: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    tmp = path.with_suffix(".json.aadock-tmp")
    tmp.write_text(json.dumps(value, indent=2) + "\n")
    tmp.chmod(0o600)
    tmp.replace(path)


def _is_dock_hook(entry: dict, hook_script: str) -> bool:
    hooks = entry.get("hooks")
    if not isinstance(hooks, list):
        return False
    for hook in hooks:
        if not isinstance(hook, dict):
            continue
        if hook.get("command") == hook_script:
            return True
        args = hook.get("args")
        if isinstance(args, list) and hook_script in args:
            return True
    return False


def install_claude_hooks(hook_script: Path) -> Optional[Path]:
    """Add Claude SessionStart/SessionEnd/StopFailure hooks.

    Existing settings are preserved and a timestamped backup is written before
    the first change.  Returns the backup path, or ``None`` if no file existed.
    """
    settings_file = claude_settings_path()
    settings = _load_settings(settings_file)
    backup: Optional[Path] = None
    if settings_file.exists():
        backup = settings_file.with_name(
            f"settings.json.aadock-backup-{os.getpid()}"
        )
        shutil.copy2(settings_file, backup)

    hooks = settings.setdefault("hooks", {})
    if not isinstance(hooks, dict):
        raise RuntimeError("existing Claude settings key 'hooks' is not an object")

    script = str(hook_script)
    for event in CLAUDE_HOOK_ACTION:
        entries = hooks.setdefault(event, [])
        if not isinstance(entries, list):
            raise RuntimeError(f"existing Claude hook entry '{event}' is not a list")
        entries[:] = [entry for entry in entries if not _is_dock_hook(entry, script)]
        entries.append(
            {
                "hooks": [
                    {
                        "type": "command",
                        "command": sys.executable,
                        "args": [script],
                    }
                ]
            }
        )

    _save_settings(settings_file, settings)
    return backup


def uninstall_claude_hooks(hook_script: Path) -> None:
    """Remove only the hook entries created by this Dock connection."""
    settings_file = claude_settings_path()
    if not settings_file.exists():
        return
    settings = _load_settings(settings_file)
    hooks = settings.get("hooks")
    if not isinstance(hooks, dict):
        return
    script = str(hook_script)
    for event in list(hooks):
        entries = hooks.get(event)
        if isinstance(entries, list):
            hooks[event] = [
                entry for entry in entries if not _is_dock_hook(entry, script)
            ]
            if not hooks[event]:
                del hooks[event]
    if not hooks:
        settings.pop("hooks", None)
    _save_settings(settings_file, settings)
