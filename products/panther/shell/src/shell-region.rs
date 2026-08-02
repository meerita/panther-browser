// @file products/panther/shell/src/shell-region.rs
// @description Defines the shell chrome regions and their placeholder colors.
// @created Diego Martín Lafuente <meerita@icloud.com>

use purr_graphics::Color;

/// One paintable region of the minimal shell chrome.
///
/// The set is closed and small. Each variant names one chrome element that the
/// layout places and the draw builder fills. A new chrome element requires a new
/// variant, which forces every exhaustive match to be reviewed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShellRegion {
    TopBar,
    NavigationBack,
    NavigationForward,
    NavigationReload,
    AddressField,
    Tab,
    Viewport,
}

/// Background fill behind the chrome.
///
/// The layout covers the whole window, so this color shows only during the
/// transient before the first region paints. It is a placeholder (D1).
pub const CLEAR_COLOR: Color = Color::new(0.10, 0.11, 0.13, 1.0);

const TOP_BAR_COLOR: Color = Color::new(0.18, 0.19, 0.22, 1.0);
const NAVIGATION_BACK_COLOR: Color = Color::new(0.30, 0.32, 0.36, 1.0);
const NAVIGATION_FORWARD_COLOR: Color = Color::new(0.34, 0.36, 0.40, 1.0);
const NAVIGATION_RELOAD_COLOR: Color = Color::new(0.38, 0.40, 0.44, 1.0);
const ADDRESS_FIELD_COLOR: Color = Color::new(0.24, 0.25, 0.28, 1.0);
const TAB_COLOR: Color = Color::new(0.28, 0.30, 0.34, 1.0);
const VIEWPORT_COLOR: Color = Color::new(0.94, 0.95, 0.96, 1.0);

impl ShellRegion {
    /// Every region in fixed order, top bar first and viewport last.
    ///
    /// The order controls paint order in the draw builder: the top bar paints
    /// before the controls that sit inside it.
    pub const ALL: [ShellRegion; 7] = [
        ShellRegion::TopBar,
        ShellRegion::NavigationBack,
        ShellRegion::NavigationForward,
        ShellRegion::NavigationReload,
        ShellRegion::AddressField,
        ShellRegion::Tab,
        ShellRegion::Viewport,
    ];

    /// Placeholder base color for the region.
    ///
    /// The colors carry no meaning yet; they only make each region visible and
    /// distinct at M1 (D1).
    pub const fn base_color(self) -> Color {
        match self {
            ShellRegion::TopBar => TOP_BAR_COLOR,
            ShellRegion::NavigationBack => NAVIGATION_BACK_COLOR,
            ShellRegion::NavigationForward => NAVIGATION_FORWARD_COLOR,
            ShellRegion::NavigationReload => NAVIGATION_RELOAD_COLOR,
            ShellRegion::AddressField => ADDRESS_FIELD_COLOR,
            ShellRegion::Tab => TAB_COLOR,
            ShellRegion::Viewport => VIEWPORT_COLOR,
        }
    }
}
