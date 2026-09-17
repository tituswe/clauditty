TITTY-MSG(1)

# NAME

titty-msg - Send messages to Titty.

# SYNOPSIS

This command communicates with running Titty instances through a socket,
making it possible to control Titty without directly accessing it.

# OPTIONS

*-s, --socket* _<SOCKET>_

	Path for IPC socket communication.

# MESSAGES

*create-window*

	Create a new window in the same Titty process.

	*FLAGS*
		*--hold*

			Remain open after child process exits.

	*OPTIONS*
		*--working-directory* _<WORKING_DIRECTORY>_

			Start the shell in the specified working directory.

		*-T, --title* _<TITLE>_

			Defines the window title.

			Default: _Titty_

		*--class* _<GENERAL>_ | _<GENERAL>_,_<INSTANCE>_

			Defines window class/app_id on X11/Wayland.

			Default: _Titty,Titty_

		*-o, --option* _<OPTION>..._

			Override configuration file options.

			Example: _titty msg create-window -o 'cursor.style="Beam"'_

		*-e, --command* _<COMMAND>..._

			Command and args to execute (must be last argument).

*config*

	Update the Titty configuration.

	*ARGS*
		*<CONFIG_OPTIONS>...*

			Configuration file options.

			Example: _titty msg config 'cursor.style="Beam"'_

	*FLAGS*
		*-r, --reset*

			Clear all runtime configuration changes.

	*OPTIONS*
		*-w, --window-id* _<WINDOW_ID>_

			Window ID for the new config.

			Use _-1_ to apply this change to all windows.

			Default: _$TITTY_WINDOW_ID_

*get-config*

	Read runtime Titty configuration.

	*OPTIONS*
		*-w, --window-id* _<WINDOW_ID>_

			Window ID for the config request.

			Use _-1_ to get the global config.

			Default: _$TITTY_WINDOW_ID_

# SEE ALSO

*titty*(1), *titty*(5), *titty-bindings*(5), *titty-escapes*(7)

# BUGS

Found a bug? Please report it at _https://github.com/tituswe/titty/issues_.

# MAINTAINERS

- Christian Duerr <contact@christianduerr.com>
- Kirill Chibisov <contact@kchibisov.com>
