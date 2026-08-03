// @file products/panther/shell/src/shell.rs
// @description Owns the shell interaction state and routes pointer and key input.
// @created Diego Martín Lafuente <meerita@icloud.com>

use purr_graphics::{DrawCommand, Extent2d};

use crate::draw_command_builder::build_commands;
use crate::pointer_hit_test::{PointerPosition, hit_test};
use crate::region_layout::{RegionLayout, layout};
use crate::shell_region::ShellRegion;
use crate::tab_strip::{ShellAction, TabStripView, resolve, strip_layout};

/// A raw key event forwarded from the window seam.
///
/// The shell names no windowing type, so it carries the key as a neutral raw
/// value. The window seam converts its native key into this value. The shell does
/// not interpret the value at M1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KeyInput {
    raw: u32,
}

impl KeyInput {
    pub fn new(raw: u32) -> Self {
        Self { raw }
    }

    /// Neutral raw key value the window forwarded.
    pub fn raw(self) -> u32 {
        self.raw
    }
}

/// Interaction state and input router for the minimal shell.
///
/// The shell owns the current extent, the cached layout, the hovered and focused
/// regions, and a dirty flag. It works in surface pixel space. The window seam
/// forwards extents, pointer positions, and key events; the shell never names a
/// windowing type. To keep on-demand redraw correct (D5), the shell marks itself
/// dirty only on an actual state change.
pub struct Shell {
    extent: Extent2d,
    layout: RegionLayout,
    tabs: TabStripView,
    hovered: Option<ShellRegion>,
    focused: Option<ShellRegion>,
    dirty: bool,
}

impl Shell {
    /// Builds a shell for one extent and requests the initial paint.
    pub fn new(extent: Extent2d) -> Self {
        Self {
            extent,
            layout: layout(extent),
            tabs: TabStripView::default(),
            hovered: None,
            focused: None,
            dirty: true,
        }
    }

    /// Recomputes the layout for a new extent and requests a repaint.
    pub fn resize(&mut self, extent: Extent2d) {
        self.extent = extent;
        self.layout = layout(extent);
        self.dirty = true;
    }

    /// Replaces the neutral tab-strip view the window pushed.
    ///
    /// The window rebuilds the view from the tab model and hands it in on every
    /// tab change. The shell marks itself dirty only when the view actually
    /// changes, so an unchanged push drives no repaint (D9).
    pub fn set_tabs(&mut self, view: TabStripView) {
        if view != self.tabs {
            self.tabs = view;
            self.dirty = true;
        }
    }

    /// Updates the hovered region from the pointer position.
    ///
    /// A move that stays inside the same region requests no repaint, so an idle
    /// pointer does not drive redraw (D5).
    pub fn pointer_moved(&mut self, position: PointerPosition) {
        let hovered = hit_test(&self.layout, position);
        if hovered != self.hovered {
            self.hovered = hovered;
            self.dirty = true;
        }
    }

    /// Sets the focus and reports a tab action for a strip press.
    ///
    /// The press focuses the region under the pointer, as before; a press inside
    /// the already focused region requests no repaint and a press outside every
    /// region clears the focus (D5). When the press lands in the tab strip band,
    /// the strip resolves it to a neutral `ShellAction` (activate, new tab, or
    /// close) and returns it; every other press returns `None`. The shell works
    /// in slot indices only and never names a tab identity (D3).
    pub fn pointer_pressed(&mut self, position: PointerPosition) -> Option<ShellAction> {
        let focused = hit_test(&self.layout, position);
        if focused != self.focused {
            self.focused = focused;
            self.dirty = true;
        }

        if focused != Some(ShellRegion::Tab) {
            return None;
        }

        let strip = strip_layout(self.layout.rect(ShellRegion::Tab), self.tabs.tab_count());
        resolve(&strip, position)
    }

    /// Returns the focused region as the key delivery target.
    ///
    /// The key payload is unused at M1: the shell has no text consumer yet. The
    /// method exists so the window exercises the delivery seam that a later phase
    /// extends into real keyboard handling.
    pub fn deliver_key(&self, key: KeyInput) -> Option<ShellRegion> {
        let _ = key;
        self.focused
    }

    /// Whether the shell state changed since the last paint.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Clears the dirty flag after a paint.
    pub fn clear_dirty(&mut self) {
        self.dirty = false;
    }

    /// Current window extent in surface pixels.
    pub fn extent(&self) -> Extent2d {
        self.extent
    }

    /// Cached region layout for the current extent.
    pub fn layout(&self) -> &RegionLayout {
        &self.layout
    }

    /// Region under the pointer, if any.
    pub fn hovered(&self) -> Option<ShellRegion> {
        self.hovered
    }

    /// Region that receives key input, if any.
    pub fn focused(&self) -> Option<ShellRegion> {
        self.focused
    }

    /// Ordered draw-command list that paints the current chrome state.
    ///
    /// The list reflects the current layout, hover, and focus, so the window
    /// paints exactly the state the shell holds (D5). The command set is `Clear`
    /// and `FillRect` only (D1).
    pub fn build_commands(&self) -> Vec<DrawCommand> {
        build_commands(&self.layout, self.hovered, self.focused, self.tabs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tab_strip::strip_layout;
    use purr_graphics::Rect;

    fn shell() -> Shell {
        Shell::new(Extent2d::new(1280, 800))
    }

    fn center(rect: Rect) -> PointerPosition {
        PointerPosition::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0)
    }

    fn center_of(shell: &Shell, region: ShellRegion) -> PointerPosition {
        center(shell.layout().rect(region))
    }

    #[test]
    fn new_requests_initial_paint() {
        let shell = shell();

        assert!(shell.is_dirty());
        assert_eq!(shell.hovered(), None);
        assert_eq!(shell.focused(), None);
    }

    #[test]
    fn resize_recomputes_layout_and_marks_dirty() {
        let mut shell = shell();
        shell.clear_dirty();

        let extent = Extent2d::new(1024, 768);
        shell.resize(extent);

        assert!(shell.is_dirty());
        assert_eq!(shell.extent(), extent);
        assert_eq!(shell.layout().extent(), extent);
    }

    #[test]
    fn pointer_move_into_new_region_updates_hover_and_marks_dirty() {
        let mut shell = shell();
        shell.clear_dirty();

        shell.pointer_moved(center_of(&shell, ShellRegion::AddressField));

        assert_eq!(shell.hovered(), Some(ShellRegion::AddressField));
        assert!(shell.is_dirty());
    }

    #[test]
    fn pointer_move_within_same_region_does_not_mark_dirty() {
        let mut shell = shell();
        let rect = shell.layout().rect(ShellRegion::AddressField);
        shell.pointer_moved(center(rect));
        shell.clear_dirty();

        shell.pointer_moved(PointerPosition::new(rect.x + 1.0, rect.y + 1.0));

        assert_eq!(shell.hovered(), Some(ShellRegion::AddressField));
        assert!(!shell.is_dirty());
    }

    #[test]
    fn pointer_press_focuses_region_under_pointer() {
        let mut shell = shell();

        let action = shell.pointer_pressed(center_of(&shell, ShellRegion::Viewport));

        assert_eq!(action, None);
        assert_eq!(shell.focused(), Some(ShellRegion::Viewport));
    }

    #[test]
    fn pointer_press_on_same_region_does_not_mark_dirty() {
        let mut shell = shell();
        shell.pointer_pressed(center_of(&shell, ShellRegion::Viewport));
        shell.clear_dirty();

        shell.pointer_pressed(center_of(&shell, ShellRegion::Viewport));

        assert_eq!(shell.focused(), Some(ShellRegion::Viewport));
        assert!(!shell.is_dirty());
    }

    #[test]
    fn deliver_key_returns_focus_and_changes_no_state() {
        let mut shell = shell();
        shell.pointer_pressed(center_of(&shell, ShellRegion::AddressField));
        shell.clear_dirty();

        let target = shell.deliver_key(KeyInput::new(42));

        assert_eq!(target, Some(ShellRegion::AddressField));
        assert_eq!(shell.focused(), Some(ShellRegion::AddressField));
        assert!(!shell.is_dirty());
    }

    #[test]
    fn clear_dirty_clears_the_flag() {
        let mut shell = shell();

        shell.clear_dirty();

        assert!(!shell.is_dirty());
    }

    #[test]
    fn set_tabs_marks_dirty_on_a_changed_view_only() {
        let mut shell = shell();
        shell.clear_dirty();

        shell.set_tabs(TabStripView::new(2, Some(0)));
        assert!(shell.is_dirty());

        shell.clear_dirty();
        shell.set_tabs(TabStripView::new(2, Some(0)));
        assert!(!shell.is_dirty());
    }

    #[test]
    fn press_on_a_slot_returns_activate_and_focuses_the_band() {
        let mut shell = shell();
        shell.set_tabs(TabStripView::new(3, Some(0)));
        let strip = strip_layout(shell.layout().rect(ShellRegion::Tab), 3);
        let slot = strip.slots[1];

        let action = shell.pointer_pressed(PointerPosition::new(slot.x + 4.0, center(slot).y));

        assert_eq!(action, Some(ShellAction::ActivateTab(1)));
        assert_eq!(shell.focused(), Some(ShellRegion::Tab));
    }

    #[test]
    fn press_on_a_close_sub_rect_returns_close() {
        let mut shell = shell();
        shell.set_tabs(TabStripView::new(3, Some(0)));
        let strip = strip_layout(shell.layout().rect(ShellRegion::Tab), 3);

        let action = shell.pointer_pressed(center(strip.closes[2]));

        assert_eq!(action, Some(ShellAction::CloseTab(2)));
    }

    #[test]
    fn press_on_the_new_tab_button_returns_new_tab() {
        let mut shell = shell();
        shell.set_tabs(TabStripView::new(3, Some(0)));
        let strip = strip_layout(shell.layout().rect(ShellRegion::Tab), 3);

        let action = shell.pointer_pressed(center(strip.new_tab));

        assert_eq!(action, Some(ShellAction::NewTab));
    }

    #[test]
    fn press_outside_the_strip_returns_none() {
        let mut shell = shell();
        shell.set_tabs(TabStripView::new(3, Some(0)));

        let action = shell.pointer_pressed(center_of(&shell, ShellRegion::AddressField));

        assert_eq!(action, None);
    }
}
