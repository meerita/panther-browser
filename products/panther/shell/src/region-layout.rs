// @file products/panther/shell/src/region-layout.rs
// @description Maps a window extent to a rectangle for each shell region.
// @created Diego Martín Lafuente <meerita@icloud.com>

use purr_graphics::{Extent2d, Rect};

use crate::shell_region::ShellRegion;

/// Fraction of the window height taken by the top bar band.
const TOP_BAR_HEIGHT_FRACTION: f32 = 0.08;

/// Fraction of the window height taken by the tab strip below the top bar.
const TAB_STRIP_HEIGHT_FRACTION: f32 = 0.05;

/// Fraction of the window width taken by each navigation control.
const CONTROL_WIDTH_FRACTION: f32 = 0.06;

/// Number of navigation controls placed side by side in the top bar.
const NAVIGATION_CONTROL_COUNT: f32 = 3.0;

/// Placed rectangle for every shell region in one window extent.
///
/// The layout works in logical pixel space; the paint seam scales to physical
/// pixels. It is computed once per extent and read many times by the hit-test and
/// the draw builder. All placement uses fractions of the extent, so every
/// rectangle stays in bounds for any extent, including a degenerate one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RegionLayout {
    extent: Extent2d,
    top_bar: Rect,
    navigation_back: Rect,
    navigation_forward: Rect,
    navigation_reload: Rect,
    address_field: Rect,
    tab: Rect,
    viewport: Rect,
}

impl RegionLayout {
    /// Source extent this layout was computed from.
    pub fn extent(&self) -> Extent2d {
        self.extent
    }

    /// Rectangle for one region in logical pixel space.
    pub fn rect(&self, region: ShellRegion) -> Rect {
        match region {
            ShellRegion::TopBar => self.top_bar,
            ShellRegion::NavigationBack => self.navigation_back,
            ShellRegion::NavigationForward => self.navigation_forward,
            ShellRegion::NavigationReload => self.navigation_reload,
            ShellRegion::AddressField => self.address_field,
            ShellRegion::Tab => self.tab,
            ShellRegion::Viewport => self.viewport,
        }
    }
}

/// Places every region for one window extent.
///
/// The top bar spans the width. The three navigation controls sit at the left of
/// the top bar, and the address field fills the rest of it. The tab strip band
/// spans the full width directly below the top bar and holds the dynamic tab
/// strip. The viewport fills the area under the tab strip. The work is constant
/// over the fixed region set.
pub fn layout(extent: Extent2d) -> RegionLayout {
    let width = extent.width as f32;
    let height = extent.height as f32;

    let top_bar_height = height * TOP_BAR_HEIGHT_FRACTION;
    let tab_strip_height = height * TAB_STRIP_HEIGHT_FRACTION;
    let control_width = width * CONTROL_WIDTH_FRACTION;

    let top_bar = Rect::new(0.0, 0.0, width, top_bar_height);

    let navigation_back = Rect::new(0.0, 0.0, control_width, top_bar_height);
    let navigation_forward = Rect::new(control_width, 0.0, control_width, top_bar_height);
    let navigation_reload = Rect::new(control_width * 2.0, 0.0, control_width, top_bar_height);

    let address_x = control_width * NAVIGATION_CONTROL_COUNT;
    let address_width = (width - address_x).max(0.0);
    let address_field = Rect::new(address_x, 0.0, address_width, top_bar_height);

    let tab = Rect::new(0.0, top_bar_height, width, tab_strip_height);

    let viewport_y = top_bar_height + tab_strip_height;
    let viewport_height = (height - viewport_y).max(0.0);
    let viewport = Rect::new(0.0, viewport_y, width, viewport_height);

    RegionLayout {
        extent,
        top_bar,
        navigation_back,
        navigation_forward,
        navigation_reload,
        address_field,
        tab,
        viewport,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOLERANCE: f32 = 0.5;

    fn right(rect: Rect) -> f32 {
        rect.x + rect.width
    }

    fn bottom(rect: Rect) -> f32 {
        rect.y + rect.height
    }

    fn is_in_bounds(rect: Rect, extent: Extent2d) -> bool {
        rect.x >= 0.0
            && rect.y >= 0.0
            && rect.width >= 0.0
            && rect.height >= 0.0
            && right(rect) <= extent.width as f32 + TOLERANCE
            && bottom(rect) <= extent.height as f32 + TOLERANCE
    }

    fn is_inside(inner: Rect, outer: Rect) -> bool {
        inner.x >= outer.x - TOLERANCE
            && inner.y >= outer.y - TOLERANCE
            && right(inner) <= right(outer) + TOLERANCE
            && bottom(inner) <= bottom(outer) + TOLERANCE
    }

    #[test]
    fn top_bar_spans_window_width() {
        let extent = Extent2d::new(1280, 800);
        let placed = layout(extent);
        let top_bar = placed.rect(ShellRegion::TopBar);

        assert_eq!(top_bar.x, 0.0);
        assert_eq!(top_bar.width, extent.width as f32);
    }

    #[test]
    fn viewport_fills_area_below_top_bar() {
        let extent = Extent2d::new(1280, 800);
        let placed = layout(extent);
        let top_bar = placed.rect(ShellRegion::TopBar);
        let viewport = placed.rect(ShellRegion::Viewport);

        assert!(viewport.y >= bottom(top_bar));
        assert_eq!(viewport.width, extent.width as f32);
        assert!((bottom(viewport) - extent.height as f32).abs() <= TOLERANCE);
    }

    #[test]
    fn navigation_controls_and_address_field_sit_inside_top_bar() {
        let extent = Extent2d::new(1280, 800);
        let placed = layout(extent);
        let top_bar = placed.rect(ShellRegion::TopBar);

        for region in [
            ShellRegion::NavigationBack,
            ShellRegion::NavigationForward,
            ShellRegion::NavigationReload,
            ShellRegion::AddressField,
        ] {
            assert!(
                is_inside(placed.rect(region), top_bar),
                "{region:?} escapes the top bar"
            );
        }
    }

    #[test]
    fn small_and_large_extents_stay_in_bounds() {
        for extent in [Extent2d::new(1, 1), Extent2d::new(3840, 2160)] {
            let placed = layout(extent);

            for region in ShellRegion::ALL {
                assert!(
                    is_in_bounds(placed.rect(region), extent),
                    "{region:?} out of bounds for {extent:?}"
                );
            }
        }
    }

    #[test]
    fn regions_do_not_extend_outside_window_extent() {
        let extent = Extent2d::new(1024, 768);
        let placed = layout(extent);

        for region in ShellRegion::ALL {
            let rect = placed.rect(region);
            assert!(right(rect) <= extent.width as f32 + TOLERANCE);
            assert!(bottom(rect) <= extent.height as f32 + TOLERANCE);
        }
    }
}
