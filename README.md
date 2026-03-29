# VibeMux

A GPU-accelerated terminal multiplexer for Windows, built in Rust. Inspired by [cmux](https://cmux.com/) but targeting Windows with [WezTerm](https://github.com/wezterm/wezterm)'s terminal crates (`portable-pty`, VTE parsing) and [iced](https://iced.rs) for the UI.

## Features

- **Vertical tab sidebar** showing workspace name, git branch, working directory, status pills, and progress bars
- **Split panes** -- horizontal and vertical splits within each workspace
- **Notification system** -- OSC 9/99/777 escape sequence parsing, Windows toast notifications, sidebar unread badges
- **Named pipe socket API** -- JSON-RPC protocol at `\\.\pipe\vibemux` for automation
- **CLI tool** -- `vibemux` command for scripting (list-workspaces, notify, send, etc.)
- **Command palette** -- fuzzy-search over all commands (Ctrl+Shift+P)
- **Find in terminal** -- search scrollback (Ctrl+F)
- **Session restore** -- autosaves layout and metadata every 30 seconds
- **TOML configuration** -- font, theme, keybindings at `%APPDATA%\vibemux\config.toml`
- **GPU-accelerated** -- iced/wgpu rendering with Catppuccin Mocha theme
- **Git branch detection** -- automatic branch display in sidebar via libgit2

## Keyboard Shortcuts

| Action | Shortcut |
|---|---|
| New Workspace | Ctrl+Shift+N |
| Close Workspace | Ctrl+Shift+W |
| Next Workspace | Ctrl+Tab |
| Split Right | Ctrl+Shift+D |
| Split Down | Ctrl+Shift+E |
| Close Pane | Ctrl+Shift+Q |
| Focus Next Pane | Alt+Tab |
| Command Palette | Ctrl+Shift+P |
| Find | Ctrl+F |
| Notification Panel | Ctrl+Shift+I |

## Building

Requires Rust 1.88+ and Windows 10 1809+ (for ConPTY).

```bash
cargo build --release
```

Binaries:
- `target/release/vibemux-app.exe` -- the GUI application
- `target/release/vibemux.exe` -- the CLI tool

## CLI Usage

```bash
# Check if VibeMux is running
vibemux ping

# List workspaces
vibemux list-workspaces

# Create a workspace
vibemux new-workspace --name "My Project"

# Send a notification
vibemux notify --title "Build Done" --body "All tests passed"

# Send text to the focused terminal
vibemux send "echo hello\n"

# Send a key press
vibemux send-key enter
```

## Socket API

VibeMux exposes a JSON-RPC API over a Windows named pipe at `\\.\pipe\vibemux`.

```json
{"id":"1","method":"workspace.list","params":{}}
{"id":"2","method":"notification.create","params":{"title":"Done","body":"Task complete"}}
{"id":"3","method":"sidebar.set_status","params":{"key":"build","value":"compiling"}}
{"id":"4","method":"sidebar.set_progress","params":{"value":0.5,"label":"Building..."}}
```

## Configuration

Create `%APPDATA%\vibemux\config.toml`:

```toml
[font]
family = "Cascadia Code"
size = 14.0

[appearance]
theme = "Dark"
unfocused_pane_opacity = 0.7
sidebar_width = 220.0

[terminal]
scrollback_limit = 10000

[keybindings]
new_workspace = "Ctrl+Shift+N"
split_right = "Ctrl+Shift+D"
split_down = "Ctrl+Shift+E"
```

## Architecture

```
vibemux/
  crates/
    vibemux-app/      GUI application (iced + wgpu)
    vibemux-term/     Terminal emulation (VTE parser, grid, PTY)
    vibemux-mux/      Multiplexer (workspaces, panes, split tree)
    vibemux-ipc/      IPC server (named pipes, JSON-RPC)
    vibemux-cli/      CLI binary
    vibemux-config/   TOML configuration
```

## License

MIT
