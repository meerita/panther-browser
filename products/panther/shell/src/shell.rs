// @file products/panther/shell/src/shell.rs
// @description Owns the shell interaction state and routes pointer and key input.
// @created Diego Martín Lafuente <meerita@icloud.com>

use purr_graphics::{DrawCommand, Extent2d};

use crate::draw_command_builder::build_commands;
use crate::labels::LabelView;
use crate::pointer_hit_test::{PointerPosition, hit_test};
use crate::region_layout::{RegionLayout, layout};
use crate::shell_action::ShellAction;
use crate::shell_region::ShellRegion;
use crate::tab_strip::{TabStripView, resolve, strip_layout};

/// A key event forwarded from the window seam.
///
/// The shell names no windowing type, so the window seam converts its native key
/// into this closed set. The set carries exactly what a single-line address field
/// needs: a typed character and the three edit keys (D2). It has no cursor,
/// selection, or composition key, so it does not grow the field into a general
/// text editor (D1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyInput {
    Character(char),
    Backspace,
    Enter,
    Escape,
}

/// Largest number of characters the address edit buffer accepts.
///
/// A conservative URL-length ceiling that bounds the buffer against untrusted key
/// input. A keystroke beyond the bound is not appended (D8).
pub const ADDRESS_INPUT_MAX_CHARS: usize = 2048;

/// Inclusive ASCII code-point bounds of the accepted address input set.
///
/// The accepted set is printable ASCII including space (`0x20..=0x7E`). The
/// predicate and the enumeration both derive from this one range, so the accepted
/// characters and the pre-packed glyph set never diverge (single source of truth).
const ADDRESS_INPUT_FIRST: u8 = 0x20;
const ADDRESS_INPUT_LAST: u8 = 0x7E;

/// Whether a typed character is accepted into the address edit buffer.
///
/// A character outside the accepted set is not appended, so the buffer holds only
/// characters the chrome atlas pre-packs (D9).
pub fn is_address_input_char(character: char) -> bool {
    let code = character as u32;
    code >= ADDRESS_INPUT_FIRST as u32 && code <= ADDRESS_INPUT_LAST as u32
}

/// Every character in the accepted address input set, in code-point order.
///
/// The chrome text producer packs one glyph per character in this set once, so
/// live address text shapes against a fixed atlas (D9). The set derives from the
/// same range as [`is_address_input_char`], so the two never diverge.
pub fn address_input_charset() -> impl Iterator<Item = char> {
    (ADDRESS_INPUT_FIRST..=ADDRESS_INPUT_LAST).map(char::from)
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
    labels: LabelView,
    hovered: Option<ShellRegion>,
    focused: Option<ShellRegion>,
    // The edit buffer and the committed baseline hold raw user data (typed
    // address characters), not localized UI prose. They are not a localized-message
    // UI text sink, so the internationalization type barrier does not apply here.
    address_buffer: String,
    committed_address: String,
    dirty: bool,
}

impl Shell {
    /// Builds a shell for one extent and requests the initial paint.
    pub fn new(extent: Extent2d) -> Self {
        Self {
            extent,
            layout: layout(extent),
            tabs: TabStripView::default(),
            labels: LabelView::default(),
            hovered: None,
            focused: None,
            address_buffer: String::new(),
            committed_address: String::new(),
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

    /// Replaces the neutral label view the window pushed.
    ///
    /// The window rebuilds the view from the chrome text producer and hands it in
    /// on every locale or label change. The shell marks itself dirty only when the
    /// view actually changes, so an unchanged push drives no repaint (D9). The view
    /// carries only placed glyph geometry and atlas identities, so the shell stays
    /// prose-free.
    pub fn set_labels(&mut self, view: LabelView) {
        if view != self.labels {
            self.labels = view;
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
            if self.focused == Some(ShellRegion::AddressField) {
                self.address_buffer.clear();
                self.address_buffer.push_str(&self.committed_address);
            }
            self.focused = focused;
            self.dirty = true;
        }

        if focused != Some(ShellRegion::Tab) {
            return None;
        }

        let strip = strip_layout(self.layout.rect(ShellRegion::Tab), self.tabs.tab_count());
        resolve(&strip, position)
    }

    /// Replaces the committed address baseline and resets the edit buffer to it.
    ///
    /// The window pushes the active tab's committed address on every tab change and
    /// after a submit, so the field reflects the model and a rejected submit visibly
    /// reverts (D5, D7). The shell marks itself dirty only on a real change. The
    /// string is raw user data, not a localized UI text sink.
    pub fn set_committed_address(&mut self, text: String) {
        let changed = self.committed_address != text || self.address_buffer != text;
        self.committed_address.clear();
        self.committed_address.push_str(&text);
        self.address_buffer = text;
        if changed {
            self.dirty = true;
        }
    }

    /// The current address edit buffer, for the window to shape into glyphs.
    pub fn address_text(&self) -> &str {
        &self.address_buffer
    }

    /// Applies a key to the address field and reports a submit action.
    ///
    /// The key acts only when the address field is focused; otherwise it changes
    /// nothing. A character is appended only when it is accepted and the buffer is
    /// below the bound (D8), backspace removes one character, and escape reverts the
    /// buffer to the committed baseline (D7). Enter reports the buffer as a submit
    /// action; it does not clear the buffer, because the window pushes the committed
    /// text back after the submit resolves. Only `Enter` produces an action.
    pub fn deliver_key(&mut self, key: KeyInput) -> Option<ShellAction> {
        if self.focused != Some(ShellRegion::AddressField) {
            return None;
        }

        match key {
            KeyInput::Character(character) => {
                if is_address_input_char(character)
                    && self.address_buffer.chars().count() < ADDRESS_INPUT_MAX_CHARS
                {
                    self.address_buffer.push(character);
                    self.dirty = true;
                }
                None
            }
            KeyInput::Backspace => {
                if self.address_buffer.pop().is_some() {
                    self.dirty = true;
                }
                None
            }
            KeyInput::Escape => {
                if self.address_buffer != self.committed_address {
                    self.address_buffer.clear();
                    self.address_buffer.push_str(&self.committed_address);
                    self.dirty = true;
                }
                None
            }
            KeyInput::Enter => Some(ShellAction::SubmitAddress(self.address_buffer.clone())),
        }
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
    /// The list reflects the current layout, hover, focus, and labels, so the
    /// window paints exactly the state the shell holds (D5). It clears, fills each
    /// region and the tab strip, then paints the label runs as textured quads.
    pub fn build_commands(&self) -> Vec<DrawCommand> {
        build_commands(
            &self.layout,
            self.hovered,
            self.focused,
            self.tabs,
            &self.labels,
        )
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

    fn focus_address_field(shell: &mut Shell) {
        let position = center_of(shell, ShellRegion::AddressField);
        shell.pointer_pressed(position);
    }

    #[test]
    fn deliver_key_appends_a_character_only_when_the_address_field_is_focused() {
        let mut shell = shell();
        focus_address_field(&mut shell);
        shell.clear_dirty();

        let action = shell.deliver_key(KeyInput::Character('a'));

        assert_eq!(action, None);
        assert_eq!(shell.address_text(), "a");
        assert!(shell.is_dirty());
    }

    #[test]
    fn deliver_key_ignores_a_key_when_a_non_address_region_is_focused() {
        let mut shell = shell();
        shell.pointer_pressed(center_of(&shell, ShellRegion::Viewport));
        shell.clear_dirty();

        let action = shell.deliver_key(KeyInput::Character('a'));

        assert_eq!(action, None);
        assert_eq!(shell.address_text(), "");
        assert!(!shell.is_dirty());
    }

    #[test]
    fn backspace_removes_the_last_character_and_is_a_no_op_when_empty() {
        let mut shell = shell();
        focus_address_field(&mut shell);
        shell.deliver_key(KeyInput::Character('a'));
        shell.deliver_key(KeyInput::Character('b'));

        shell.deliver_key(KeyInput::Backspace);
        assert_eq!(shell.address_text(), "a");

        shell.deliver_key(KeyInput::Backspace);
        assert_eq!(shell.address_text(), "");

        shell.clear_dirty();
        shell.deliver_key(KeyInput::Backspace);
        assert_eq!(shell.address_text(), "");
        assert!(!shell.is_dirty());
    }

    #[test]
    fn escape_resets_the_buffer_to_the_committed_baseline() {
        let mut shell = shell();
        shell.set_committed_address("panther:demo".to_owned());
        focus_address_field(&mut shell);
        shell.deliver_key(KeyInput::Character('x'));
        assert_eq!(shell.address_text(), "panther:demox");

        shell.deliver_key(KeyInput::Escape);

        assert_eq!(shell.address_text(), "panther:demo");
    }

    #[test]
    fn enter_reports_a_submit_action_with_the_current_buffer() {
        let mut shell = shell();
        focus_address_field(&mut shell);
        shell.deliver_key(KeyInput::Character('h'));
        shell.deliver_key(KeyInput::Character('i'));

        let action = shell.deliver_key(KeyInput::Enter);

        assert_eq!(action, Some(ShellAction::SubmitAddress("hi".to_owned())));
        assert_eq!(shell.address_text(), "hi");
    }

    #[test]
    fn the_buffer_stops_accepting_characters_at_the_bound() {
        let mut shell = shell();
        focus_address_field(&mut shell);

        for _ in 0..ADDRESS_INPUT_MAX_CHARS {
            shell.deliver_key(KeyInput::Character('a'));
        }
        assert_eq!(
            shell.address_text().chars().count(),
            ADDRESS_INPUT_MAX_CHARS
        );

        shell.deliver_key(KeyInput::Character('a'));
        assert_eq!(
            shell.address_text().chars().count(),
            ADDRESS_INPUT_MAX_CHARS
        );
    }

    #[test]
    fn a_character_outside_the_accepted_set_is_not_appended() {
        let mut shell = shell();
        focus_address_field(&mut shell);

        shell.deliver_key(KeyInput::Character('a'));
        shell.deliver_key(KeyInput::Character('ñ'));

        assert_eq!(shell.address_text(), "a");
    }

    #[test]
    fn pointer_press_reverts_the_buffer_when_focus_moves_away_from_the_address_field() {
        let mut shell = shell();
        shell.set_committed_address("panther:demo".to_owned());
        focus_address_field(&mut shell);
        shell.deliver_key(KeyInput::Character('x'));
        assert_eq!(shell.address_text(), "panther:demox");

        shell.pointer_pressed(center_of(&shell, ShellRegion::Viewport));

        assert_eq!(shell.address_text(), "panther:demo");
    }

    #[test]
    fn set_committed_address_resets_the_buffer_and_marks_dirty_on_a_real_change_only() {
        let mut shell = shell();
        focus_address_field(&mut shell);
        shell.deliver_key(KeyInput::Character('x'));
        shell.clear_dirty();

        shell.set_committed_address("panther:demo".to_owned());
        assert_eq!(shell.address_text(), "panther:demo");
        assert!(shell.is_dirty());

        shell.clear_dirty();
        shell.set_committed_address("panther:demo".to_owned());
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
    fn set_labels_marks_dirty_on_a_changed_view_only() {
        use purr_text::{
            BundledFont, CmapOneToOneAdapter, GlyphKey, GlyphRunGeneration, GlyphRunId,
            PlacedGlyphRun, ShapingRequest, TextShapingAdapter, TextUnit, build_glyph_atlas,
        };

        let font = BundledFont::load().expect("the bundled font parses");
        let size = TextUnit::from_px(15).expect("in range");
        let run = CmapOneToOneAdapter
            .shape(ShapingRequest {
                font: &font,
                text: "Ab",
                size,
                run_id: GlyphRunId::new(1),
                generation: GlyphRunGeneration::new(1),
            })
            .expect("the label shapes");
        let keys: Vec<GlyphKey> = run
            .glyphs()
            .iter()
            .map(|glyph| GlyphKey::new(glyph.glyph(), size))
            .collect();
        let atlas = build_glyph_atlas(
            &font,
            &keys,
            purr_graphics::ProducerNamespace::new(3),
            purr_graphics::ResourceId::new(1),
            purr_graphics::ResourceGeneration::new(1),
            purr_graphics::DeviceGeneration::new(1),
        )
        .expect("the atlas builds");
        let metrics = font.metrics(size).expect("metrics scale in range");
        let placed = PlacedGlyphRun::from_shaped_run(&run, &atlas, metrics);
        let view = LabelView::new(vec![(ShellRegion::AddressField, placed)]);

        let mut shell = shell();
        shell.clear_dirty();

        shell.set_labels(view.clone());
        assert!(shell.is_dirty());

        shell.clear_dirty();
        shell.set_labels(view);
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
