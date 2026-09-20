<h1 align="center">Clauditty - An AI-native terminal</h1>

## About

Clauditty is a fast, lightweight terminal built for coding agents like
[Claude Code](https://claude.com/claude-code). It is a fork of
[Alacritty](https://github.com/alacritty/alacritty).

It comes with sensible defaults and a flexible [config](#configuration). See the
[features](./docs/features.md) overview for what it can do.

## Tabs and panes

Tabs live in a sidebar on the left. Agent tabs run Claude Code. Terminal tabs
and split panes run your shell, and a new window starts with one. The focused
pane has an orange border.

Each tab card shows the app in the focused pane, a preview of its last lines
and its working directory.

### Ready queue

The sidebar groups tabs into three sections:

- **Ready**: finished while you were away, oldest first
- **Working**: an agent is printing, or a command is running
- **Idle**: nothing to do

`Cmd+1` always opens the top tab, so the loop is: `Cmd+1`, read, reply, repeat.
Pressing `Enter` in a ready tab marks it as read. `Cmd+Enter` marks it as read
and jumps to the first other tab in the sidebar.

Claude Code counts as finished after 2 seconds without output. A command counts
once the prompt is back, if it ran for 5 seconds or more.

| Shortcut            | Action                             |
| ------------------- | ---------------------------------- |
| `Cmd+Shift+T`       | New Claude Code tab                |
| `Cmd+T`             | New terminal tab                   |
| `Cmd+D`             | Split pane to the right            |
| `Cmd+Shift+D`       | Split pane down                    |
| `Cmd+W`             | Close pane, or tab if last pane    |
| `Cmd+Enter`         | Mark tab read, go to next tab      |
| `Cmd+Arrow`         | Move to the pane in that direction |
| `Cmd+Ctrl+Arrow`    | Resize the focused pane            |
| `Cmd+1` to `Cmd+9`  | Switch tab                         |
| `Cmd+Shift+[` / `]` | Previous / next tab                |

`Cmd+Up` on the top pane goes to the previous tab, and `Cmd+Down` on the bottom
pane goes to the next tab.

Click a tab to switch to it, or a pane to focus it. Drag the line between panes
to resize them.

Change the agent tab command with `tabs.command` in the config.

### Session restore

Quitting stores your tabs, panes and their folders. The next run reopens them,
and Claude Code tabs come back with `claude --resume`, so the conversation
carries on. Closing every tab clears the stored session.

### Alerts

When a tab becomes ready while Clauditty is in the background, the Dock icon
bounces and shows how many tabs are waiting. Turn either off with
`alerts.bounce` and `alerts.badge`.

### Harnesses

Each pane is shown through a harness, like Claude Code or a plain terminal. A
harness sets how the pane is detected, its name, icon and accent color, how its
activity is tracked and what its tab previews.

To support another agent, implement the `Harness` trait in
`clauditty/src/harness/` and add it to `AGENTS` in `harness/mod.rs`.

## Build and run

You need [Rust](https://www.rust-lang.org/tools/install) 1.85 or newer.

```sh
cargo run
```

To rebuild and restart on every save:

```sh
brew install watchexec
watchexec -r -e rs,toml,glsl -- cargo run
```

To build a macOS app (`target/release/osx/Clauditty.app`):

```sh
make app
```

More build details are in [INSTALL.md](INSTALL.md).

### Requirements

- At least OpenGL ES 2.0
- [Windows] ConPTY support (Windows 10 version 1809 or higher)

## Configuration

See `man 5 clauditty` for all config options.

Clauditty doesn't create the config file for you, but it looks for one in the
following locations:

1. `$XDG_CONFIG_HOME/clauditty/clauditty.toml`
2. `$XDG_CONFIG_HOME/clauditty.toml`
3. `$HOME/.config/clauditty/clauditty.toml`
4. `$HOME/.clauditty.toml`
5. `/etc/clauditty/clauditty.toml`

On Windows, the config file will be looked for in:

* `%APPDATA%\clauditty\clauditty.toml`

Changes to the config file apply right away, with no restart.

## License

Clauditty is released under the [Apache License, Version 2.0](LICENSE-APACHE).

Based on [Alacritty](https://github.com/alacritty/alacritty) by Christian Duerr,
Joe Wilm and contributors.
