<h1 align="center">Clauditty - An AI-native terminal</h1>

## About

Clauditty is a fast, lightweight terminal built for coding agents like
[Claude Code](https://claude.com/claude-code). It is a fork of
[Alacritty](https://github.com/alacritty/alacritty).

It comes with sensible defaults and a flexible [config](#configuration). See the
[features](./docs/features.md) overview for what it can do.

## Tabs and panes

Tabs live in a sidebar on the left. Each new tab runs Claude Code in its main
pane. Split panes run your shell.

Each tab card shows the app in the focused pane, a preview of its last lines
and its working directory.

| Shortcut            | Action                             |
| ------------------- | ---------------------------------- |
| `Cmd+T`             | New tab                            |
| `Cmd+D`             | Split pane to the right            |
| `Cmd+Shift+D`       | Split pane down                    |
| `Cmd+W`             | Close pane, or tab if last pane    |
| `Cmd+Arrow`         | Move to the pane in that direction |
| `Cmd+1` to `Cmd+9`  | Switch tab                         |
| `Cmd+Shift+[` / `]` | Previous / next tab                |

`Cmd+Up` on the top pane goes to the previous tab, and `Cmd+Down` on the bottom
pane goes to the next tab.

Click a tab to switch to it, or a pane to focus it.

Change the tab command with `tabs.command` in the config. An empty command
starts a plain shell.

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
