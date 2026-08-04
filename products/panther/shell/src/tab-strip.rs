// @file products/panther/shell/src/tab-strip.rs
// @description Lays out and resolves the dynamic tab strip inside the strip band.
// @created Diego Martín Lafuente <meerita@icloud.com>

use purr_graphics::Rect;

use crate::pointer_hit_test::{PointerPosition, contains};
use crate::shell_action::ShellAction;

/// Smallest slot width, so the close sub-rect stays hittable.
const MIN_SLOT_WIDTH: f32 = 44.0;

/// Largest slot width, so a few tabs do not stretch across the band.
const MAX_SLOT_WIDTH: f32 = 200.0;

/// Side length of the per-slot close sub-rect.
const CLOSE_SIZE: f32 = 12.0;

/// Margin between the close sub-rect and the slot right edge.
const CLOSE_MARGIN: f32 = 6.0;

/// Neutral snapshot of the tab state the window pushes into the shell.
///
/// The shell works in slot indices only; it never names a `TabId`. The window
/// builds this view from the tab model and hands it to the shell each time the
/// tab state changes. An empty view has no tab and no active slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TabStripView {
    tab_count: usize,
    active: Option<usize>,
}

impl TabStripView {
    /// Builds a view from a tab count and the active slot index.
    pub fn new(tab_count: usize, active: Option<usize>) -> Self {
        Self { tab_count, active }
    }

    /// Number of tabs to render as slots.
    pub fn tab_count(self) -> usize {
        self.tab_count
    }

    /// Active slot index, if any.
    pub fn active(self) -> Option<usize> {
        self.active
    }
}

/// Placed rectangles for one strip state inside the band.
///
/// `slots` holds one body rect per rendered tab, `closes` holds the matching
/// close sub-rect per rendered slot, and `new_tab` is the button at the band
/// right. The layout is bounded by the tab count, which the window caps.
pub(crate) struct StripLayout {
    pub(crate) slots: Vec<Rect>,
    pub(crate) closes: Vec<Rect>,
    pub(crate) new_tab: Rect,
}

/// Places the slots, their close sub-rects, and the new-tab button in the band.
///
/// The new-tab button is a square at the band right, its width equal to the band
/// height. The slots fill the area left of the button, each a fraction of that
/// area clamped to a minimum and maximum width. A slot that would start beyond
/// the slot area is omitted, so overflow is clipped rather than scrolled (D8).
/// Each close sub-rect is a small square inset at its slot right edge and
/// vertically centered (D7). The work is linear in the bounded tab count.
pub(crate) fn strip_layout(band: Rect, tab_count: usize) -> StripLayout {
    let button_width = band.height;
    let new_tab = Rect::new(
        band.x + (band.width - button_width).max(0.0),
        band.y,
        button_width,
        band.height,
    );

    let slot_area_width = (band.width - button_width).max(0.0);

    let mut slots = Vec::with_capacity(tab_count);
    let mut closes = Vec::with_capacity(tab_count);

    if tab_count == 0 {
        return StripLayout {
            slots,
            closes,
            new_tab,
        };
    }

    let slot_width = (slot_area_width / tab_count as f32).clamp(MIN_SLOT_WIDTH, MAX_SLOT_WIDTH);
    let slot_area_right = band.x + slot_area_width;

    for index in 0..tab_count {
        let slot_x = band.x + index as f32 * slot_width;
        if slot_x >= slot_area_right {
            break;
        }

        let slot = Rect::new(slot_x, band.y, slot_width, band.height);
        let close = Rect::new(
            slot.x + slot.width - CLOSE_SIZE - CLOSE_MARGIN,
            slot.y + (slot.height - CLOSE_SIZE) / 2.0,
            CLOSE_SIZE,
            CLOSE_SIZE,
        );

        slots.push(slot);
        closes.push(close);
    }

    StripLayout {
        slots,
        closes,
        new_tab,
    }
}

/// Resolves a band position to a tab action, close sub-rect first.
///
/// The close sub-rect of a slot sits inside the slot body, so it is tested
/// before the body (D7). The new-tab button is tested last. A position that
/// matches nothing returns `None`. Containment is half-open, consistent with the
/// region hit-test.
pub(crate) fn resolve(strip: &StripLayout, position: PointerPosition) -> Option<ShellAction> {
    for (index, close) in strip.closes.iter().enumerate() {
        if contains(*close, position) {
            return Some(ShellAction::CloseTab(index));
        }
    }

    for (index, slot) in strip.slots.iter().enumerate() {
        if contains(*slot, position) {
            return Some(ShellAction::ActivateTab(index));
        }
    }

    if contains(strip.new_tab, position) {
        return Some(ShellAction::NewTab);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOLERANCE: f32 = 0.5;

    fn band() -> Rect {
        Rect::new(0.0, 64.0, 1280.0, 40.0)
    }

    fn right(rect: Rect) -> f32 {
        rect.x + rect.width
    }

    fn bottom(rect: Rect) -> f32 {
        rect.y + rect.height
    }

    fn is_inside(inner: Rect, outer: Rect) -> bool {
        inner.x >= outer.x - TOLERANCE
            && inner.y >= outer.y - TOLERANCE
            && right(inner) <= right(outer) + TOLERANCE
            && bottom(inner) <= bottom(outer) + TOLERANCE
    }

    fn center(rect: Rect) -> PointerPosition {
        PointerPosition::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0)
    }

    #[test]
    fn places_one_slot_and_one_close_per_tab_and_a_new_tab_button() {
        let strip = strip_layout(band(), 3);

        assert_eq!(strip.slots.len(), 3);
        assert_eq!(strip.closes.len(), 3);
        for (slot, close) in strip.slots.iter().zip(strip.closes.iter()) {
            assert!(is_inside(*slot, band()), "slot escapes the band");
            assert!(is_inside(*close, *slot), "close escapes its slot");
        }
        assert!(is_inside(strip.new_tab, band()), "new-tab escapes the band");
    }

    #[test]
    fn slots_are_ordered_left_to_right() {
        let strip = strip_layout(band(), 4);

        for pair in strip.slots.windows(2) {
            assert!(pair[0].x < pair[1].x, "slots are not left to right");
        }
    }

    #[test]
    fn new_tab_button_sits_at_the_band_right() {
        let strip = strip_layout(band(), 2);

        assert!((right(strip.new_tab) - right(band())).abs() <= TOLERANCE);
        assert_eq!(strip.new_tab.width, band().height);
    }

    #[test]
    fn zero_tabs_places_only_the_new_tab_button() {
        let strip = strip_layout(band(), 0);

        assert!(strip.slots.is_empty());
        assert!(strip.closes.is_empty());
        assert!(is_inside(strip.new_tab, band()));
    }

    #[test]
    fn slot_width_never_drops_below_the_minimum() {
        let strip = strip_layout(band(), 8);

        for slot in &strip.slots {
            assert!(slot.width >= MIN_SLOT_WIDTH - TOLERANCE);
        }
    }

    #[test]
    fn resolve_reports_close_for_a_point_in_the_close_sub_rect() {
        let strip = strip_layout(band(), 3);
        let position = center(strip.closes[1]);

        assert_eq!(resolve(&strip, position), Some(ShellAction::CloseTab(1)));
    }

    #[test]
    fn resolve_reports_activate_for_the_slot_body_outside_the_close() {
        let strip = strip_layout(band(), 3);
        let slot = strip.slots[2];
        let position = PointerPosition::new(slot.x + 4.0, center(slot).y);

        assert_eq!(resolve(&strip, position), Some(ShellAction::ActivateTab(2)));
    }

    #[test]
    fn resolve_reports_new_tab_for_the_button() {
        let strip = strip_layout(band(), 2);

        assert_eq!(
            resolve(&strip, center(strip.new_tab)),
            Some(ShellAction::NewTab)
        );
    }

    #[test]
    fn resolve_reports_none_outside_the_strip() {
        let strip = strip_layout(band(), 2);
        let position = PointerPosition::new(band().x + 10.0, band().y - 10.0);

        assert_eq!(resolve(&strip, position), None);
    }

    #[test]
    fn empty_view_reports_no_tabs_and_no_active_slot() {
        let view = TabStripView::default();

        assert_eq!(view.tab_count(), 0);
        assert_eq!(view.active(), None);
    }
}
