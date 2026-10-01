<div align="center">

<img src="src-tauri/icons/icon.svg" width="96" alt="OrbCue icon">

# OrbCue

<h3>Every context switch should be worth it.</h3>

An orb on the Windows desktop that watches the AI assistants in your terminals.<br>
While they are busy, it only counts. It speaks when it is your turn.

<sub>The orb that cues you when an agent needs you.</sub>

<p>
  <a href="https://github.com/QZYH123/OrbCue/releases/latest"><img src="https://img.shields.io/github/v/release/QZYH123/OrbCue?label=latest" alt="Latest"></a>
  <a href="LICENSE"><img src="https://img.shields.io/github/license/QZYH123/OrbCue" alt="License: MIT"></a>
  <a href="#requirements"><img src="https://img.shields.io/badge/Windows-10%20%2F%2011-0078D6?logo=windows&logoColor=white" alt="Windows 10 / 11"></a>
  <a href="#supported-tools"><img src="https://img.shields.io/badge/CLI-Claude%20%7C%20Codex%20%7C%20Cursor%20%7C%20Grok-555" alt="CLI: Claude, Codex, Cursor, Grok"></a>
</p>

[中文](README.md) · [**Download**](https://github.com/QZYH123/OrbCue/releases/latest) · [Quick start](#quick-start) · [The interface](#the-interface) · [Privacy and data](#privacy-and-data) · [FAQ](#faq)

<br>

<img src="docs/screenshots/demo.gif" width="720" alt="While an agent works, the orb only counts. When it is your turn, it changes color and shows a notification. Open the panel to see who is busy and who is waiting. Click a row to jump back to that terminal.">

</div>

<br>

## Why it exists

With several AI assistants open, it feels like parallel work. In reality, it is mostly switching windows: you look over, and that one is still running; you look back, and another has been waiting.

OrbCue does the watching. The orb is what you see. The cue is its job: one alert when it is your turn to act.

- **A quiet count.** The orb shows working primary sessions over total primary sessions in the current list, for example `2/5`.
- **It speaks when it is your turn.** Waiting for input, Waiting for approval, a failure, or a normal finish each triggers one system notification and plays a sound.
- **See it at a glance.** Click the orb to open the panel. Sessions are grouped by project, and you can filter by All / Working / Not working.
- **Back to the terminal in one click.** The back arrow on a row jumps to that terminal.
- **Stay informed away from your desk.** Optional: the same alert can go to [ntfy](https://ntfy.sh/) on your phone.
- **Rest at the edge when not in use.** Drag it to a screen edge and it becomes a semicircle. Hover over it, and it slides out.
- **Five looks:** Prototype, Fluent, Glyph, Braun, and Glass.
- **It does not touch your data.** It does not read transcripts, prompts, commands, or code, and it does not operate those tools for you. It stays offline by default. State stays on this machine.

## Supported tools

| Tool | Command | States it can report |
| --- | --- | --- |
| Claude Code | `claude` | start, waiting, done, failed, closed |
| Grok Build | `grok` | start, waiting, done, failed, closed |
| Codex | `codex` | start, waiting, done, closed (an interrupt and a chat error are not visible) |
| Cursor Agent | `agent` / `cursor-agent` | start, done, failed, closed (a multiple-choice prompt is not marked as waiting) |

These are the command-line (CLI) tools, not the Cursor editor. While the editor (including WSL Remote) and the CLI share the user-level `~/.cursor/hooks.json`, OrbCue only recognizes the `cursor-agent` process. Agents and subagents inside the editor do not appear on the orb. Tools installed on Windows or in WSL can both be connected, and both show on the same orb. Other tools can [connect on their own](#connect-another-tool).

## Quick start

### Requirements

- **Windows 10 / 11 (x64).** The orb and the panel run only on Windows. There is no official macOS or Linux desktop app yet.
- **WSL (optional).** With WSL installed, tools inside WSL and tools on Windows show on the same orb. OrbCue still works if WSL is not installed.
- **Windows Terminal (optional).** You need it only when you want to jump back to an exact tab. See [Jump back to the exact tab](#jump-back-to-the-exact-tab).

### Install

Download the NSIS installer from [GitHub Releases](https://github.com/QZYH123/OrbCue/releases/latest). After it is installed, start OrbCue. The orb appears at the bottom right of the desktop. Only one OrbCue instance runs per user. To start with Windows, turn on Start at login in Settings.

You can also build it from source. See [Build from source](#build-from-source).

### First run

1. Click the orb to open the panel (or press `Ctrl+Shift+Space`). In the bottom bar, open Connect.
2. On the tool you want, click Connect, review the files that would change, then click Connect in that dialog. If the tool was not found, click Add folder.
3. **Open a new terminal** and use the tool as you already do. A session that was already running before you connected does not appear on the orb.
4. After a task starts, the orb shows a number. When you need to act, `?` (waiting) or `!` (failed) appears at the top right.

To quit: tray icon → Quit.

## The interface

Click the orb to open the panel. Click the orb again, or click the × at the top right of the panel, to close it. From the tray icon, you can Open OrbCue, Show orb, Hide orb, or Quit.

<p align="center">
  <img src="docs/screenshots/panel-activity.png" width="300" alt="Activity lists sessions grouped by project">
  &nbsp;&nbsp;
  <img src="docs/screenshots/panel-connect.png" width="300" alt="Connect lists tools on this machine and marks each one Windows or WSL">
</p>
<p align="center"><sub>Left: Activity. Right: Connect.</sub></p>

| Page | What it does |
| --- | --- |
| Orb | Stays on the desktop. The number shows working / tracked sessions. `?` means a task is waiting for you; `!` means a task failed. Drag it to a screen edge and it rests as a semicircle. |
| Activity | The current sessions, grouped by project. Filter by All / Working / Not working. |
| Audit | The latest done, failed, waiting, and closed records in this run, up to 128. They stay in memory only and are cleared on restart. |
| Connect | Tools found on this machine. Review them, then connect or disconnect. |
| Settings | Look, alerts, how the orb behaves, and how it starts. See [Settings](#settings). |

### Activity

Each session shows only the tool name, the project, and the state. It does not show conversation contents.

- **Mark read:** clears the current waiting mark.
- **Clear:** removes a stuck row from the list. This only changes what OrbCue shows. It sends no command to the tool. A cleared row does not come back.
- **Back arrow:** jumps to the terminal. The next section explains where that jump lands.
- **Mark all read** and **Clear all** in the bottom bar act on the current list.
- When the tool process exits, the row disappears on its own.
- A subtask counts toward its parent task. It does not take its own number on the orb.

### Jump back to the exact tab

Where the back arrow lands depends on how the terminal was opened.

| How the terminal was opened | What the back arrow does |
| --- | --- |
| A Windows Terminal tab opened with `orb run`, Original command, or Run alias | Jumps to that exact tab. This still works after the tab was dragged out or merged. |
| An ordinary terminal you opened yourself | Jumps to the place in that window you used most recently. If the window is already closed, OrbCue reports a failure and does not jump somewhere else. |

Window-level jumping works for Windows Terminal, a standalone cmd or PowerShell window, Alacritty, WezTerm, Git Bash (mintty), and Tabby. It does not work for the built-in terminal in VS Code, Cursor, or other editors.

Three ways to open a dedicated tab (any one is enough):

- **Original command.** Turn it on in Settings. In a new terminal, type a connected command (`claude`, `codex`, or `grok`; Cursor uses `agent` or `cursor-agent`). That is the same as `orb run` plus that tool.
- **Run alias.** In Settings, give `orb run` a short command.
- **Type it.** In a new Windows Terminal, run `orb run grok`.

Before using this for the first time, start OrbCue once and **open the terminal again**. After Original command is on, `command grok` still runs the original program in the current terminal.

### Connect

Each row is one tool OrbCue found. The row says whether it runs on Windows or in WSL, and whether it is Available or Connected.

- **Connect.** The first click lists the files that would change. Nothing is written until you click Connect again in that dialog.
- **Disconnect.** Removes only what OrbCue itself wrote. It does not change other settings you edited later.
- **Refresh.** Rescans for tools installed on this machine.
- **Add folder.** If the usual install path has nothing, pick the folder that contains the executable.

The same tool installed once on Windows and once in WSL is two rows. Connect them separately.

Connect scans the PATH the desktop app can read, the user PATH, and common install paths (for example `%USERPROFILE%\.local\bin`, Grok Build at `%USERPROFILE%\.grok\bin`, and the Cursor command line at `%LOCALAPPDATA%\cursor-agent`). When WSL is installed, it also scans the PATH inside WSL. Windows programs that are visible from WSL are not counted a second time.

### Settings

| Setting | Default | What it does |
| --- | :---: | --- |
| **Alerts** | | |
| Sounds | On | Done sound, Waiting sound, and Failure sound are separate switches |
| System notifications | On | One system notification for Waiting for input, Waiting for approval, or a failed task |
| Completion alerts | On | On a normal finish, the PC and the phone are each notified once. Turn this off and a finish only plays a sound. If System notifications are off, the PC does not show a notification, and the phone still follows this switch |
| Phone alerts | Off | Paste an ntfy topic URL. Waiting for input, Waiting for approval, and a failure each push once. A normal finish follows Completion alerts. Leave it empty and OrbCue stays offline. What is sent is in [Privacy and data](#privacy-and-data) |
| **Orb** | | |
| Language | System | System, 中文, or English. The panel, notifications, tray, and orb commands change together |
| Appearance | | Prototype, Fluent, Glyph, Braun, and Glass |
| Hide badge | Off | The orb no longer shows `?` and `!` |
| Snap to edge | On | Drag within about one orb's width of a screen edge and it snaps there: a translucent semicircle with no number. Hover, and it slides out along that same edge; move away and it snaps back. While `?` or `!` is showing it stays open, until you drag it to the edge again. Turn this off and dragging is ordinary |
| **Start** | | |
| Original command | Off | In a new terminal, typing a connected command is the same as `orb run` plus that tool. The original program is not changed |
| Run alias | | A short command for `orb run`, so you can jump back to the exact tab |
| Replace this tab | Off | After the new tab opens, close the current tab. This is the same as `orb run --close`, and it works only in an interactive terminal |
| Start at login | Off | Start OrbCue when you sign in to Windows |
| Global shortcut | On | `Ctrl+Shift+Space` opens or closes the panel |

<p align="center">
  <img src="docs/screenshots/themes.png" alt="Five looks: Prototype, Fluent, Glyph, Braun, and Glass">
</p>

## What connecting changes

Each tool connects through the hook it already has. OrbCue registers that hook in the tool's own config. It does not replace any executable.

- Before it writes, it lists the files that would change. It writes only after you confirm.
- The first time it edits the config for Claude Code, Codex, or Cursor, it keeps a backup, for example `settings.json.orbcue.bak`.
- Disconnect removes only what OrbCue itself wrote.
- If connecting fails, nothing is downloaded or reinstalled.

Known limits. The matching row on Connect notes these too:

- **Cursor.** Print mode (`agent -p`) has no end-of-turn notice. The row stays until the process exits. An interactive turn is marked Done.
- **Codex.** Esc or Ctrl+C while it is replying does not notify OrbCue, so the task stays on Working. A chat error is not shown as Failed. Click Clear on Activity. After you quit Codex, the task also leaves the list.
- **Claude Code and Codex.** Deny in an approval prompt does not notify OrbCue. The orb stays on Needs approval until the tool continues or that turn ends. You can also click Clear on Activity. Grok Build returns to Working as soon as you deny.

## Privacy and data

OrbCue only receives status changes a tool sends on purpose: it started, it is waiting for you, or it finished.

**What it does not do**

- It does not read transcripts, prompts, commands, code, terminal output, or secrets. The panel does not show a conversation summary. It shows the tool name, the project, and the state.
- It does not scan the process list to guess whether a tool is working. To see whether a process has exited, it looks up only the one process recorded when you connected.
- It does not listen on a network port. By default it uploads nothing.

**What is stored, and where**

- It talks only on this machine, and only for the current user. Windows uses a named pipe. The WSL command line uses a Unix socket.
- What it writes to disk is only: source, task id, state, time, the read mark, the terminal mark, and the project path.
  - Windows: `%LOCALAPPDATA%\OrbCue\state.json`
  - WSL / Linux command line: `$XDG_STATE_HOME/orbcue/state.json` (by default under `~/.local/state`)
- A task summary exists only briefly in memory. It is not written to disk.
- After a restart that minimal state comes back, but old alerts are not played again. Expired events are dropped.

**Phone alerts (optional)**

- Off by default. The topic URL is saved at `%LOCALAPPDATA%\OrbCue\phone-notify.url`.
- A push is only Waiting for input, Waiting for approval, Task failed, or Task finished, plus the tool name. If there is a project, the project folder name is added. The full path, the conversation, and the commands are not sent.
- Whether a finished task is sent follows Completion alerts. Clear the address to stop pushing.
- Use a long topic name. Anyone who knows that topic URL can see these alerts.

A problem with sounds, with drawing the interface, or with connecting one tool does not stop the background status service. A stuck row can be cleared on Activity at any time.

## FAQ

<details>
<summary><b>A tool that was already running before I connected does not appear on the orb.</b></summary>

<br>OrbCue does not go back and add it. After you connect, open the terminal again and start the tool.

</details>

<details>
<summary><b>A task stays on Working.</b></summary>

<br>The tool may have been killed, or it did not tell OrbCue that it finished (for example, Esc to interrupt Codex). Click Clear on Activity. When the process really exits, OrbCue removes the row on its own.

</details>

<details>
<summary><b>I clicked Deny in the approval prompt, and it still says Needs approval.</b></summary>

<br>Claude Code and Codex do not tell OrbCue about that denial. It recovers when the tool continues, or when the current turn ends. You can also click Clear on Activity. Grok Build does not have this problem.

</details>

<details>
<summary><b>The back arrow cannot find the window, or it does not land on the tab I wanted.</b></summary>

<br>A terminal you opened yourself can only jump back to the window you used most recently. To land on one exact tab, start the tool in Windows Terminal with Original command, a Run alias, or `orb run`. See [Jump back to the exact tab](#jump-back-to-the-exact-tab).

</details>

<details>
<summary><b>The orb is gone.</b></summary>

<br>Check the tray icon. Hide orb may be on; click Show orb, or press `Ctrl+Shift+Space` to open the panel. With Snap to edge on, the orb may also be a translucent semicircle on a screen edge. Look along the edges.

</details>

<details>
<summary><b>Cursor reports a hook failure, or the task does not show on the orb.</b></summary>

<br>Cursor CLI treats an empty hook result, or a result that is not JSON, as a failure. OrbCue returns an empty JSON object so Cursor does not report that error itself. If it is still wrong, Disconnect on Connect and connect again.

</details>

<details>
<summary><b>Nothing pops up when I need to act.</b></summary>

<br>In Settings, turn on System notifications. In Windows, go to Settings → System → Notifications and allow OrbCue. If a normal finish does not reach the PC or the phone, also check that Completion alerts are on.

</details>

<details>
<summary><b>Can the phone alert me when I am away from the computer?</b></summary>

<br>Yes. On an Android phone, install ntfy and subscribe to a long topic that only you know. Paste `https://ntfy.sh/your-topic` into Phone alerts in Settings and click Apply. The phone receives one test push first.

After that, Waiting for input, Waiting for approval, a failure, and a normal finish when Completion alerts are on, each push once. A push contains only the current cue, the tool name, and the project folder name. OrbCue has to keep running, and the computer has to be able to reach that address. Leave the field empty and nothing is pushed.

</details>

<details>
<summary><b>Does it work without WSL?</b></summary>

<br>Yes. Without WSL, Connect shows only tools in Windows.

</details>

<details>
<summary><b>Connect cannot find a Windows tool that works when I run it.</b></summary>

<br>Tools such as fnm and nvm inject PATH only inside a particular terminal setup. The desktop app cannot see that. Click Refresh first. An official install usually writes into your user directory. If it is still missing, click Add folder and pick the folder that contains the executable.

</details>

<details>
<summary><b>Can OrbCue see my conversations?</b></summary>

<br>No. It only receives status changes a tool sends on purpose: it started, it is waiting for you, or it finished. See [Privacy and data](#privacy-and-data).

</details>

## Uninstall

1. On Connect, click Disconnect for each tool. That removes the hooks OrbCue wrote.
2. Tray icon → Quit. If you used the installer, then uninstall it in Windows Settings → Apps.
3. To remove what is left, delete `%LOCALAPPDATA%\OrbCue`. If you used WSL, also delete `~/.local/bin/orb`, and the block in your shell config marked `# >>> orbcue PATH >>>`.

## Connect another tool

Connect only has presets for the tools listed above. Other tools that send their own status use these commands with one session id. You do not need to change OrbCue's code. `orb stop` and `orb completed` match `orb complete`. `orb error` matches `orb fail`.

```bash
orb start <stable-session-id> --source <tool-name>
orb waiting <stable-session-id> --source <tool-name>
orb permission <stable-session-id> --source <tool-name>
orb complete <stable-session-id> --source <tool-name>
orb fail <stable-session-id> --source <tool-name>
```

## Build from source

You need Rust 1.80 or newer, Node.js 20 or newer, and npm. The desktop app uses Tauri 2. The interface uses Svelte 5. To run the desktop app on Windows:

```bash
npm ci --prefix frontend
npm run tauri -- dev
```

`npm --prefix frontend run dev` is only a preview of the interface in a browser, using sample data. It is not the full OrbCue.

<br>

---

<div align="center">

Thanks to the [LINUX DO](https://linux.do/) community for the help while I learned about AI.

<sub>License: [MIT](LICENSE)</sub>

</div>
