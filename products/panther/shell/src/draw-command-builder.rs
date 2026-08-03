// @file products/panther/shell/src/draw-command-builder.rs
// @description Builds the ordered draw-command list that paints the shell chrome.
// @created Diego Martín Lafuente <meerita@icloud.com>

use purr_graphics::DrawCommand;

use crate::region_layout::RegionLayout;
use crate::shell_region::{
    ACTIVE_TAB_COLOR, CLEAR_COLOR, CLOSE_COLOR, INACTIVE_TAB_COLOR, NEW_TAB_COLOR, ShellRegion,
};
use crate::tab_strip::{TabStripView, strip_layout};

/// Builds the ordered colored-rectangle list that paints the chrome.
///
/// The list starts with one `Clear` to the background, then one `FillRect` per
/// region in `ShellRegion::ALL` order (top bar first, viewport last), so a
/// control paints over the top bar band it sits in. The hovered and focused
/// regions take the hover and focus color, so the interaction state is visible
/// (D1, D3). The `Tab` band then holds the dynamic strip: one fill per slot (the
/// active slot highlighted), one fill per close sub-rect, and the new-tab button,
/// all painted over the band. The capacity is reserved from the fixed region
/// count and the bounded strip element count, so the list never reallocates.
pub fn build_commands(
    layout: &RegionLayout,
    hovered: Option<ShellRegion>,
    focused: Option<ShellRegion>,
    tabs: TabStripView,
) -> Vec<DrawCommand> {
    let mut commands = Vec::with_capacity(ShellRegion::ALL.len() + 2 + 2 * tabs.tab_count());

    commands.push(DrawCommand::Clear { color: CLEAR_COLOR });

    for region in ShellRegion::ALL {
        let color = region.display_color(hovered == Some(region), focused == Some(region));
        commands.push(DrawCommand::FillRect {
            rect: layout.rect(region),
            color,
        });
    }

    let strip = strip_layout(layout.rect(ShellRegion::Tab), tabs.tab_count());

    for (index, slot) in strip.slots.iter().enumerate() {
        let color = if tabs.active() == Some(index) {
            ACTIVE_TAB_COLOR
        } else {
            INACTIVE_TAB_COLOR
        };
        commands.push(DrawCommand::FillRect { rect: *slot, color });
    }

    for close in &strip.closes {
        commands.push(DrawCommand::FillRect {
            rect: *close,
            color: CLOSE_COLOR,
        });
    }

    commands.push(DrawCommand::FillRect {
        rect: strip.new_tab,
        color: NEW_TAB_COLOR,
    });

    commands
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::region_layout::layout;
    use purr_graphics::Extent2d;

    fn built(hovered: Option<ShellRegion>, focused: Option<ShellRegion>) -> Vec<DrawCommand> {
        let layout = layout(Extent2d::new(1280, 800));
        build_commands(&layout, hovered, focused, TabStripView::default())
    }

    #[test]
    fn first_command_clears_to_background() {
        let commands = built(None, None);

        assert_eq!(commands[0], DrawCommand::Clear { color: CLEAR_COLOR });
    }

    #[test]
    fn empty_view_fills_every_region_and_the_new_tab_button() {
        let commands = built(None, None);

        let fills = commands
            .iter()
            .filter(|command| matches!(command, DrawCommand::FillRect { .. }))
            .count();

        assert_eq!(fills, ShellRegion::ALL.len() + 1);
        assert_eq!(commands.len(), ShellRegion::ALL.len() + 2);
    }

    #[test]
    fn fill_order_follows_the_fixed_region_order() {
        let extent = Extent2d::new(1280, 800);
        let placed = layout(extent);
        let commands = build_commands(&placed, None, None, TabStripView::default());

        for (index, region) in ShellRegion::ALL.iter().enumerate() {
            assert_eq!(
                commands[index + 1],
                DrawCommand::FillRect {
                    rect: placed.rect(*region),
                    color: region.base_color(),
                },
                "{region:?} paints out of order"
            );
        }
    }

    #[test]
    fn each_region_fill_rectangle_equals_the_region_rectangle() {
        let extent = Extent2d::new(1024, 768);
        let placed = layout(extent);
        let commands = build_commands(&placed, None, None, TabStripView::default());

        for (index, region) in ShellRegion::ALL.iter().enumerate() {
            let DrawCommand::FillRect { rect, .. } = commands[index + 1] else {
                panic!("{region:?} is not a fill");
            };
            assert_eq!(rect, placed.rect(*region));
        }
    }

    #[test]
    fn hovered_region_uses_the_hover_color() {
        let region = ShellRegion::AddressField;
        let commands = built(Some(region), None);
        let index = ShellRegion::ALL.iter().position(|r| *r == region).unwrap() + 1;

        assert_eq!(
            commands[index],
            DrawCommand::FillRect {
                rect: layout(Extent2d::new(1280, 800)).rect(region),
                color: region.display_color(true, false),
            }
        );
        assert_ne!(region.display_color(true, false), region.base_color());
    }

    #[test]
    fn focused_region_uses_the_focus_color() {
        let region = ShellRegion::Viewport;
        let commands = built(None, Some(region));
        let index = ShellRegion::ALL.iter().position(|r| *r == region).unwrap() + 1;

        assert_eq!(
            commands[index],
            DrawCommand::FillRect {
                rect: layout(Extent2d::new(1280, 800)).rect(region),
                color: region.display_color(false, true),
            }
        );
        assert_ne!(region.display_color(false, true), region.base_color());
    }

    #[test]
    fn focus_takes_precedence_over_hover() {
        let region = ShellRegion::AddressField;
        let commands = built(Some(region), Some(region));
        let index = ShellRegion::ALL.iter().position(|r| *r == region).unwrap() + 1;

        let DrawCommand::FillRect { color, .. } = commands[index] else {
            panic!("{region:?} is not a fill");
        };
        assert_eq!(color, region.display_color(true, true));
        assert_eq!(color, region.display_color(false, true));
    }

    fn fills_after_regions(commands: &[DrawCommand]) -> &[DrawCommand] {
        &commands[ShellRegion::ALL.len() + 1..]
    }

    #[test]
    fn strip_emits_one_slot_one_close_per_tab_and_one_new_tab_fill() {
        let placed = layout(Extent2d::new(1280, 800));
        let commands = build_commands(&placed, None, None, TabStripView::new(3, Some(0)));

        let strip = fills_after_regions(&commands);

        assert_eq!(strip.len(), 3 + 3 + 1);
    }

    #[test]
    fn active_slot_uses_the_active_color_and_an_inactive_slot_does_not() {
        let placed = layout(Extent2d::new(1280, 800));
        let commands = build_commands(&placed, None, None, TabStripView::new(3, Some(1)));

        let strip = fills_after_regions(&commands);

        let color_of = |command: &DrawCommand| match command {
            DrawCommand::FillRect { color, .. } => *color,
            _ => panic!("strip element is not a fill"),
        };
        assert_eq!(color_of(&strip[1]), ACTIVE_TAB_COLOR);
        assert_eq!(color_of(&strip[0]), INACTIVE_TAB_COLOR);
        assert_ne!(ACTIVE_TAB_COLOR, INACTIVE_TAB_COLOR);
    }
}
