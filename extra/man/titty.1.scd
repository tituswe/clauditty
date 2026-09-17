TITTY(1)

# NAME

Titty - A fast, cross-platform, OpenGL terminal emulator.

# SYNOPSIS

Titty is a modern terminal emulator that comes with sensible defaults, but
allows for extensive configuration. By integrating with other applications,
rather than reimplementing their functionality, it manages to provide a flexible
set of features with high performance.

# FLAGS

*-h, --help*

	Prints help information.

*--hold*

	Remain open after child process exits.

*--daemon*

	Do not spawn an initial window.

*--print-events*

	Print all events to STDOUT.

*-q*

	Reduces the level of verbosity (the min level is *-qq*).

*--ref-test*

	Generates ref test

*-v*

	Increases the level of verbosity (the max level is *-vvv*).

*-V, --version*

	Prints version information.

# OPTIONS

*--class* _<GENERAL>_ | _<GENERAL>_,_<INSTANCE>_

	Defines the window class hint on Linux.

	When only the general class is passed, instance will be set to the same value.

	On Wayland the general class sets the _app\_id_, while the instance class is ignored.

	Default: _Titty,Titty_

*-e, --command* _<COMMAND>..._

	Command and args to execute (must be last argument).

*--config-file* _<CONFIG_FILE>_

	Specify alternative configuration file.

	Titty doesn't create the config file for you, but it looks for one in the
	following locations on UNIX systems:

		. _$XDG_CONFIG_HOME/titty/titty.toml_
		. _$XDG_CONFIG_HOME/titty.toml_
		. _$HOME/.config/titty/titty.toml_
		. _$HOME/.titty.toml_
		. _/etc/titty/titty.toml_

	On Windows, the config file will be looked for in:

		. _%APPDATA%\\titty\\titty.toml_

*--embed* _<PARENT>_

	X11 window ID to embed Titty within (decimal or hexadecimal with _0x_ prefix).

*-o, --option* _<OPTION>..._

	Override configuration file options.

	Example: _titty -o 'cursor.style="Beam"'_

*--socket* _<SOCKET>_

	Path for IPC socket creation.

*-T, --title* _<TITLE>_

	Defines the window title.

	Default: _Titty_

*--working-directory* _<WORKING_DIRECTORY>_

	Start the shell in the specified working directory.

# SUBCOMMANDS

*msg*

	Send IPC socket messages (see *titty-msg*(1)).

*migrate*

	Migrate the configuration file.

	*-c, --config-file* _<CONFIG_FILE>_

		Path to the configuration file.

	*-d, --dry-run*

		Only output TOML config to STDOUT.

	*-i, --skip-imports*

		Do not recurse over imports.

	*--skip-renames*

		Do not move renamed fields to their new location.

	*-s, --silent*

		Do not output to STDOUT.

	*-h, --help*

		Print help information.

# SEE ALSO

*titty-msg*(1), *titty*(5), *titty-bindings*(5), *titty-escapes*(7)

# BUGS

Found a bug? Please report it at _https://github.com/tituswe/titty/issues_.

# MAINTAINERS

- Christian Duerr <contact@christianduerr.com>
- Kirill Chibisov <contact@kchibisov.com>
