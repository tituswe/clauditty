//! Sidebar listing the tabs of a window.

use std::path::Path;

use unicode_width::UnicodeWidthChar;

use clauditty_terminal::index::{Column, Point};
use clauditty_terminal::vte::ansi::NamedColor;

use crate::display::color::Rgb;
use crate::display::{Display, SizeInfo};
use crate::layout::Rect;
use crate::renderer::rects::RenderRect;

/// Width of the sidebar in cells.
pub const SIDEBAR_COLUMNS: usize = 32;

/// Lines of screen content previewed on each tab.
pub const PREVIEW_LINES: usize = 2;

/// Empty lines above the first tab.
const TOP_LINES: usize = 1;

/// Lines of text on a tab card: title, preview and working directory.
const CARD_LINES: usize = PREVIEW_LINES + 2;

/// Lines taken by each tab, including the gap below the card.
const LINES_PER_TAB: usize = CARD_LINES + 1;

/// Lines taken by a group header, including the gap below it.
const HEADER_LINES: usize = 2;

/// First column of text on a card, leaving room for the accent bar.
const TEXT_COLUMN: usize = 2;

/// Color marking tabs ready for the user.
const READY_COLOR: Rgb = Rgb::new(0xd9, 0x77, 0x57);

/// Sidebar group of a tab, in the order they are shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabGroup {
    /// Finished while the user was away.
    Ready,
    Working,
    Idle,
}

impl TabGroup {
    const ALL: [TabGroup; 3] = [TabGroup::Ready, TabGroup::Working, TabGroup::Idle];

    fn label(self) -> &'static str {
        match self {
            TabGroup::Ready => "READY",
            TabGroup::Working => "WORKING",
            TabGroup::Idle => "IDLE",
        }
    }
}

/// A tab as shown in the sidebar.
pub struct SidebarTab {
    pub group: TabGroup,
    /// Name of the harness in the tab's focused pane.
    pub title: String,
    /// Icon of the harness.
    pub icon: char,
    /// Color of the harness, used for its icon and the active tab marker.
    pub accent: Rgb,
    /// Last lines of text in the focused pane.
    pub preview: Vec<String>,
    /// Working directory of the focused pane.
    pub working_directory: String,
    /// Short status, like the time since the tab finished.
    pub status: String,
    pub active: bool,
}

/// Border around the focused pane.
pub struct PaneBorder {
    pub rect: Rect,
    pub width: f32,
    pub color: Rgb,
}

/// A row of the sidebar, starting at `line`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SidebarRow {
    Header { group: TabGroup, count: usize, line: usize },
    /// Tab card at `position` in the sidebar order.
    Card { position: usize, line: usize },
}

/// Rows of the sidebar for tabs in sidebar order, grouped by `groups`.
fn sidebar_rows(groups: &[TabGroup]) -> Vec<SidebarRow> {
    let mut rows = Vec::new();
    let mut line = TOP_LINES;
    let mut position = 0;

    for group in TabGroup::ALL {
        let count = groups.iter().filter(|tab_group| **tab_group == group).count();
        if count == 0 {
            continue;
        }

        rows.push(SidebarRow::Header { group, count, line });
        line += HEADER_LINES;

        for _ in 0..count {
            rows.push(SidebarRow::Card { position, line });
            position += 1;
            line += LINES_PER_TAB;
        }
    }

    rows
}

/// Width of the sidebar in pixels.
pub fn sidebar_width(size_info: &SizeInfo) -> f32 {
    (SIDEBAR_COLUMNS as f32 * size_info.cell_width()).floor()
}

/// Sidebar position of the tab card at a point in window pixels.
///
/// The `groups` are those of the tabs in sidebar order.
pub fn tab_at(size_info: &SizeInfo, groups: &[TabGroup], x: f32, y: f32) -> Option<usize> {
    if x < 0. || x >= sidebar_width(size_info) || y < 0. {
        return None;
    }

    let clicked_line = (y / size_info.cell_height()) as usize;
    sidebar_rows(groups).into_iter().find_map(|row| match row {
        SidebarRow::Card { position, line } => {
            (line..line + CARD_LINES).contains(&clicked_line).then_some(position)
        },
        _ => None,
    })
}

/// Shorten a path for display, replacing the home directory with `~`.
pub fn display_path(path: &Path, home: Option<&Path>) -> String {
    match home.and_then(|home| path.strip_prefix(home).ok()) {
        Some(relative) if relative.as_os_str().is_empty() => String::from("~"),
        Some(relative) => format!("~/{}", relative.display()),
        None => path.display().to_string(),
    }
}

impl Display {
    /// Draw the sidebar, the dividers between panes and the focused pane's border.
    ///
    /// The sidebar starts `top` pixels below the top of the window, leaving room for the title bar.
    pub fn draw_sidebar(
        &mut self,
        tabs: &[SidebarTab],
        dividers: &[Rect],
        border: Option<PaneBorder>,
        top: f32,
    ) {
        let window_size = self.window_size_info;
        let metrics = self.glyph_cache.font_metrics();

        let background = self.colors[NamedColor::Background as usize];
        let foreground = self.colors[NamedColor::Foreground as usize];
        let colors = SidebarColors {
            sidebar: mix(background, foreground, 0.03),
            card: mix(background, foreground, 0.07),
            active_card: mix(background, foreground, 0.14),
            divider: mix(background, foreground, 0.2),
            title: mix(background, foreground, 0.85),
            active_title: foreground,
            preview: mix(background, foreground, 0.55),
            detail: mix(background, foreground, 0.4),
        };

        // Dividers and the border are placed in window coordinates.
        let mut window_rects: Vec<_> = dividers
            .iter()
            .map(|divider| {
                let Rect { x, y, width, height } = *divider;
                RenderRect::new(x, y, width, height, colors.divider, 1.)
            })
            .collect();

        if let Some(PaneBorder { rect, width: border, color }) = border {
            let Rect { x, y, width, height } = rect;
            window_rects.extend([
                RenderRect::new(x, y, width, border, color, 1.),
                RenderRect::new(x, y + height - border, width, border, color, 1.),
                RenderRect::new(x, y, border, height, color, 1.),
                RenderRect::new(x + width - border, y, border, height, color, 1.),
            ]);
        }

        self.renderer.set_origin(0, 0);
        self.renderer.resize(&window_size);
        self.renderer.draw_rects(&window_size, &metrics, window_rects);

        // Draw the sidebar below the title bar.
        let size_info = SizeInfo::new(
            window_size.width(),
            (window_size.height() - top).max(1.),
            window_size.cell_width(),
            window_size.cell_height(),
            0.,
            0.,
            false,
        );
        self.renderer.resize(&size_info);

        let width = sidebar_width(&size_info);
        let cell_width = size_info.cell_width();
        let cell_height = size_info.cell_height();
        let margin = (cell_width / 2.).floor();
        let card_padding = (cell_height / 4.).floor();
        let accent_width = (cell_width / 4.).max(2.).floor();

        // Backgrounds of the sidebar and cards.
        let groups: Vec<_> = tabs.iter().map(|tab| tab.group).collect();
        let rows = sidebar_rows(&groups);

        let mut rects =
            vec![RenderRect::new(0., 0., width, size_info.height(), colors.sidebar, 1.)];
        for row in &rows {
            let SidebarRow::Card { position, line } = *row else { continue };
            let tab = &tabs[position];
            let x = margin;
            let y = line as f32 * cell_height - card_padding;
            let card_width = width - 2. * margin;
            let card_height = CARD_LINES as f32 * cell_height + 2. * card_padding;
            let color = if tab.active { colors.active_card } else { colors.card };
            rects.push(RenderRect::new(x, y, card_width, card_height, color, 1.));

            if tab.active {
                rects.push(RenderRect::new(x, y, accent_width, card_height, tab.accent, 1.));
            }
        }

        self.renderer.draw_rects(&size_info, &metrics, rects);

        for row in rows {
            match row {
                SidebarRow::Header { group, count, line } => {
                    let color = if group == TabGroup::Ready { READY_COLOR } else { colors.detail };
                    let text = format!("{} {count}", group.label());
                    let bg = colors.sidebar;
                    self.draw_sidebar_text(&size_info, line, TEXT_COLUMN, &text, color, bg);
                },
                SidebarRow::Card { position, line } => {
                    self.draw_tab_card(&size_info, position, line, &tabs[position], &colors);
                },
            }
        }
    }

    /// Draw the text of the tab card at `position` in the sidebar.
    fn draw_tab_card(
        &mut self,
        size_info: &SizeInfo,
        index: usize,
        line: usize,
        tab: &SidebarTab,
        colors: &SidebarColors,
    ) {
        let bg = if tab.active { colors.active_card } else { colors.card };
        let last_column = SIDEBAR_COLUMNS - TEXT_COLUMN;

        // Title row: icon, app name and tab shortcut.
        let icon = tab.icon.to_string();
        self.draw_sidebar_text(size_info, line, TEXT_COLUMN, &icon, tab.accent, bg);

        let shortcut = if index < 9 { format!("⌘{}", index + 1) } else { String::new() };
        let mut shortcut_column = last_column - text_width(&shortcut);
        self.draw_sidebar_text(size_info, line, shortcut_column, &shortcut, colors.detail, bg);

        // Mark tabs waiting to be read.
        if tab.group == TabGroup::Ready {
            shortcut_column -= 2;
            self.draw_sidebar_text(size_info, line, shortcut_column, "●", READY_COLOR, bg);
        }

        let title_column = TEXT_COLUMN + 2;
        let title = truncate(&tab.title, shortcut_column.saturating_sub(title_column + 1));
        let title_color = if tab.active { colors.active_title } else { colors.title };
        self.draw_sidebar_text(size_info, line, title_column, &title, title_color, bg);

        // Preview of the focused pane.
        let preview_width = last_column - TEXT_COLUMN;
        for (offset, text) in tab.preview.iter().take(PREVIEW_LINES).enumerate() {
            let text = truncate(text, preview_width);
            let line = line + 1 + offset;
            self.draw_sidebar_text(size_info, line, TEXT_COLUMN, &text, colors.preview, bg);
        }

        // Working directory and status.
        let detail_line = line + 1 + PREVIEW_LINES;
        let status_column = last_column - text_width(&tab.status);
        let status_color = if tab.group == TabGroup::Ready { READY_COLOR } else { colors.detail };
        let status = &tab.status;
        self.draw_sidebar_text(size_info, detail_line, status_column, status, status_color, bg);

        let directory_width = status_column.saturating_sub(TEXT_COLUMN + 1);
        let directory = truncate_start(&tab.working_directory, directory_width);
        self.draw_sidebar_text(size_info, detail_line, TEXT_COLUMN, &directory, colors.detail, bg);
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_sidebar_text(
        &mut self,
        size_info: &SizeInfo,
        line: usize,
        column: usize,
        text: &str,
        fg: Rgb,
        bg: Rgb,
    ) {
        if text.is_empty() {
            return;
        }

        let point = Point::new(line, Column(column));
        self.renderer.draw_string(point, fg, bg, text.chars(), size_info, &mut self.glyph_cache);
    }
}

struct SidebarColors {
    sidebar: Rgb,
    card: Rgb,
    active_card: Rgb,
    divider: Rgb,
    title: Rgb,
    active_title: Rgb,
    preview: Rgb,
    detail: Rgb,
}

/// Blend from `from` towards `to` by `amount` between 0 and 1.
fn mix(from: Rgb, to: Rgb, amount: f32) -> Rgb {
    from * (1. - amount) + to * amount
}

/// Number of cells taken by `text`.
fn text_width(text: &str) -> usize {
    text.chars().map(|c| c.width().unwrap_or(0)).sum()
}

/// Shorten text to fit `max_width` cells, ending with an ellipsis when cut.
fn truncate(text: &str, max_width: usize) -> String {
    if text_width(text) <= max_width {
        return text.to_owned();
    }

    let mut width = 0;
    let mut truncated = String::new();
    for c in text.chars() {
        width += c.width().unwrap_or(0);
        if width + 1 > max_width {
            break;
        }
        truncated.push(c);
    }
    truncated.push('…');
    truncated
}

/// Shorten text to fit `max_width` cells, keeping the end and starting with an ellipsis.
fn truncate_start(text: &str, max_width: usize) -> String {
    if text_width(text) <= max_width {
        return text.to_owned();
    }

    let mut width = 0;
    let mut kept = Vec::new();
    for c in text.chars().rev() {
        width += c.width().unwrap_or(0);
        if width + 1 > max_width {
            break;
        }
        kept.push(c);
    }
    std::iter::once('…').chain(kept.into_iter().rev()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_group_tabs() {
        let groups = [TabGroup::Ready, TabGroup::Idle, TabGroup::Idle];
        assert_eq!(sidebar_rows(&groups), vec![
            SidebarRow::Header { group: TabGroup::Ready, count: 1, line: 1 },
            SidebarRow::Card { position: 0, line: 3 },
            SidebarRow::Header { group: TabGroup::Idle, count: 2, line: 8 },
            SidebarRow::Card { position: 1, line: 10 },
            SidebarRow::Card { position: 2, line: 15 },
        ]);
    }

    #[test]
    fn tab_cards() {
        let size_info = SizeInfo::new(800., 600., 10., 20., 0., 0., false);
        let groups = [TabGroup::Working, TabGroup::Idle];

        // Header of the working group.
        assert_eq!(tab_at(&size_info, &groups, 5., 25.), None);
        // First card, lines 3 to 6.
        assert_eq!(tab_at(&size_info, &groups, 5., 65.), Some(0));
        assert_eq!(tab_at(&size_info, &groups, 5., 135.), Some(0));
        // Gap and header of the idle group.
        assert_eq!(tab_at(&size_info, &groups, 5., 145.), None);
        // Second card, lines 10 to 13.
        assert_eq!(tab_at(&size_info, &groups, 5., 205.), Some(1));
        assert_eq!(tab_at(&size_info, &groups, 330., 205.), None);
    }

    #[test]
    fn truncate_text() {
        assert_eq!(truncate("claude", 10), "claude");
        assert_eq!(truncate("claude code", 6), "claud…");
        assert_eq!(truncate_start("~/projects/clauditty", 10), "…clauditty");
    }

    #[test]
    fn display_paths() {
        let home = Some(Path::new("/Users/me"));
        assert_eq!(display_path(Path::new("/Users/me"), home), "~");
        assert_eq!(display_path(Path::new("/Users/me/projects"), home), "~/projects");
        assert_eq!(display_path(Path::new("/tmp"), home), "/tmp");
    }
}
