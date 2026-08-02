// @file products/panther/shell/src/pointer-hit-test.rs
// @description Resolves a pointer position to the shell region under it.
// @created Diego Martín Lafuente <meerita@icloud.com>

use purr_graphics::Rect;

use crate::region_layout::RegionLayout;
use crate::shell_region::ShellRegion;

/// Pointer location in surface pixel space.
///
/// The shell defines its own pointer type so the hit-test never names a
/// windowing type. The window seam converts its native position into this value
/// before it reaches the shell.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointerPosition {
    pub x: f32,
    pub y: f32,
}

impl PointerPosition {
    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// Resolves a pointer position to the most specific region under it.
///
/// The navigation controls, the address field, and the tab sit inside the top
/// bar band, so they are tested first. The top bar band is tested next, then the
/// viewport. A position outside every region returns `None`. The work is constant
/// over the fixed region set.
pub fn hit_test(layout: &RegionLayout, position: PointerPosition) -> Option<ShellRegion> {
    const PRIORITY_ORDER: [ShellRegion; 7] = [
        ShellRegion::NavigationBack,
        ShellRegion::NavigationForward,
        ShellRegion::NavigationReload,
        ShellRegion::AddressField,
        ShellRegion::Tab,
        ShellRegion::TopBar,
        ShellRegion::Viewport,
    ];

    PRIORITY_ORDER
        .into_iter()
        .find(|&region| contains(layout.rect(region), position))
}

/// Tests half-open containment (`x0 <= x < x1`, `y0 <= y < y1`).
///
/// The half-open rule stops two adjacent regions from both claiming a shared
/// edge pixel, so every position resolves to at most one region.
fn contains(rect: Rect, position: PointerPosition) -> bool {
    position.x >= rect.x
        && position.x < rect.x + rect.width
        && position.y >= rect.y
        && position.y < rect.y + rect.height
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::region_layout::layout;
    use purr_graphics::Extent2d;

    fn center(rect: Rect) -> PointerPosition {
        PointerPosition::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0)
    }

    #[test]
    fn address_field_wins_over_top_bar() {
        let placed = layout(Extent2d::new(1280, 800));
        let position = center(placed.rect(ShellRegion::AddressField));

        assert_eq!(hit_test(&placed, position), Some(ShellRegion::AddressField));
    }

    #[test]
    fn navigation_control_is_resolved() {
        let placed = layout(Extent2d::new(1280, 800));

        for region in [
            ShellRegion::NavigationBack,
            ShellRegion::NavigationForward,
            ShellRegion::NavigationReload,
        ] {
            let position = center(placed.rect(region));
            assert_eq!(hit_test(&placed, position), Some(region));
        }
    }

    #[test]
    fn top_bar_band_resolves_to_a_child_region() {
        let placed = layout(Extent2d::new(1280, 800));
        let top_bar = placed.rect(ShellRegion::TopBar);
        let row_y = top_bar.y + top_bar.height / 2.0;
        let children = [
            ShellRegion::NavigationBack,
            ShellRegion::NavigationForward,
            ShellRegion::NavigationReload,
            ShellRegion::AddressField,
        ];

        for step in 0..top_bar.width as u32 {
            let position = PointerPosition::new(step as f32, row_y);
            let region = hit_test(&placed, position).expect("top bar band is fully claimed");
            assert!(
                children.contains(&region),
                "{region:?} claimed a top bar position"
            );
        }
    }

    #[test]
    fn content_area_resolves_to_viewport() {
        let placed = layout(Extent2d::new(1280, 800));
        let position = center(placed.rect(ShellRegion::Viewport));

        assert_eq!(hit_test(&placed, position), Some(ShellRegion::Viewport));
    }

    #[test]
    fn position_outside_window_returns_none() {
        let placed = layout(Extent2d::new(1280, 800));

        assert_eq!(hit_test(&placed, PointerPosition::new(-1.0, 10.0)), None);
        assert_eq!(hit_test(&placed, PointerPosition::new(1280.0, 10.0)), None);
        assert_eq!(hit_test(&placed, PointerPosition::new(10.0, 800.0)), None);
    }

    #[test]
    fn shared_edge_resolves_to_one_region() {
        let placed = layout(Extent2d::new(1280, 800));
        let back = placed.rect(ShellRegion::NavigationBack);
        let edge = PointerPosition::new(back.x + back.width, back.y + back.height / 2.0);

        assert_eq!(
            hit_test(&placed, edge),
            Some(ShellRegion::NavigationForward)
        );
    }
}
