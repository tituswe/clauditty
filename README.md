<h1 align="center">Titty - A fast, cross-platform, OpenGL terminal emulator</h1>

## About

Titty is a fast, lightweight terminal emulator. It is a fork of
[Alacritty](https://github.com/alacritty/alacritty).

It comes with sensible defaults and a flexible [config](#configuration). See the
[features](./docs/features.md) overview for what it can do.

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

To build a macOS app (`target/release/osx/Titty.app`):

```sh
make app
```

More build details are in [INSTALL.md](INSTALL.md).

### Requirements

- At least OpenGL ES 2.0
- [Windows] ConPTY support (Windows 10 version 1809 or higher)

## Configuration

See `man 5 titty` for all config options.

Titty doesn't create the config file for you, but it looks for one in the
following locations:

1. `$XDG_CONFIG_HOME/titty/titty.toml`
2. `$XDG_CONFIG_HOME/titty.toml`
3. `$HOME/.config/titty/titty.toml`
4. `$HOME/.titty.toml`
5. `/etc/titty/titty.toml`

On Windows, the config file will be looked for in:

* `%APPDATA%\titty\titty.toml`

Changes to the config file apply right away, with no restart.

## License

Titty is released under the [Apache License, Version 2.0](LICENSE-APACHE).

Based on [Alacritty](https://github.com/alacritty/alacritty) by Christian Duerr,
Joe Wilm and contributors.
