//! Terminal window context.

use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fs::File;
use std::io::Write;
use std::mem;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Instant;

use glutin::config::Config as GlutinConfig;
use glutin::display::GetGlDisplay;
#[cfg(all(feature = "x11", not(any(target_os = "macos", windows))))]
use glutin::platform::x11::X11GlConfigExt;
use log::{error, info};
use serde_json as json;
use winit::dpi::PhysicalPosition;
use winit::event::{ElementState, Event as WinitEvent, Ime, Modifiers, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoopProxy};
use winit::keyboard::{Key, NamedKey};
use winit::raw_window_handle::HasDisplayHandle;
use winit::window::{CursorIcon, WindowId};

use clauditty_terminal::event::{Event as TerminalEvent, OnResize};
use clauditty_terminal::grid::Dimensions;
use clauditty_terminal::term::test::TermSize;
use clauditty_terminal::tty;

use crate::activity::{self, Activity};
use crate::cli::{ParsedOptions, WindowOptions};
use crate::clipboard::Clipboard;
use crate::config::UiConfig;
use crate::config::window::Decorations;
#[cfg(not(any(windows, target_os = "openbsd")))]
use crate::daemon::foreground_process_path;
use crate::display::sidebar::{self, PaneBorder, SidebarTab, TabGroup};
use crate::display::window::Window;
use crate::display::{Display, SizeInfo};
use crate::event::{ActionContext, Event, EventType, Mouse, TouchPurpose};
use crate::layout::{self, Divider, FocusDirection, Layout, PaneId, Rect, SplitDirection};
#[cfg(unix)]
use crate::logging::LOG_TARGET_IPC_CONFIG;
use crate::message_bar::MessageBuffer;
use crate::pane::Pane;
use crate::scheduler::Scheduler;
use crate::session::{self, PaneSession, Session, TabSession};
use crate::{input, renderer};

/// Change to the tabs or panes of a window, requested by an action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneCommand {
    /// New tab running `tabs.command`, like Claude Code.
    NewAgentTab,
    /// New tab running a plain shell.
    NewTab,
    Split(SplitDirection),
    ClosePane,
    CloseWindow,
    Focus(FocusDirection),
    Resize(FocusDirection),
    DismissTab,
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

/// Share of a split moved by one resize step.
const RESIZE_STEP: f32 = 0.03;

/// Distance from a divider where it can be grabbed, in points.
const DIVIDER_GRAB_WIDTH: f32 = 6.;

/// Corner radius of the focused pane's border, in points.
const PANE_CORNER_RADIUS: f32 = 6.;

/// Height of the macOS title bar in points.
const TITLE_BAR_HEIGHT: f32 = 28.;

/// Spinner frames for working tabs.
const SPINNER: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

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
    /// Divider being dragged to resize panes.
    dragged_divider: Option<Divider>,
    window_focused: bool,
    should_close: bool,
    /// Whether the session on disk is out of date.
    session_dirty: bool,
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
        session: Option<Session>,
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

        Self::new(display, config, options, proxy, session)
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

        let mut window_context = Self::new(display, config, options, proxy, None)?;

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
        session: Option<Session>,
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
            dragged_divider: Default::default(),
            window_focused: Default::default(),
            should_close: Default::default(),
            session_dirty: Default::default(),
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

        // Reopen the tabs of the last run, or start a fresh one.
        match session.filter(|session| !session.tabs.is_empty()) {
            Some(session) => window_context.restore(session),
            None => {
                // New windows start with a terminal, like any other terminal emulator.
                let mut pty_config = window_context.config.pty_config();
                options.terminal_options.override_pty_config(&mut pty_config);

                window_context.open_tab(&pty_config)?;
            },
        }

        if window_context.tabs.is_empty() {
            return Err("no tabs could be opened".into());
        }

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
        let mut dividers: Vec<_> = tab
            .layout
            .dividers(self.terminal_area(), gap)
            .into_iter()
            .map(|divider| divider.rect)
            .collect();
        let top = self.title_bar_height();
        let sidebar_width = sidebar::sidebar_width(&window_size);
        dividers.push(Rect::new(sidebar_width, top, gap, window_size.height() - top));

        // Outline the focused pane in the color of its harness.
        let scale_factor = self.display.window.scale_factor as f32;
        let border = self.panes.get_mut(&tab.focused).map(|pane| PaneBorder {
            rect: pane.rect,
            width: pane_border_width(scale_factor),
            color: pane.harness().accent_color(),
            radius: (PANE_CORNER_RADIUS * scale_factor).round(),
        });

        let sidebar_tabs = self.sidebar_tabs();
        let opacity = self.config.window_opacity();
        self.display.draw_sidebar(&sidebar_tabs, &dividers, border, top, opacity);

        let title = self.config.window.identity.title.clone();
        self.display.draw_title_bar(&title, top, scale_factor);

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
                let (x, y) = (position.x as f32, position.y as f32);

                // Dragging a divider resizes the panes around it.
                if let Some(divider) = self.dragged_divider.clone() {
                    let ratio = divider.ratio_at((x, y), self.divider_width());
                    if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                        tab.layout.set_ratio(&divider.path, ratio);
                    }
                    self.session_dirty = true;
                    self.update_layout();
                    return Vec::new();
                }

                // Show that dividers can be dragged.
                if let Some(divider) = self.divider_at(x, y) {
                    let cursor = match divider.direction {
                        SplitDirection::Right => CursorIcon::ColResize,
                        SplitDirection::Down => CursorIcon::RowResize,
                    };
                    self.display.window.set_mouse_cursor(cursor);
                    return Vec::new();
                }

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
                let top = self.title_bar_height();
                if y < top {
                    return Vec::new();
                }
                if x < sidebar::sidebar_width(&window_size) {
                    let order = self.tab_order();
                    let groups: Vec<_> =
                        order.iter().map(|index| tab_group(self.tab_activity(*index))).collect();
                    if let Some(position) = sidebar::tab_at(&window_size, &groups, x, y - top) {
                        self.select_tab(TabSelection::Index(position));
                    }
                    return Vec::new();
                }

                // Grabbing a divider starts a resize.
                if let Some(divider) = self.divider_at(x, y) {
                    self.dragged_divider = Some(divider);
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
            WindowEvent::MouseInput { state: ElementState::Released, .. }
                if self.dragged_divider.is_some() =>
            {
                self.dragged_divider = None;
                Vec::new()
            },
            WindowEvent::Focused(is_focused) => {
                self.window_focused = is_focused;
                vec![(focused, WinitEvent::WindowEvent { window_id, event: window_event })]
            },
            WindowEvent::KeyboardInput { event: ref key, .. } => {
                if key.state == ElementState::Pressed {
                    self.on_pane_input(focused);

                    // Replying to a ready tab marks it as read.
                    let is_enter = key.logical_key == Key::Named(NamedKey::Enter);
                    if is_enter && !self.modifiers.state().super_key() {
                        self.mark_tab_read(self.active_tab);
                    }
                }
                vec![(focused, WinitEvent::WindowEvent { window_id, event: window_event })]
            },
            WindowEvent::Ime(Ime::Commit(_)) => {
                self.on_pane_input(focused);
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
        let event = WindowEvent::CursorMoved { device_id, position };
        WinitEvent::WindowEvent { window_id, event }
    }

    /// Apply tab and pane changes requested by actions.
    fn apply_pane_commands(&mut self) {
        for command in mem::take(&mut self.pane_commands) {
            let result = match command {
                PaneCommand::NewAgentTab => {
                    let pty_config = self.tab_pty_config(self.focused_working_directory());
                    self.open_tab(&pty_config)
                },
                PaneCommand::NewTab => {
                    let mut pty_config = self.config.pty_config();
                    if let Some(working_directory) = self.focused_working_directory() {
                        pty_config.working_directory = Some(working_directory);
                    }
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
                    // Quitting keeps the tabs for the next run.
                    session::save(&self.session());
                    self.should_close = true;
                    Ok(())
                },
                PaneCommand::Focus(direction) => {
                    self.move_focus(direction);
                    Ok(())
                },
                PaneCommand::Resize(direction) => {
                    self.resize_focused_pane(direction);
                    Ok(())
                },
                PaneCommand::DismissTab => {
                    self.dismiss_tab();
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

    /// Everything needed to reopen this window's tabs and panes.
    pub fn session(&mut self) -> Session {
        let mut taken = HashSet::new();
        let mut tabs = Vec::with_capacity(self.tabs.len());

        for index in 0..self.tabs.len() {
            let tab = &self.tabs[index];
            let (layout, focused) = (tab.layout.clone(), tab.focused);

            let mut panes = Vec::new();
            for pane_id in layout.panes() {
                let Some(pane) = self.panes.get_mut(&pane_id) else { continue };
                let harness = pane.harness();
                let working_directory = pane.working_directory().map(Path::to_path_buf);
                let command = harness.restore_command(working_directory.as_deref(), &mut taken);
                panes.push((pane_id, PaneSession { command, working_directory }));
            }

            tabs.push(TabSession { layout, focused, panes });
        }

        Session { tabs, active_tab: self.active_tab }
    }

    /// The session to store, if the tabs or panes changed since the last call.
    pub fn take_session_update(&mut self) -> Option<Session> {
        mem::take(&mut self.session_dirty).then(|| self.session())
    }

    /// Reopen the tabs and panes of a saved session.
    fn restore(&mut self, session: Session) {
        for tab in session.tabs {
            let rects = tab.layout.rects(self.terminal_area(), self.divider_width());

            let mut panes: HashMap<PaneId, Pane> = HashMap::default();
            for (pane_id, pane) in &tab.panes {
                let Some((_, rect)) = rects.iter().find(|(id, _)| id == pane_id) else { continue };
                let pty_config =
                    self.pty_config(pane.command.as_deref(), pane.working_directory.clone());

                match self.spawn_pane(*pane_id, &pty_config, *rect) {
                    Ok(pane) => {
                        panes.insert(*pane_id, pane);
                    },
                    Err(err) => error!("Could not restore pane: {err}"),
                }
            }

            // Drop panes whose shell could not start.
            let mut layout = Some(tab.layout.clone());
            for pane_id in tab.layout.panes().into_iter().filter(|id| !panes.contains_key(id)) {
                layout = layout.and_then(|layout| layout.remove(pane_id));
            }
            let Some(layout) = layout else { continue };

            let focused = if panes.contains_key(&tab.focused) {
                tab.focused
            } else {
                layout.panes()[0]
            };

            self.next_pane_id = self.next_pane_id.max(layout.panes().iter().map(|id| id.0).max().unwrap_or(0));
            self.panes.extend(panes);
            self.tabs.push(Tab { layout, focused });
        }

        self.active_tab = session.active_tab.min(self.tabs.len().saturating_sub(1));
        self.on_layout_change();
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
            // Closing every tab means the user wants none of them back.
            session::save(&Session::default());
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

        // Tabs are numbered in sidebar order.
        let order = self.tab_order();
        let position = order.iter().position(|index| *index == self.active_tab).unwrap_or(0);
        let position = match selection {
            TabSelection::Next => (position + 1) % count,
            TabSelection::Previous => (position + count - 1) % count,
            TabSelection::Index(index) if index < count => index,
            TabSelection::Index(_) => return,
            TabSelection::Last => count - 1,
        };
        self.active_tab = order[position];

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

        // Enter the tab above or below in the sidebar from the side we came from.
        let order = self.tab_order();
        let position = order.iter().position(|index| *index == self.active_tab).unwrap_or(0);
        let (index, edge) = match direction {
            FocusDirection::Up if position > 0 => (order[position - 1], FocusDirection::Down),
            FocusDirection::Down if position + 1 < order.len() => {
                (order[position + 1], FocusDirection::Up)
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

    /// Mark the active tab as read and go to the first other tab in the sidebar.
    ///
    /// Working tabs keep working, only ready tabs move to idle.
    fn dismiss_tab(&mut self) {
        self.mark_tab_read(self.active_tab);

        let order = self.tab_order();
        if let Some(&next) = order.iter().find(|index| **index != self.active_tab) {
            self.active_tab = next;
        }

        self.on_focus_change();
    }

    /// Mark every ready pane of a tab as read.
    fn mark_tab_read(&mut self, index: usize) {
        let Some(tab) = self.tabs.get(index) else { return };
        for pane_id in tab.layout.panes() {
            if let Some(pane) = self.panes.get_mut(&pane_id) {
                pane.activity.mark_read();
            }
        }
        self.dirty = true;
    }

    /// Record the user typing into a pane.
    fn on_pane_input(&mut self, pane_id: PaneId) {
        if let Some(pane) = self.panes.get_mut(&pane_id) {
            pane.activity.on_input(Instant::now());
        }
    }

    /// Record output printed by a pane.
    pub fn on_pane_output(&mut self, pane_id: PaneId) {
        if let Some(pane) = self.panes.get_mut(&pane_id) {
            pane.activity.on_output(Instant::now());
        }
    }

    /// Check which panes started or finished working.
    pub fn update_activity(&mut self) {
        let now = Instant::now();
        let watching = self.window_focused && !self.occluded;

        let mut changed = false;
        let mut busy = false;
        let mut became_ready = false;
        for (index, tab) in self.tabs.iter().enumerate() {
            // Work finishing in the tab the user is looking at doesn't need their attention.
            let visible = watching && index == self.active_tab;

            for pane_id in tab.layout.panes() {
                let Some(pane) = self.panes.get_mut(&pane_id) else { continue };
                let rule = pane.harness().activity_rule();
                let shell_in_front = pane.process().shell_in_front();

                if pane.activity.update(now, rule, shell_in_front, visible) {
                    changed = true;
                    became_ready |= matches!(pane.activity.activity(), Activity::Ready { .. });
                }
                busy |= pane.activity.activity() != Activity::Idle;
            }
        }

        // Point the user at work waiting for them in another app.
        if became_ready && !self.window_focused && self.config.alerts.bounce {
            self.display.window.request_attention();
        }

        // Redraw for new states, and to keep spinners and timers moving.
        if changed || busy {
            self.dirty = true;
            if self.display.window.has_frame {
                self.display.window.request_redraw();
            }
        }
    }

    /// Number of tabs waiting to be read.
    pub fn ready_tabs(&self) -> usize {
        (0..self.tabs.len())
            .filter(|index| matches!(self.tab_activity(*index), Activity::Ready { .. }))
            .count()
    }

    /// Tab indices in sidebar order: ready tabs oldest first, then working, then idle.
    fn tab_order(&self) -> Vec<usize> {
        let mut order: Vec<_> =
            (0..self.tabs.len()).map(|index| (index, self.tab_activity(index))).collect();
        order.sort_by_key(|(index, activity)| match activity {
            Activity::Ready { since } => (0, Some(*since), *index),
            Activity::Working { .. } => (1, None, *index),
            Activity::Idle => (2, None, *index),
        });
        order.into_iter().map(|(index, _)| index).collect()
    }

    /// Activity of a tab, taking the most urgent of its panes.
    fn tab_activity(&self, index: usize) -> Activity {
        let Some(tab) = self.tabs.get(index) else { return Activity::Idle };

        let mut ready = None;
        let mut working = None;
        for pane in tab.layout.panes().iter().filter_map(|id| self.panes.get(id)) {
            match pane.activity.activity() {
                Activity::Ready { since } => ready = ready.min(Some(since)).or(Some(since)),
                Activity::Working { since } => working = working.min(Some(since)).or(Some(since)),
                Activity::Idle => (),
            }
        }

        match (ready, working) {
            (Some(since), _) => Activity::Ready { since },
            (None, Some(since)) => Activity::Working { since },
            (None, None) => Activity::Idle,
        }
    }

    /// Move the divider next to the focused pane one step.
    fn resize_focused_pane(&mut self, direction: FocusDirection) {
        let Some(focused) = self.focused_pane_id() else { return };
        let split_direction = match direction {
            FocusDirection::Left | FocusDirection::Right => SplitDirection::Right,
            FocusDirection::Up | FocusDirection::Down => SplitDirection::Down,
        };

        let Some(tab) = self.tabs.get_mut(self.active_tab) else { return };
        let Some((path, _)) = tab.layout.split_containing(focused, split_direction) else { return };
        let Some(ratio) = tab.layout.ratio_mut(&path).copied() else { return };

        let step = match direction {
            FocusDirection::Right | FocusDirection::Down => RESIZE_STEP,
            FocusDirection::Left | FocusDirection::Up => -RESIZE_STEP,
        };
        tab.layout.set_ratio(&path, ratio + step);

        self.session_dirty = true;
        self.update_layout();
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
        self.session_dirty = true;
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
        self.pty_config(Some(&self.config.tabs.command), working_directory)
    }

    /// PTY options running `command`, falling back to a shell once it exits.
    fn pty_config(&self, command: Option<&str>, working_directory: Option<PathBuf>) -> tty::Options {
        let mut pty_config = self.config.pty_config();
        if working_directory.is_some() {
            pty_config.working_directory = working_directory;
        }

        let command = command.unwrap_or_default().trim();

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

    /// Divider of the active tab at a point in window pixels.
    fn divider_at(&self, x: f32, y: f32) -> Option<Divider> {
        let gap = self.divider_width();
        let grab = (DIVIDER_GRAB_WIDTH * self.display.window.scale_factor as f32).round();

        let tab = self.tabs.get(self.active_tab)?;
        tab.layout
            .dividers(self.terminal_area(), gap)
            .into_iter()
            .find(|divider| divider.contains(x, y, grab))
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
        let y = self.title_bar_height();
        Rect::new(x, y, (window_size.width() - x).max(0.), (window_size.height() - y).max(0.))
    }

    /// Height of the title bar drawn over the window's content.
    ///
    /// With transparent decorations on macOS, the title bar shows the terminal background.
    fn title_bar_height(&self) -> f32 {
        let transparent = matches!(
            self.config.window.decorations,
            Decorations::Transparent | Decorations::Buttonless
        );

        if cfg!(target_os = "macos") && transparent {
            (TITLE_BAR_HEIGHT * self.display.window.scale_factor as f32).round()
        } else {
            0.
        }
    }

    /// Width of the lines between panes.
    fn divider_width(&self) -> f32 {
        self.display.window.scale_factor.round().max(1.) as f32
    }

    /// Tabs in sidebar order, previewing each tab's focused pane.
    fn sidebar_tabs(&mut self) -> Vec<SidebarTab> {
        let home = home::home_dir();
        let now = Instant::now();

        let order = self.tab_order();
        let mut sidebar_tabs = Vec::with_capacity(order.len());
        for index in order {
            let activity = self.tab_activity(index);
            let tab = &self.tabs[index];
            let pane_count = tab.layout.panes().len();

            let Some(pane) = self.panes.get_mut(&tab.focused) else { continue };
            let harness = pane.harness();
            let title = harness.title(&pane.process());
            let preview = pane.preview(harness, sidebar::PREVIEW_LINES);
            let working_directory = pane
                .working_directory()
                .map(|path| sidebar::display_path(path, home.as_deref()))
                .unwrap_or_default();

            let status = match activity {
                Activity::Ready { since } => {
                    format!("{} ago", activity::format_duration(now.duration_since(since)))
                },
                Activity::Working { since } => {
                    let elapsed = now.duration_since(since);
                    let frame = SPINNER[(elapsed.as_millis() / 500) as usize % SPINNER.len()];
                    format!("{frame} {}", activity::format_duration(elapsed))
                },
                Activity::Idle if pane_count > 1 => format!("{pane_count} panes"),
                Activity::Idle => String::new(),
            };

            sidebar_tabs.push(SidebarTab {
                group: tab_group(activity),
                title,
                icon: harness.icon(),
                accent: harness.accent_color(),
                preview,
                working_directory,
                status,
                active: index == self.active_tab,
            });
        }
        sidebar_tabs
    }
}

/// Sidebar group for a tab's activity.
fn tab_group(activity: Activity) -> TabGroup {
    match activity {
        Activity::Ready { .. } => TabGroup::Ready,
        Activity::Working { .. } => TabGroup::Working,
        Activity::Idle => TabGroup::Idle,
    }
}

/// Terminal size for a pane covering `rect`.
fn pane_size_info(
    window_size: &SizeInfo,
    config: &UiConfig,
    scale_factor: f32,
    rect: Rect,
) -> SizeInfo {
    // Keep text clear of the focus border.
    let inset = pane_border_width(scale_factor) + (3. * scale_factor).round();

    let padding = config.window.padding(scale_factor);
    SizeInfo::new(
        rect.width,
        rect.height,
        window_size.cell_width(),
        window_size.cell_height(),
        padding.0 + inset,
        padding.1 + inset,
        config.window.dynamic_padding,
    )
}

/// Width of the border around the focused pane.
fn pane_border_width(scale_factor: f32) -> f32 {
    (1.5 * scale_factor).round().max(1.)
}
