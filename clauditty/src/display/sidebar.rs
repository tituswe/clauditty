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

/// First column of text on a card, leaving room for the accent bar.
const TEXT_COLUMN: usize = 2;

/// Accent color of the active tab and Claude Code.
const ACCENT_COLOR: Rgb = Rgb::new(0xd9, 0x77, 0x57);

/// A tab as shown in the sidebar.
pub struct SidebarTab {
    /// Name of the app in the tab's focused pane.
    pub title: String,
    /// Whether the focused pane runs Claude Code.
    pub claude: bool,
    /// Last lines of text in the focused pane.
    pub preview: Vec<String>,
    /// Working directory of the focused pane.
    pub working_directory: String,
    pub pane_count: usize,
    pub active: bool,
}

/// Width of the sidebar in pixels.
pub fn sidebar_width(size_info: &SizeInfo) -> f32 {
    (SIDEBAR_COLUMNS as f32 * size_info.cell_width()).floor()
}

/// Index of the tab card at a point in window pixels.
pub fn tab_at(size_info: &SizeInfo, x: f32, y: f32) -> Option<usize> {
    if x < 0. || x >= sidebar_width(size_info) || y < 0. {
        return None;
    }

    let line = (y / size_info.cell_height()) as usize;
    let line = line.checked_sub(TOP_LINES)?;

    // Skip the gap between cards.
    (line % LINES_PER_TAB < CARD_LINES).then_some(line / LINES_PER_TAB)
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
    /// Draw the sidebar and the dividers between panes.
    pub fn draw_sidebar(&mut self, tabs: &[SidebarTab], dividers: &[Rect]) {
        let size_info = self.window_size_info;
        let metrics = self.glyph_cache.font_metrics();

        self.renderer.set_origin(0, 0);
        self.renderer.resize(&size_info);

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

        let width = sidebar_width(&size_info);
        let cell_width = size_info.cell_width();
        let cell_height = size_info.cell_height();
        let margin = (cell_width / 2.).floor();
        let card_padding = (cell_height / 4.).floor();
        let accent_width = (cell_width / 4.).max(2.).floor();

        // Backgrounds of the sidebar and cards.
        let mut rects =
            vec![RenderRect::new(0., 0., width, size_info.height(), colors.sidebar, 1.)];
        for (index, tab) in tabs.iter().enumerate() {
            let x = margin;
            let y = (TOP_LINES + index * LINES_PER_TAB) as f32 * cell_height - card_padding;
            let card_width = width - 2. * margin;
            let card_height = CARD_LINES as f32 * cell_height + 2. * card_padding;
            let color = if tab.active { colors.active_card } else { colors.card };
            rects.push(RenderRect::new(x, y, card_width, card_height, color, 1.));

            if tab.active {
                rects.push(RenderRect::new(x, y, accent_width, card_height, ACCENT_COLOR, 1.));
            }
        }

        rects.extend(dividers.iter().map(|divider| {
            let Rect { x, y, width, height } = *divider;
            RenderRect::new(x, y, width, height, colors.divider, 1.)
        }));

        self.renderer.draw_rects(&size_info, &metrics, rects);

        for (index, tab) in tabs.iter().enumerate() {
            self.draw_tab_card(index, tab, &colors);
        }
    }

    /// Draw the text of one tab card.
    fn draw_tab_card(&mut self, index: usize, tab: &SidebarTab, colors: &SidebarColors) {
        let line = TOP_LINES + index * LINES_PER_TAB;
        let bg = if tab.active { colors.active_card } else { colors.card };
        let last_column = SIDEBAR_COLUMNS - TEXT_COLUMN;

        // Title row: icon, app name and tab shortcut.
        let (icon, icon_color) = if tab.claude { ('✳', ACCENT_COLOR) } else { ('›', colors.detail) };
        self.draw_sidebar_text(line, TEXT_COLUMN, &icon.to_string(), icon_color, bg);

        let shortcut = if index < 9 { format!("⌘{}", index + 1) } else { String::new() };
        let shortcut_column = last_column - text_width(&shortcut);
        self.draw_sidebar_text(line, shortcut_column, &shortcut, colors.detail, bg);

        let title_column = TEXT_COLUMN + 2;
        let title = truncate(&tab.title, shortcut_column.saturating_sub(title_column + 1));
        let title_color = if tab.active { colors.active_title } else { colors.title };
        self.draw_sidebar_text(line, title_column, &title, title_color, bg);

        // Preview of the focused pane.
        let preview_width = last_column - TEXT_COLUMN;
        for (offset, text) in tab.preview.iter().take(PREVIEW_LINES).enumerate() {
            let text = truncate(text, preview_width);
            self.draw_sidebar_text(line + 1 + offset, TEXT_COLUMN, &text, colors.preview, bg);
        }

        // Working directory and pane count.
        let detail_line = line + 1 + PREVIEW_LINES;
        let pane_count = match tab.pane_count {
            1 => String::new(),
            count => format!("{count} panes"),
        };
        let pane_count_column = last_column - text_width(&pane_count);
        self.draw_sidebar_text(detail_line, pane_count_column, &pane_count, colors.detail, bg);

        let directory_width = pane_count_column.saturating_sub(TEXT_COLUMN + 1);
        let directory = truncate_start(&tab.working_directory, directory_width);
        self.draw_sidebar_text(detail_line, TEXT_COLUMN, &directory, colors.detail, bg);
    }

    fn draw_sidebar_text(&mut self, line: usize, column: usize, text: &str, fg: Rgb, bg: Rgb) {
        if text.is_empty() {
            return;
        }

        let point = Point::new(line, Column(column));
        let size_info = self.window_size_info;
        self.renderer.draw_string(point, fg, bg, text.chars(), &size_info, &mut self.glyph_cache);
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
    fn tab_cards() {
        let size_info = SizeInfo::new(800., 600., 10., 20., 0., 0., false);

        assert_eq!(tab_at(&size_info, 5., 10.), None);
        assert_eq!(tab_at(&size_info, 5., 25.), Some(0));
        assert_eq!(tab_at(&size_info, 5., 85.), Some(0));
        assert_eq!(tab_at(&size_info, 5., 105.), None);
        assert_eq!(tab_at(&size_info, 5., 125.), Some(1));
        assert_eq!(tab_at(&size_info, 330., 125.), None);
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
