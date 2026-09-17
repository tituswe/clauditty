CLAUDITTY-MSG(1)

# NAME

clauditty-msg - Send messages to Clauditty.

# SYNOPSIS

This command communicates with running Clauditty instances through a socket,
making it possible to control Clauditty without directly accessing it.

# OPTIONS

*-s, --socket* _<SOCKET>_

	Path for IPC socket communication.

# MESSAGES

*create-window*

	Create a new window in the same Clauditty process.

	*FLAGS*
		*--hold*

			Remain open after child process exits.

	*OPTIONS*
		*--working-directory* _<WORKING_DIRECTORY>_

			Start the shell in the specified working directory.

		*-T, --title* _<TITLE>_

			Defines the window title.

			Default: _Clauditty_

		*--class* _<GENERAL>_ | _<GENERAL>_,_<INSTANCE>_

			Defines window class/app_id on X11/Wayland.

			Default: _Clauditty,Clauditty_

		*-o, --option* _<OPTION>..._

			Override configuration file options.

			Example: _clauditty msg create-window -o 'cursor.style="Beam"'_

		*-e, --command* _<COMMAND>..._

			Command and args to execute (must be last argument).

*config*

	Update the Clauditty configuration.

	*ARGS*
		*<CONFIG_OPTIONS>...*

			Configuration file options.

			Example: _clauditty msg config 'cursor.style="Beam"'_

	*FLAGS*
		*-r, --reset*

			Clear all runtime configuration changes.

	*OPTIONS*
		*-w, --window-id* _<WINDOW_ID>_

			Window ID for the new config.

			Use _-1_ to apply this change to all windows.

			Default: _$CLAUDITTY_WINDOW_ID_

*get-config*

	Read runtime Clauditty configuration.

	*OPTIONS*
		*-w, --window-id* _<WINDOW_ID>_

			Window ID for the config request.

			Use _-1_ to get the global config.

			Default: _$CLAUDITTY_WINDOW_ID_

# SEE ALSO

*clauditty*(1), *clauditty*(5), *clauditty-bindings*(5), *clauditty-escapes*(7)

# BUGS

Found a bug? Please report it at _https://github.com/tituswe/clauditty/issues_.

# MAINTAINERS

- Christian Duerr <contact@christianduerr.com>
- Kirill Chibisov <contact@kchibisov.com>
