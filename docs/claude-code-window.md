# Claude Code window

The built-in **Claude Code** window shows which Claude Code sessions are
waiting for you: a tool call that needs approval, a question that needs input,
or a finished answer. Sessions that are working are not listed. Both companion
apps provide the window; the firmware shows it like a display plugin, so any
firmware with `sceneProtocol: 1` works without an update.

## How it works

Claude Code sends [HTTP hooks](https://code.claude.com/docs/en/hooks) to the
companion. The companion listens on `127.0.0.1:47651` only and accepts
`POST /claude-code` with the header `X-AIMonitor-Token`. The token is generated
per installation. Every accepted request gets an empty `200` response, so the
hooks never approve, block, or change anything in Claude Code. Requests without
the right token get `401`.

| Hook | Display state |
|---|---|
| `PermissionRequest`, `Notification` with `permission_prompt` | Needs approval |
| `Notification` with `idle_prompt`, `agent_needs_input`, `elicitation_dialog`, `elicitation_url_dialog`; `StopFailure` | Needs input |
| `Stop`, `Notification` with `agent_completed` | Done |
| `UserPromptSubmit`, `PostToolUse`, `PostToolUseFailure`, `SessionStart` | Working (not listed) |
| `SessionEnd` | Removed |

An `idle_prompt` after `Stop` keeps the session as done. A session without
events for 12 hours is dropped. The window lists up to four sessions, approval
first, then input, then done, oldest first within each group, with the project
folder name and how long it has been waiting. Folder names are shown as
printable ASCII (`Größe` becomes `Groesse`).

The window lists sessions when the hooks are in `~/.claude/settings.json` or
when an event with the right token arrived within the last 12 hours. Hooks in
a project's settings therefore work too. Otherwise the window asks you to set
up the hooks. If events arrive although `~/.claude/settings.json` has no hooks,
the Plugins page says so instead of showing a warning.

## Setup

1. Open **Plugins** in the companion and choose **Set up hooks**. After a
   confirmation, the companion adds one HTTP hook per event to
   `~/.claude/settings.json`. Existing entries are kept, the previous file is
   saved as `settings.json.aimonitor-backup`, and invalid JSON is never
   overwritten. The file is rewritten with sorted keys.
2. Add the **Claude Code** window in the window manager.
3. Restart running Claude Code sessions so they load the hooks.

**Remove hooks** deletes only the companion's entries. They are recognized by
an HTTP hook to `127.0.0.1` with the path `/claude-code`. If the token or the
event list changes, the Plugins page reports the hooks as outdated.

If the port is in use, the window says so and the Plugins page shows the error.

## Implementation

Event handling, the scene, request parsing, and the settings update live in
`companion-windows/crates/core/src/claude_code.rs`. The Mac app mirrors state
and scene in `companion/Sources/ClaudeCodeWindow.swift` and updates
`settings.json` through `aimonitor-plugin-host claude-hooks`. The window key is
`plugin:builtin.claude-code`; installed plugins cannot use IDs that start with
`builtin.`.
