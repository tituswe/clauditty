//! Terminal window context.

use std::collections::HashMap;
use std::error::Error;
use std::fs::File;
use std::io::Write;
use std::mem;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Instant;

use glutin::config::Config as GlutinConfig;
use glutin::display::GetGlDisplay;
#[cfg(all(feature = "x11", not(any(target_os = "macos", windows))))]
use glutin::platform::x11::X11GlConfigExt;
use log::{error, info};
use serde_json as json;
use winit::dpi::PhysicalPosition;
use winit::event::{ElementState, Event as WinitEvent, Modifiers, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoopProxy};
use winit::raw_window_handle::HasDisplayHandle;
use winit::window::{CursorIcon, WindowId};

use clauditty_terminal::event::{Event as TerminalEvent, OnResize};
use clauditty_terminal::grid::Dimensions;
use clauditty_terminal::term::test::TermSize;
use clauditty_terminal::tty;

use crate::cli::{ParsedOptions, WindowOptions};
use crate::clipboard::Clipboard;
use crate::config::UiConfig;
#[cfg(not(any(windows, target_os = "openbsd")))]
use crate::daemon::foreground_process_path;
use crate::display::sidebar::{self, SidebarTab};
use crate::display::window::Window;
use crate::display::{Display, SizeInfo};
use crate::event::{ActionContext, Event, EventType, Mouse, TouchPurpose};
use crate::layout::{self, FocusDirection, Layout, PaneId, Rect, SplitDirection};
#[cfg(unix)]
use crate::logging::LOG_TARGET_IPC_CONFIG;
use crate::message_bar::MessageBuffer;
use crate::pane::{Pane, PaneApp};
use crate::scheduler::Scheduler;
use crate::{input, renderer};

/// Change to the tabs or panes of a window, requested by an action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneCommand {
    NewTab,
    Split(SplitDirection),
    ClosePane,
    CloseWindow,
    Focus(FocusDirection),
    SelectTab(TabSelection),
}

/// Which tab to switch to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabSelection {
    Next,
    Previous,
    Index(usize),
    Last,
}

/// A tab in the sidebar, holding a split layout of panes.
struct Tab {
    layout: Layout,
    focused: PaneId,
}

/// Event context for one individual Clauditty window.
pub struct WindowContext {
    pub message_buffer: MessageBuffer,
    pub display: Display,
    pub dirty: bool,
    event_queue: Vec<WinitEvent<Event>>,
    panes: HashMap<PaneId, Pane>,
    tabs: Vec<Tab>,
    active_tab: usize,
    next_pane_id: usize,
    pane_commands: Vec<PaneCommand>,
    proxy: EventLoopProxy<Event>,
    /// Last mouse position in window pixels.
    cursor_position: PhysicalPosition<f64>,
    /// Whether the mouse is over the focused pane.
    cursor_in_focused_pane: bool,
    window_focused: bool,
    should_close: bool,
    cursor_blink_timed_out: bool,
    prev_bell_cmd: Option<Instant>,
    modifiers: Modifiers,
    mouse: Mouse,
    touch: TouchPurpose,
    occluded: bool,
    preserve_title: bool,
    window_config: ParsedOptions,
    config: Rc<UiConfig>,
}

impl WindowContext {
    /// Create initial window context that does bootstrapping the graphics API we're going to use.
    pub fn initial(
        event_loop: &ActiveEventLoop,
        proxy: EventLoopProxy<Event>,
        config: Rc<UiConfig>,
        mut options: WindowOptions,
    ) -> Result<Self, Box<dyn Error>> {
        let raw_display_handle = event_loop.display_handle().unwrap().as_raw();

        let mut identity = config.window.identity.clone();
        options.window_identity.override_identity_config(&mut identity);

        // Windows has different order of GL platform initialization compared to any other platform;
        // it requires the window first.
        #[cfg(windows)]
        let window = Window::new(event_loop, &config, &identity, &mut options)?;
        #[cfg(windows)]
        let raw_window_handle = Some(window.raw_window_handle());

        #[cfg(not(windows))]
        let raw_window_handle = None;

        let gl_display = renderer::platform::create_gl_display(
            raw_display_handle,
            raw_window_handle,
            config.debug.prefer_egl,
        )?;
        let gl_config = renderer::platform::pick_gl_config(&gl_display, raw_window_handle)?;

        #[cfg(not(windows))]
        let window = Window::new(
            event_loop,
            &config,
            &identity,
            &mut options,
            #[cfg(all(feature = "x11", not(any(target_os = "macos", windows))))]
            gl_config.x11_visual(),
        )?;

        // Create context.
        let gl_context =
            renderer::platform::create_gl_context(&gl_display, &gl_config, raw_window_handle)?;

        let display = Display::new(window, gl_context, &config, false)?;

        Self::new(display, config, options, proxy)
    }

    /// Create additional context with the graphics platform other windows are using.
    pub fn additional(
        gl_config: &GlutinConfig,
        event_loop: &ActiveEventLoop,
        proxy: EventLoopProxy<Event>,
        config: Rc<UiConfig>,
        mut options: WindowOptions,
        config_overrides: ParsedOptions,
    ) -> Result<Self, Box<dyn Error>> {
        let gl_display = gl_config.display();

        let mut identity = config.window.identity.clone();
        options.window_identity.override_identity_config(&mut identity);

        // Check if new window will be opened as a tab.
        // This must be done before `Window::new()`, which unsets `window_tabbing_id`.
        #[cfg(target_os = "macos")]
        let tabbed = options.window_tabbing_id.is_some();
        #[cfg(not(target_os = "macos"))]
        let tabbed = false;

        let window = Window::new(
            event_loop,
            &config,
            &identity,
            &mut options,
            #[cfg(all(feature = "x11", not(any(target_os = "macos", windows))))]
            gl_config.x11_visual(),
        )?;

        // Create context.
        let raw_window_handle = window.raw_window_handle();
        let gl_context =
            renderer::platform::create_gl_context(&gl_display, gl_config, Some(raw_window_handle))?;

        let display = Display::new(window, gl_context, &config, tabbed)?;

        let mut window_context = Self::new(display, config, options, proxy)?;

        // Set the config overrides at startup.
        //
        // These are already applied to `config`, so no update is necessary.
        window_context.window_config = config_overrides;

        Ok(window_context)
    }

    /// Create a new terminal window context.
    fn new(
        display: Display,
        config: Rc<UiConfig>,
        options: WindowOptions,
        proxy: EventLoopProxy<Event>,
    ) -> Result<Self, Box<dyn Error>> {
        let preserve_title = options.window_identity.title.is_some();

        let mut window_context = WindowContext {
            preserve_title,
            display,
            config,
            proxy,
            panes: Default::default(),
            tabs: Default::default(),
            active_tab: Default::default(),
            next_pane_id: Default::default(),
            pane_commands: Default::default(),
            cursor_position: Default::default(),
            cursor_in_focused_pane: Default::default(),
            window_focused: Default::default(),
            should_close: Default::default(),
            cursor_blink_timed_out: Default::default(),
            prev_bell_cmd: Default::default(),
            message_buffer: Default::default(),
            window_config: Default::default(),
            event_queue: Default::default(),
            modifiers: Default::default(),
            occluded: Default::default(),
            mouse: Default::default(),
            touch: Default::default(),
            dirty: Default::default(),
        };

        // A command passed on the CLI replaces the tab command in the first tab.
        let mut pty_config = match options.terminal_options.command() {
            Some(_) => window_context.config.pty_config(),
            None => window_context.tab_pty_config(None),
        };
        options.terminal_options.override_pty_config(&mut pty_config);

        window_context.open_tab(&pty_config)?;

        info!(
            "PTY dimensions: {:?} x {:?}",
            window_context.display.size_info.screen_lines(),
            window_context.display.size_info.columns()
        );

        Ok(window_context)
    }

    /// Update the terminal window to the latest config.
    pub fn update_config(&mut self, new_config: Rc<UiConfig>) {
        let old_config = mem::replace(&mut self.config, new_config);

        // Apply ipc config if there are overrides.
        self.config = self.window_config.override_config_rc(self.config.clone());

        self.display.update_config(&self.config);
        for pane in self.panes.values() {
            pane.terminal.lock().set_options(self.config.term_options());
        }

        // Reload cursor if its thickness has changed.
        if (old_config.cursor.thickness() - self.config.cursor.thickness()).abs() > f32::EPSILON {
            self.display.pending_update.set_cursor_dirty();
        }

        if old_config.font != self.config.font {
            let scale_factor = self.display.window.scale_factor as f32;
            // Do not update font size if it has been changed at runtime.
            if self.display.font_size == old_config.font.size().scale(scale_factor) {
                self.display.font_size = self.config.font.size().scale(scale_factor);
            }

            let font = self.config.font.clone().with_size(self.display.font_size);
            self.display.pending_update.set_font(font);
        }

        // Always reload the theme to account for auto-theme switching.
        self.display.window.set_theme(self.config.window.theme());

        // Update display if either padding options or resize increments were changed.
        let window_config = &old_config.window;
        if window_config.padding(1.) != self.config.window.padding(1.)
            || window_config.dynamic_padding != self.config.window.dynamic_padding
            || window_config.resize_increments != self.config.window.resize_increments
        {
            self.display.pending_update.dirty = true;
        }

        // Update title on config reload according to the following table.
        //
        // │cli │ dynamic_title │ current_title == old_config ││ set_title │
        // │ Y  │       _       │              _              ││     N     │
        // │ N  │       Y       │              Y              ││     Y     │
        // │ N  │       Y       │              N              ││     N     │
        // │ N  │       N       │              _              ││     Y     │
        if !self.preserve_title
            && (!self.config.window.dynamic_title
                || self.display.window.title() == old_config.window.identity.title)
        {
            self.display.window.set_title(self.config.window.identity.title.clone());
        }

        let opaque = self.config.window_opacity() >= 1.;

        // Disable shadows for transparent windows on macOS.
        #[cfg(target_os = "macos")]
        self.display.window.set_has_shadow(opaque);

        #[cfg(target_os = "macos")]
        self.display.window.set_option_as_alt(self.config.window.option_as_alt());

        // Change opacity and blur state.
        self.display.window.set_transparent(!opaque);
        self.display.window.set_blur(self.config.window.blur);

        // Update hint keys.
        self.display.hint_state.update_alphabet(self.config.hints.alphabet());

        // Update cursor blinking.
        let event = Event::new(TerminalEvent::CursorBlinkingChange.into(), None);
        self.event_queue.push(event.into());

        self.dirty = true;
    }

    /// Get reference to the window's configuration.
    #[cfg(unix)]
    pub fn config(&self) -> &UiConfig {
        &self.config
    }

    /// Clear the window config overrides.
    #[cfg(unix)]
    pub fn reset_window_config(&mut self, config: Rc<UiConfig>) {
        // Clear previous window errors.
        self.message_buffer.remove_target(LOG_TARGET_IPC_CONFIG);

        self.window_config.clear();

        // Reload current config to pull new IPC config.
        self.update_config(config);
    }

    /// Add new window config overrides.
    #[cfg(unix)]
    pub fn add_window_config(&mut self, config: Rc<UiConfig>, options: &ParsedOptions) {
        // Clear previous window errors.
        self.message_buffer.remove_target(LOG_TARGET_IPC_CONFIG);

        self.window_config.extend_from_slice(options);

        // Reload current config to pull new IPC config.
        self.update_config(config);
    }

    /// Whether the window has no panes left and should be closed.
    pub fn should_close(&self) -> bool {
        self.should_close
    }

    /// Draw the window.
    pub fn draw(&mut self, scheduler: &mut Scheduler) {
        self.display.window.requested_redraw = false;

        if self.occluded {
            return;
        }

        self.dirty = false;

        // Force the display to process any pending display update.
        self.display.process_renderer_update();

        // Request immediate re-draw if visual bell animation is not finished yet.
        if !self.display.visual_bell.completed() {
            // We can get an OS redraw which bypasses clauditty's frame throttling, thus
            // marking the window as dirty when we don't have frame yet.
            if self.display.window.has_frame {
                self.display.window.request_redraw();
            } else {
                self.dirty = true;
            }
        }

        let Some(tab) = self.tabs.get(self.active_tab) else { return };

        self.display.begin_frame(&self.config);

        // Draw every pane of the active tab.
        for pane_id in tab.layout.panes() {
            let Some(pane) = self.panes.get_mut(&pane_id) else { continue };
            let terminal = pane.terminal.lock();
            self.display.draw_pane(
                terminal,
                pane.size_info,
                pane.rect,
                &self.message_buffer,
                &self.config,
                &mut pane.search_state,
                pane_id == tab.focused,
            );
        }

        // Draw the sidebar, its border and the dividers between panes.
        let window_size = self.display.window_size_info;
        let gap = self.divider_width();
        let mut dividers = tab.layout.dividers(self.terminal_area(), gap);
        dividers.push(Rect::new(sidebar::sidebar_width(&window_size), 0., gap, window_size.height()));

        let sidebar_tabs = self.sidebar_tabs();
        self.display.draw_sidebar(&sidebar_tabs, &dividers);

        self.display.end_frame(scheduler);
    }

    /// Process events for this terminal window.
    pub fn handle_event(
        &mut self,
        #[cfg(target_os = "macos")] event_loop: &ActiveEventLoop,
        event_proxy: &EventLoopProxy<Event>,
        clipboard: &mut Clipboard,
        scheduler: &mut Scheduler,
        event: WinitEvent<Event>,
    ) {
        let is_redraw =
            matches!(event, WinitEvent::WindowEvent { event: WindowEvent::RedrawRequested, .. });

        match event {
            WinitEvent::AboutToWait
            | WinitEvent::WindowEvent { event: WindowEvent::RedrawRequested, .. } => {
                // Skip further event handling with no staged updates.
                if self.event_queue.is_empty() {
                    return;
                }

                // Continue to process all pending events.
            },
            event => {
                self.event_queue.push(event);
                return;
            },
        }

        for event in mem::take(&mut self.event_queue) {
            for (pane_id, event) in self.route_event(event) {
                let Some(pane) = self.panes.get_mut(&pane_id) else { continue };

                self.display.size_info = pane.size_info;
                let mut terminal = pane.terminal.lock();

                let context = ActionContext {
                    cursor_blink_timed_out: &mut self.cursor_blink_timed_out,
                    prev_bell_cmd: &mut self.prev_bell_cmd,
                    message_buffer: &mut self.message_buffer,
                    inline_search_state: &mut pane.inline_search_state,
                    search_state: &mut pane.search_state,
                    modifiers: &mut self.modifiers,
                    notifier: &mut pane.notifier,
                    display: &mut self.display,
                    mouse: &mut self.mouse,
                    touch: &mut self.touch,
                    dirty: &mut self.dirty,
                    occluded: &mut self.occluded,
                    terminal: &mut terminal,
                    #[cfg(not(windows))]
                    master_fd: pane.master_fd,
                    #[cfg(not(windows))]
                    shell_pid: pane.shell_pid,
                    preserve_title: self.preserve_title,
                    config: &self.config,
                    pane_commands: &mut self.pane_commands,
                    event_proxy,
                    #[cfg(target_os = "macos")]
                    event_loop,
                    clipboard,
                    scheduler,
                };
                input::Processor::new(context).handle_event(event);
            }
        }

        self.apply_pane_commands();

        // Process DisplayUpdate events.
        if self.display.pending_update.dirty {
            self.update_layout();
        }

        if self.dirty || self.mouse.hint_highlight_dirty {
            if let Some(pane) = self.focused_pane_id().and_then(|id| self.panes.get(&id)) {
                self.display.size_info = pane.size_info;
                let terminal = pane.terminal.lock();
                self.dirty |= self.display.update_highlighted_hints(
                    &terminal,
                    &self.config,
                    &self.mouse,
                    self.modifiers.state(),
                );
            }
            self.mouse.hint_highlight_dirty = false;
        }

        // Don't call `request_redraw` when event is `RedrawRequested` since the `dirty` flag
        // represents the current frame, but redraw is for the next frame.
        if self.dirty && self.display.window.has_frame && !self.occluded && !is_redraw {
            self.display.window.request_redraw();
        }
    }

    /// Handle the shell of a pane exiting.
    pub fn on_pane_exit(&mut self, pane_id: PaneId) {
        // Keep the pane open if the user asked to hold it.
        if self.display.window.hold {
            return;
        }

        self.remove_pane(pane_id);
        self.update_layout();

        if self.display.window.has_frame {
            self.display.window.request_redraw();
        }
    }

    /// ID of this terminal context.
    pub fn id(&self) -> WindowId {
        self.display.window.id()
    }

    /// Write the ref test results to the disk.
    pub fn write_ref_test_results(&self) {
        let Some(pane) = self.focused_pane_id().and_then(|id| self.panes.get(&id)) else { return };

        // Dump grid state.
        let mut grid = pane.terminal.lock().grid().clone();
        grid.initialize_all();
        grid.truncate();

        let serialized_grid = json::to_string(&grid).expect("serialize grid");

        let size_info = &pane.size_info;
        let size = TermSize::new(size_info.columns(), size_info.screen_lines());
        let serialized_size = json::to_string(&size).expect("serialize size");

        let serialized_config = format!("{{\"history_size\":{}}}", grid.history_size());

        File::create("./grid.json")
            .and_then(|mut f| f.write_all(serialized_grid.as_bytes()))
            .expect("write grid.json");

        File::create("./size.json")
            .and_then(|mut f| f.write_all(serialized_size.as_bytes()))
            .expect("write size.json");

        File::create("./config.json")
            .and_then(|mut f| f.write_all(serialized_config.as_bytes()))
            .expect("write config.json");
    }

    /// Find the pane each event belongs to.
    ///
    /// Mouse events are translated into the pane's own coordinates. Events for the window
    /// itself, like clicks in the sidebar, are handled here and not passed on.
    fn route_event(&mut self, event: WinitEvent<Event>) -> Vec<(PaneId, WinitEvent<Event>)> {
        let Some(focused) = self.focused_pane_id() else { return Vec::new() };

        let (window_id, window_event) = match event {
            WinitEvent::UserEvent(user_event) => {
                let pane_id = user_event.pane_id().unwrap_or(focused);
                if !self.panes.contains_key(&pane_id) {
                    return Vec::new();
                }

                // Titles are shown in the sidebar, so keep them per pane.
                match user_event.payload() {
                    EventType::Terminal(TerminalEvent::Title(title)) => {
                        // Programs like Claude Code clear their title on exit.
                        let title = Some(title.clone()).filter(|title| !title.trim().is_empty());
                        self.set_pane_title(pane_id, title);
                        return Vec::new();
                    },
                    EventType::Terminal(TerminalEvent::ResetTitle) => {
                        self.set_pane_title(pane_id, None);
                        return Vec::new();
                    },
                    _ => (),
                }

                return vec![(pane_id, WinitEvent::UserEvent(user_event))];
            },
            WinitEvent::WindowEvent { window_id, event } => (window_id, event),
            event => return vec![(focused, event)],
        };

        let x = self.cursor_position.x as f32;
        let y = self.cursor_position.y as f32;

        match window_event {
            WindowEvent::CursorMoved { device_id, position } => {
                self.cursor_position = position;

                let Some(rect) = self.panes.get(&focused).map(|pane| pane.rect) else {
                    return Vec::new();
                };

                let dragging = self.mouse.left_button_state == ElementState::Pressed
                    || self.mouse.middle_button_state == ElementState::Pressed
                    || self.mouse.right_button_state == ElementState::Pressed;
                let inside = rect.contains(position.x as f32, position.y as f32);

                // Only the focused pane follows the mouse, unless it's dragging a selection.
                if !inside && !dragging {
                    if !mem::replace(&mut self.cursor_in_focused_pane, false) {
                        return Vec::new();
                    }

                    self.display.window.set_mouse_cursor(CursorIcon::Default);
                    let event = WindowEvent::CursorLeft { device_id };
                    return vec![(focused, WinitEvent::WindowEvent { window_id, event })];
                }

                self.cursor_in_focused_pane = inside;
                vec![(focused, Self::cursor_moved(window_id, device_id, position, rect))]
            },
            WindowEvent::MouseInput { device_id, state: ElementState::Pressed, .. } => {
                // Clicks in the sidebar switch tabs.
                let window_size = self.display.window_size_info;
                if x < sidebar::sidebar_width(&window_size) {
                    if let Some(index) = sidebar::tab_at(&window_size, x, y) {
                        self.select_tab(TabSelection::Index(index));
                    }
                    return Vec::new();
                }

                let Some(target) = self.pane_at(x, y) else { return Vec::new() };
                let rect = self.panes[&target].rect;

                // Clicking another pane focuses it first.
                if target != focused {
                    self.focus_pane(target);
                }
                self.cursor_in_focused_pane = true;

                vec![
                    (target, Self::cursor_moved(window_id, device_id, self.cursor_position, rect)),
                    (target, WinitEvent::WindowEvent { window_id, event: window_event }),
                ]
            },
            WindowEvent::MouseWheel { device_id, .. } => {
                // Scroll the pane under the mouse, even when it's not focused.
                let target = self.pane_at(x, y).unwrap_or(focused);
                let rect = self.panes[&target].rect;

                vec![
                    (target, Self::cursor_moved(window_id, device_id, self.cursor_position, rect)),
                    (target, WinitEvent::WindowEvent { window_id, event: window_event }),
                ]
            },
            WindowEvent::Focused(is_focused) => {
                self.window_focused = is_focused;
                vec![(focused, WinitEvent::WindowEvent { window_id, event: window_event })]
            },
            event => vec![(focused, WinitEvent::WindowEvent { window_id, event })],
        }
    }

    /// Cursor move event relative to the top left corner of a pane.
    fn cursor_moved(
        window_id: WindowId,
        device_id: winit::event::DeviceId,
        position: PhysicalPosition<f64>,
        rect: Rect,
    ) -> WinitEvent<Event> {
        let position =
            PhysicalPosition::new(position.x - rect.x as f64, position.y - rect.y as f64);
        WinitEvent::WindowEvent { window_id, event: WindowEvent::CursorMoved { device_id, position } }
    }

    /// Apply tab and pane changes requested by actions.
    fn apply_pane_commands(&mut self) {
        for command in mem::take(&mut self.pane_commands) {
            let result = match command {
                PaneCommand::NewTab => {
                    let pty_config = self.tab_pty_config(self.focused_working_directory());
                    self.open_tab(&pty_config)
                },
                PaneCommand::Split(direction) => self.split_focused_pane(direction),
                PaneCommand::ClosePane => {
                    if let Some(pane_id) = self.focused_pane_id() {
                        self.remove_pane(pane_id);
                    }
                    Ok(())
                },
                PaneCommand::CloseWindow => {
                    self.should_close = true;
                    Ok(())
                },
                PaneCommand::Focus(direction) => {
                    self.move_focus(direction);
                    Ok(())
                },
                PaneCommand::SelectTab(selection) => {
                    self.select_tab(selection);
                    Ok(())
                },
            };

            if let Err(err) = result {
                error!("Could not open pane: {err}");
            }
        }
    }

    /// Open a new tab with a single pane and switch to it.
    fn open_tab(&mut self, pty_config: &tty::Options) -> Result<(), Box<dyn Error>> {
        let pane_id = self.next_pane_id();
        let rect = self.terminal_area();
        let pane = self.spawn_pane(pane_id, pty_config, rect)?;

        self.panes.insert(pane_id, pane);
        self.tabs.push(Tab { layout: Layout::Pane(pane_id), focused: pane_id });
        self.active_tab = self.tabs.len() - 1;

        self.on_layout_change();

        Ok(())
    }

    /// Split the focused pane, starting a shell in the new pane.
    fn split_focused_pane(&mut self, direction: SplitDirection) -> Result<(), Box<dyn Error>> {
        let Some(focused) = self.focused_pane_id() else { return Ok(()) };

        let mut pty_config = self.config.pty_config();
        if let Some(working_directory) = self.focused_working_directory() {
            pty_config.working_directory = Some(working_directory);
        }

        // Place the new pane first, so its shell starts with the right size.
        let pane_id = self.next_pane_id();
        let area = self.terminal_area();
        let gap = self.divider_width();
        let tab = &mut self.tabs[self.active_tab];
        tab.layout.split(focused, pane_id, direction);

        let rect = tab.layout.rects(area, gap).into_iter().find(|(id, _)| *id == pane_id);
        let pane = match rect.map(|(_, rect)| self.spawn_pane(pane_id, &pty_config, rect)) {
            Some(Ok(pane)) => pane,
            result => {
                // Undo the split.
                let tab = &mut self.tabs[self.active_tab];
                let layout = mem::replace(&mut tab.layout, Layout::Pane(focused));
                tab.layout = layout.remove(pane_id).unwrap_or(Layout::Pane(focused));
                return result.map_or(Ok(()), |result| result.map(|_| ()));
            },
        };

        self.panes.insert(pane_id, pane);
        self.tabs[self.active_tab].focused = pane_id;

        self.on_layout_change();

        Ok(())
    }

    /// Close a pane, closing its tab when it was the last pane.
    fn remove_pane(&mut self, pane_id: PaneId) {
        let Some(tab_index) = self.tabs.iter().position(|tab| tab.layout.panes().contains(&pane_id))
        else {
            return;
        };

        // Dropping the pane shuts down its shell.
        self.panes.remove(&pane_id);

        let tab = &mut self.tabs[tab_index];
        let panes = tab.layout.panes();
        let layout = mem::replace(&mut tab.layout, Layout::Pane(pane_id));

        match layout.remove(pane_id) {
            Some(layout) => {
                // Focus the pane before the closed one, or the one after it.
                if tab.focused == pane_id {
                    let index = panes.iter().position(|id| *id == pane_id).unwrap_or(0);
                    let neighbor = if index > 0 { panes[index - 1] } else { panes[1] };
                    tab.focused = neighbor;
                }
                tab.layout = layout;
            },
            None => {
                self.tabs.remove(tab_index);
                if tab_index < self.active_tab || self.active_tab >= self.tabs.len() {
                    self.active_tab = self.active_tab.saturating_sub(1);
                }
            },
        }

        if self.tabs.is_empty() {
            self.should_close = true;
        }

        self.on_layout_change();
    }

    /// Switch to another tab.
    fn select_tab(&mut self, selection: TabSelection) {
        let count = self.tabs.len();
        if count == 0 {
            return;
        }

        self.active_tab = match selection {
            TabSelection::Next => (self.active_tab + 1) % count,
            TabSelection::Previous => (self.active_tab + count - 1) % count,
            TabSelection::Index(index) if index < count => index,
            TabSelection::Index(_) => return,
            TabSelection::Last => count - 1,
        };

        self.on_focus_change();
    }

    /// Focus the pane next to the focused one.
    ///
    /// Moving up from the top pane or down from the bottom pane switches tabs.
    fn move_focus(&mut self, direction: FocusDirection) {
        let Some(focused) = self.focused_pane_id() else { return };
        let area = self.terminal_area();
        let gap = self.divider_width();

        let rects = self.tabs[self.active_tab].layout.rects(area, gap);
        if let Some(pane_id) = layout::neighbor(&rects, focused, direction) {
            self.focus_pane(pane_id);
            return;
        }

        // Enter the new tab from the side we came from.
        let (index, edge) = match direction {
            FocusDirection::Up if self.active_tab > 0 => (self.active_tab - 1, FocusDirection::Down),
            FocusDirection::Down if self.active_tab + 1 < self.tabs.len() => {
                (self.active_tab + 1, FocusDirection::Up)
            },
            _ => return,
        };

        let tab = &mut self.tabs[index];
        if let Some(pane_id) = layout::edge_pane(&tab.layout.rects(area, gap), edge) {
            tab.focused = pane_id;
        }
        self.active_tab = index;

        self.on_focus_change();
    }

    /// Focus a pane in the active tab.
    fn focus_pane(&mut self, pane_id: PaneId) {
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            tab.focused = pane_id;
        }

        self.on_focus_change();
    }

    /// Resize panes and update focus after panes were added or removed.
    fn on_layout_change(&mut self) {
        self.display.pending_update.dirty = true;
        self.on_focus_change();
    }

    /// Mark only the focused pane as focused, and show its title.
    fn on_focus_change(&mut self) {
        let focused = self.focused_pane_id();

        for (pane_id, pane) in &self.panes {
            pane.terminal.lock().is_focused = self.window_focused && Some(*pane_id) == focused;
        }

        if let Some(pane) = focused.and_then(|id| self.panes.get(&id)) {
            self.display.size_info = pane.size_info;
        }

        // Hints belong to the previously focused pane.
        self.display.highlighted_hint = None;
        self.display.vi_highlighted_hint = None;

        self.update_window_title();
        self.dirty = true;
    }

    /// Store the title of a pane, updating the window title if it's focused.
    fn set_pane_title(&mut self, pane_id: PaneId, title: Option<String>) {
        if let Some(pane) = self.panes.get_mut(&pane_id) {
            pane.title = title;
        }

        if self.focused_pane_id() == Some(pane_id) {
            self.update_window_title();
        }

        self.dirty = true;
    }

    fn update_window_title(&mut self) {
        if self.preserve_title || !self.config.window.dynamic_title {
            return;
        }

        let title = self
            .focused_pane_id()
            .and_then(|id| self.panes.get(&id))
            .and_then(|pane| pane.title.clone())
            .unwrap_or_else(|| self.config.window.identity.title.clone());
        self.display.window.set_title(title);
    }

    /// Apply the window size to every pane, resizing terminals whose grid changed.
    fn update_layout(&mut self) {
        self.display.handle_update(&self.config);

        let area = self.terminal_area();
        let gap = self.divider_width();
        let window_size = self.display.window_size_info;
        let scale_factor = self.display.window.scale_factor as f32;

        for (tab_index, tab) in self.tabs.iter().enumerate() {
            for (pane_id, rect) in tab.layout.rects(area, gap) {
                let Some(pane) = self.panes.get_mut(&pane_id) else { continue };
                let focused = tab_index == self.active_tab && pane_id == tab.focused;

                let mut size = pane_size_info(&window_size, &self.config, scale_factor, rect);

                // Make room for the search bar and the message bar.
                let search_lines = usize::from(pane.search_state.history_index.is_some());
                let message_lines = match self.message_buffer.message() {
                    Some(message) if focused => message.text(&size).len(),
                    _ => 0,
                };
                size.reserve_lines(search_lines + message_lines);

                // Resize the terminal when its dimensions have changed.
                if pane.size_info.screen_lines() != size.screen_lines()
                    || pane.size_info.columns() != size.columns()
                {
                    pane.notifier.on_resize(size.into());
                    pane.terminal.lock().resize(size);
                }

                if pane.size_info != size {
                    pane.search_state.clear_focused_match();
                }

                pane.size_info = size;
                pane.rect = rect;
            }
        }

        if let Some(pane) = self.focused_pane_id().and_then(|id| self.panes.get(&id)) {
            self.display.size_info = pane.size_info;
        }

        self.dirty = true;
    }

    /// Start a shell in a new pane covering `rect`.
    fn spawn_pane(
        &self,
        pane_id: PaneId,
        pty_config: &tty::Options,
        rect: Rect,
    ) -> Result<Pane, Box<dyn Error>> {
        let window_size = self.display.window_size_info;
        let scale_factor = self.display.window.scale_factor as f32;
        let size_info = pane_size_info(&window_size, &self.config, scale_factor, rect);

        Pane::new(
            pane_id,
            self.display.window.id(),
            self.proxy.clone(),
            &self.config,
            pty_config,
            size_info,
            rect,
        )
    }

    /// PTY options running the tab command, falling back to a shell once it exits.
    fn tab_pty_config(&self, working_directory: Option<PathBuf>) -> tty::Options {
        let mut pty_config = self.config.pty_config();
        if working_directory.is_some() {
            pty_config.working_directory = working_directory;
        }

        let command = self.config.tabs.command.trim();

        #[cfg(not(windows))]
        if !command.is_empty() {
            // Use an interactive login shell, so the command is found on the user's `PATH`.
            let shell = std::env::var("SHELL").unwrap_or_else(|_| String::from("/bin/sh"));
            let script = format!("{command}; exec {shell} -l");
            let args = vec![String::from("-l"), String::from("-i"), String::from("-c"), script];
            pty_config.shell = Some(tty::Shell::new(shell, args));
        }

        #[cfg(windows)]
        let _ = command;

        pty_config
    }

    /// Working directory of the focused pane's foreground process.
    fn focused_working_directory(&self) -> Option<PathBuf> {
        #[cfg(not(any(windows, target_os = "openbsd")))]
        {
            let pane = self.panes.get(&self.focused_pane_id()?)?;
            foreground_process_path(pane.master_fd, pane.shell_pid).ok()
        }

        #[cfg(any(windows, target_os = "openbsd"))]
        None
    }

    fn focused_pane_id(&self) -> Option<PaneId> {
        self.tabs.get(self.active_tab).map(|tab| tab.focused)
    }

    /// Pane of the active tab at a point in window pixels.
    fn pane_at(&self, x: f32, y: f32) -> Option<PaneId> {
        let tab = self.tabs.get(self.active_tab)?;
        tab.layout
            .panes()
            .into_iter()
            .find(|id| self.panes.get(id).is_some_and(|pane| pane.rect.contains(x, y)))
    }

    fn next_pane_id(&mut self) -> PaneId {
        self.next_pane_id += 1;
        PaneId(self.next_pane_id)
    }

    /// Area right of the sidebar where panes are placed.
    fn terminal_area(&self) -> Rect {
        let window_size = self.display.window_size_info;
        let x = sidebar::sidebar_width(&window_size) + self.divider_width();
        Rect::new(x, 0., (window_size.width() - x).max(0.), window_size.height())
    }

    /// Width of the lines between panes.
    fn divider_width(&self) -> f32 {
        self.display.window.scale_factor.round().max(1.) as f32
    }

    /// Tabs as shown in the sidebar, previewing each tab's focused pane.
    fn sidebar_tabs(&mut self) -> Vec<SidebarTab> {
        let home = home::home_dir();

        let mut sidebar_tabs = Vec::with_capacity(self.tabs.len());
        for (index, tab) in self.tabs.iter().enumerate() {
            let mut pane = self.panes.get_mut(&tab.focused);
            let app = pane.as_deref_mut().map_or(PaneApp::Shell, Pane::app);
            let working_directory = pane
                .as_deref_mut()
                .and_then(Pane::working_directory)
                .map(|path| sidebar::display_path(path, home.as_deref()))
                .unwrap_or_default();
            let preview = pane.map(|pane| pane.preview(sidebar::PREVIEW_LINES)).unwrap_or_default();

            sidebar_tabs.push(SidebarTab {
                title: app.name().to_owned(),
                claude: app == PaneApp::Claude,
                preview,
                working_directory,
                pane_count: tab.layout.panes().len(),
                active: index == self.active_tab,
            });
        }
        sidebar_tabs
    }
}

/// Terminal size for a pane covering `rect`.
fn pane_size_info(
    window_size: &SizeInfo,
    config: &UiConfig,
    scale_factor: f32,
    rect: Rect,
) -> SizeInfo {
    let padding = config.window.padding(scale_factor);
    SizeInfo::new(
        rect.width,
        rect.height,
        window_size.cell_width(),
        window_size.cell_height(),
        padding.0,
        padding.1,
        config.window.dynamic_padding,
    )
}
