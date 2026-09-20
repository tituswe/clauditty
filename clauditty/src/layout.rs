//! Split layout of the panes inside a tab.

use serde::{Deserialize, Serialize};

/// Unique identifier of a pane within a window.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PaneId(pub usize);

/// Side a new pane is opened on.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitDirection {
    /// New pane to the right of the current one.
    Right,
    /// New pane below the current one.
    Down,
}

/// Direction to move pane focus in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusDirection {
    Left,
    Right,
    Up,
    Down,
}

/// Side of a split.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Branch {
    First,
    Second,
}

/// Path to a split, following branches down from the root.
pub type SplitPath = Vec<Branch>;

/// A divider between two panes, and the area its split covers.
#[derive(Debug, Clone, PartialEq)]
pub struct Divider {
    pub path: SplitPath,
    pub direction: SplitDirection,
    /// Area covered by the divider itself.
    pub rect: Rect,
    /// Area covered by the split, including both panes.
    pub area: Rect,
}

/// Smallest and largest share of a split one pane can take.
const RATIO_LIMITS: (f32, f32) = (0.1, 0.9);

/// Rectangle in window pixels, with the origin at the top left.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self { x, y, width, height }
    }

    /// Check if a point is inside the rectangle.
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }
}

/// Tree of panes, split right or down.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum Layout {
    Pane(PaneId),
    Split { direction: SplitDirection, ratio: f32, first: Box<Layout>, second: Box<Layout> },
}

impl Layout {
    /// Split `target` in two, placing `new` right of or below it.
    ///
    /// Returns `false` if `target` is not part of the layout.
    pub fn split(&mut self, target: PaneId, new: PaneId, direction: SplitDirection) -> bool {
        match self {
            Layout::Pane(id) if *id == target => {
                let first = Box::new(Layout::Pane(target));
                let second = Box::new(Layout::Pane(new));
                *self = Layout::Split { direction, ratio: 0.5, first, second };
                true
            },
            Layout::Pane(_) => false,
            Layout::Split { first, second, .. } => {
                first.split(target, new, direction) || second.split(target, new, direction)
            },
        }
    }

    /// Remove `target`, letting its sibling take its space.
    ///
    /// Returns `None` when the last pane was removed.
    pub fn remove(self, target: PaneId) -> Option<Layout> {
        match self {
            Layout::Pane(id) if id == target => None,
            Layout::Pane(_) => Some(self),
            Layout::Split { direction, ratio, first, second } => {
                match (first.remove(target), second.remove(target)) {
                    (Some(first), Some(second)) => Some(Layout::Split {
                        direction,
                        ratio,
                        first: Box::new(first),
                        second: Box::new(second),
                    }),
                    (Some(remaining), None) | (None, Some(remaining)) => Some(remaining),
                    (None, None) => None,
                }
            },
        }
    }

    /// All panes, from top left to bottom right.
    pub fn panes(&self) -> Vec<PaneId> {
        let mut panes = Vec::new();
        self.collect_panes(&mut panes);
        panes
    }

    fn collect_panes(&self, panes: &mut Vec<PaneId>) {
        match self {
            Layout::Pane(id) => panes.push(*id),
            Layout::Split { first, second, .. } => {
                first.collect_panes(panes);
                second.collect_panes(panes);
            },
        }
    }

    /// Place every pane inside `area`, leaving a `gap` between split panes.
    pub fn rects(&self, area: Rect, gap: f32) -> Vec<(PaneId, Rect)> {
        let mut rects = Vec::new();
        self.collect_rects(area, gap, &mut rects, &mut Vec::new(), &mut Vec::new());
        rects
    }

    /// Dividers between the panes, with the split each one belongs to.
    pub fn dividers(&self, area: Rect, gap: f32) -> Vec<Divider> {
        let mut dividers = Vec::new();
        self.collect_rects(area, gap, &mut Vec::new(), &mut dividers, &mut Vec::new());
        dividers
    }

    fn collect_rects(
        &self,
        area: Rect,
        gap: f32,
        rects: &mut Vec<(PaneId, Rect)>,
        dividers: &mut Vec<Divider>,
        path: &mut SplitPath,
    ) {
        match self {
            Layout::Pane(id) => rects.push((*id, area)),
            Layout::Split { direction, ratio, first, second } => {
                let (first_area, rect, second_area) = split_area(area, *direction, *ratio, gap);
                dividers.push(Divider { path: path.clone(), direction: *direction, rect, area });

                path.push(Branch::First);
                first.collect_rects(first_area, gap, rects, dividers, path);
                path.pop();

                path.push(Branch::Second);
                second.collect_rects(second_area, gap, rects, dividers, path);
                path.pop();
            },
        }
    }

    /// Share of the split at `path`, between 0 and 1.
    pub fn ratio_mut(&mut self, path: &[Branch]) -> Option<&mut f32> {
        match (self, path.split_first()) {
            (Layout::Split { ratio, .. }, None) => Some(ratio),
            (Layout::Split { first, .. }, Some((Branch::First, rest))) => first.ratio_mut(rest),
            (Layout::Split { second, .. }, Some((Branch::Second, rest))) => second.ratio_mut(rest),
            _ => None,
        }
    }

    /// Move the divider of the split at `path`, keeping both panes usable.
    pub fn set_ratio(&mut self, path: &[Branch], ratio: f32) {
        if let Some(current) = self.ratio_mut(path) {
            *current = ratio.clamp(RATIO_LIMITS.0, RATIO_LIMITS.1);
        }
    }

    /// Closest split above `pane` running in `direction`, and the side the pane is on.
    pub fn split_containing(
        &self,
        pane: PaneId,
        direction: SplitDirection,
    ) -> Option<(SplitPath, Branch)> {
        let mut path = Vec::new();
        let mut found = None;
        self.find_split(pane, direction, &mut path, &mut found);
        found
    }

    fn find_split(
        &self,
        pane: PaneId,
        direction: SplitDirection,
        path: &mut SplitPath,
        found: &mut Option<(SplitPath, Branch)>,
    ) -> bool {
        match self {
            Layout::Pane(id) => *id == pane,
            Layout::Split { direction: split_direction, first, second, .. } => {
                path.push(Branch::First);
                let in_first = first.find_split(pane, direction, path, found);
                path.pop();

                path.push(Branch::Second);
                let in_second = second.find_split(pane, direction, path, found);
                path.pop();

                // Keep the closest split, so deeper ones win.
                if found.is_none() && *split_direction == direction && (in_first || in_second) {
                    let branch = if in_first { Branch::First } else { Branch::Second };
                    *found = Some((path.clone(), branch));
                }

                in_first || in_second
            },
        }
    }
}

impl Divider {
    /// Share of the split for a divider dragged to `position`, in window pixels.
    pub fn ratio_at(&self, position: (f32, f32), gap: f32) -> f32 {
        let (offset, length) = match self.direction {
            SplitDirection::Right => (position.0 - self.area.x, self.area.width - gap),
            SplitDirection::Down => (position.1 - self.area.y, self.area.height - gap),
        };

        (offset / length.max(1.)).clamp(RATIO_LIMITS.0, RATIO_LIMITS.1)
    }

    /// Whether a point is on the divider, within `grab` pixels.
    pub fn contains(&self, x: f32, y: f32, grab: f32) -> bool {
        let rect = Rect::new(
            self.rect.x - grab,
            self.rect.y - grab,
            self.rect.width + 2. * grab,
            self.rect.height + 2. * grab,
        );
        rect.contains(x, y)
    }
}

/// Closest pane next to `from` in `direction`.
///
/// Panes must share part of their edge with `from`. Ties go to the pane sharing the most.
pub fn neighbor(
    rects: &[(PaneId, Rect)],
    from: PaneId,
    direction: FocusDirection,
) -> Option<PaneId> {
    let (_, current) = rects.iter().find(|(id, _)| *id == from)?;

    rects
        .iter()
        .filter(|(id, _)| *id != from)
        .filter_map(|(id, rect)| {
            let horizontal = overlap(current.y, current.height, rect.y, rect.height);
            let vertical = overlap(current.x, current.width, rect.x, rect.width);
            let (distance, shared) = match direction {
                FocusDirection::Left => (current.x - (rect.x + rect.width), horizontal),
                FocusDirection::Right => (rect.x - (current.x + current.width), horizontal),
                FocusDirection::Up => (current.y - (rect.y + rect.height), vertical),
                FocusDirection::Down => (rect.y - (current.y + current.height), vertical),
            };
            (distance >= 0. && shared > 0.).then_some((distance, shared, *id))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0).then(b.1.total_cmp(&a.1)))
        .map(|(_, _, id)| id)
}

/// Pane touching the `edge` of the layout, preferring the leftmost one.
pub fn edge_pane(rects: &[(PaneId, Rect)], edge: FocusDirection) -> Option<PaneId> {
    let key = |rect: &Rect| match edge {
        FocusDirection::Left => (rect.x, rect.y),
        FocusDirection::Right => (-(rect.x + rect.width), rect.y),
        FocusDirection::Up => (rect.y, rect.x),
        FocusDirection::Down => (-(rect.y + rect.height), rect.x),
    };

    rects
        .iter()
        .min_by(|(_, a), (_, b)| {
            let (a, b) = (key(a), key(b));
            a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1))
        })
        .map(|(id, _)| *id)
}

/// Length shared by two ranges on one axis.
fn overlap(start_a: f32, length_a: f32, start_b: f32, length_b: f32) -> f32 {
    (start_a + length_a).min(start_b + length_b) - start_a.max(start_b)
}

/// Split `area` into two parts with a divider between them.
fn split_area(area: Rect, direction: SplitDirection, ratio: f32, gap: f32) -> (Rect, Rect, Rect) {
    match direction {
        SplitDirection::Right => {
            let first_width = ((area.width - gap) * ratio).floor().max(0.);
            let second_x = area.x + first_width + gap;
            let second_width = (area.x + area.width - second_x).max(0.);
            (
                Rect::new(area.x, area.y, first_width, area.height),
                Rect::new(area.x + first_width, area.y, gap, area.height),
                Rect::new(second_x, area.y, second_width, area.height),
            )
        },
        SplitDirection::Down => {
            let first_height = ((area.height - gap) * ratio).floor().max(0.);
            let second_y = area.y + first_height + gap;
            let second_height = (area.y + area.height - second_y).max(0.);
            (
                Rect::new(area.x, area.y, area.width, first_height),
                Rect::new(area.x, area.y + first_height, area.width, gap),
                Rect::new(area.x, second_y, area.width, second_height),
            )
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_right_then_down() {
        let mut layout = Layout::Pane(PaneId(0));
        assert!(layout.split(PaneId(0), PaneId(1), SplitDirection::Right));
        assert!(layout.split(PaneId(1), PaneId(2), SplitDirection::Down));
        assert!(!layout.split(PaneId(9), PaneId(3), SplitDirection::Down));

        assert_eq!(layout.panes(), vec![PaneId(0), PaneId(1), PaneId(2)]);
    }

    #[test]
    fn remove_gives_space_to_sibling() {
        let mut layout = Layout::Pane(PaneId(0));
        layout.split(PaneId(0), PaneId(1), SplitDirection::Right);
        layout.split(PaneId(1), PaneId(2), SplitDirection::Down);

        let layout = layout.remove(PaneId(1)).unwrap();
        assert_eq!(layout.panes(), vec![PaneId(0), PaneId(2)]);

        let layout = layout.remove(PaneId(0)).unwrap();
        assert_eq!(layout, Layout::Pane(PaneId(2)));

        assert_eq!(layout.remove(PaneId(2)), None);
    }

    #[test]
    fn rects_fill_area() {
        let mut layout = Layout::Pane(PaneId(0));
        layout.split(PaneId(0), PaneId(1), SplitDirection::Right);
        layout.split(PaneId(1), PaneId(2), SplitDirection::Down);

        let area = Rect::new(100., 0., 201., 101.);
        let rects = layout.rects(area, 1.);

        assert_eq!(rects, vec![
            (PaneId(0), Rect::new(100., 0., 100., 101.)),
            (PaneId(1), Rect::new(201., 0., 100., 50.)),
            (PaneId(2), Rect::new(201., 51., 100., 50.)),
        ]);

        let dividers = layout.dividers(area, 1.);
        assert_eq!(dividers.len(), 2);
        assert_eq!(dividers[0].rect, Rect::new(200., 0., 1., 101.));
        assert_eq!(dividers[0].path, vec![]);
        assert_eq!(dividers[1].rect, Rect::new(201., 50., 100., 1.));
        assert_eq!(dividers[1].path, vec![Branch::Second]);
    }

    #[test]
    fn neighbors() {
        // 0 | 1
        //   | -
        //   | 2
        let mut layout = Layout::Pane(PaneId(0));
        layout.split(PaneId(0), PaneId(1), SplitDirection::Right);
        layout.split(PaneId(1), PaneId(2), SplitDirection::Down);
        let rects = layout.rects(Rect::new(0., 0., 201., 101.), 1.);

        assert_eq!(neighbor(&rects, PaneId(0), FocusDirection::Right), Some(PaneId(1)));
        assert_eq!(neighbor(&rects, PaneId(2), FocusDirection::Left), Some(PaneId(0)));
        assert_eq!(neighbor(&rects, PaneId(1), FocusDirection::Down), Some(PaneId(2)));
        assert_eq!(neighbor(&rects, PaneId(2), FocusDirection::Up), Some(PaneId(1)));
        assert_eq!(neighbor(&rects, PaneId(0), FocusDirection::Up), None);
        assert_eq!(neighbor(&rects, PaneId(2), FocusDirection::Down), None);
        assert_eq!(neighbor(&rects, PaneId(1), FocusDirection::Right), None);
    }

    #[test]
    fn edge_panes() {
        let mut layout = Layout::Pane(PaneId(0));
        layout.split(PaneId(0), PaneId(1), SplitDirection::Right);
        layout.split(PaneId(1), PaneId(2), SplitDirection::Down);
        let rects = layout.rects(Rect::new(0., 0., 201., 101.), 1.);

        assert_eq!(edge_pane(&rects, FocusDirection::Up), Some(PaneId(0)));
        assert_eq!(edge_pane(&rects, FocusDirection::Down), Some(PaneId(0)));
        assert_eq!(edge_pane(&rects, FocusDirection::Right), Some(PaneId(1)));

        let rects = vec![
            (PaneId(3), Rect::new(0., 0., 100., 50.)),
            (PaneId(4), Rect::new(0., 51., 100., 50.)),
        ];
        assert_eq!(edge_pane(&rects, FocusDirection::Up), Some(PaneId(3)));
        assert_eq!(edge_pane(&rects, FocusDirection::Down), Some(PaneId(4)));
    }

    #[test]
    fn resize_splits() {
        let mut layout = Layout::Pane(PaneId(0));
        layout.split(PaneId(0), PaneId(1), SplitDirection::Right);
        layout.split(PaneId(1), PaneId(2), SplitDirection::Down);

        // The pane on the right is below the vertical divider and left of nothing.
        assert_eq!(
            layout.split_containing(PaneId(2), SplitDirection::Down),
            Some((vec![Branch::Second], Branch::Second))
        );
        assert_eq!(
            layout.split_containing(PaneId(2), SplitDirection::Right),
            Some((vec![], Branch::Second))
        );
        assert_eq!(layout.split_containing(PaneId(9), SplitDirection::Right), None);

        layout.set_ratio(&[], 0.75);
        assert_eq!(layout.ratio_mut(&[]).copied(), Some(0.75));

        // Panes never shrink away completely.
        layout.set_ratio(&[], 2.);
        assert_eq!(layout.ratio_mut(&[]).copied(), Some(RATIO_LIMITS.1));

        let area = Rect::new(0., 0., 201., 101.);
        let rects = layout.rects(area, 1.);
        assert_eq!(rects[0].1.width, 180.);
    }

    #[test]
    fn divider_dragging() {
        let mut layout = Layout::Pane(PaneId(0));
        layout.split(PaneId(0), PaneId(1), SplitDirection::Right);

        let area = Rect::new(100., 0., 201., 101.);
        let divider = layout.dividers(area, 1.).remove(0);

        assert!(divider.contains(201., 50., 2.));
        assert!(!divider.contains(150., 50., 2.));
        assert_eq!(divider.ratio_at((150., 50.), 1.), 0.25);
    }

    #[test]
    fn rect_contains() {
        let rect = Rect::new(10., 10., 5., 5.);
        assert!(rect.contains(10., 14.9));
        assert!(!rect.contains(15., 12.));
    }
}
