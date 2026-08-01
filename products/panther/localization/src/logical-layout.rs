// @file products/panther/localization/src/logical-layout.rs
// @description Defines the logical start and end layout contract.
// @created Diego Martín Lafuente <meerita@icloud.com>

use locale::TextDirection;

/// A logical inline edge of a layout box.
///
/// Component-facing layout describes edges as `Start` and `End`, never as
/// physical left and right, so one layout follows the writing direction of the
/// active locale. `Start` is the edge where inline text begins and `End` is the
/// edge where it ends. The scope is the inline axis, the only axis that
/// right-to-left layout flips; the block axis stays invariant until vertical
/// writing modes exist. The mapping to a physical side lives only in
/// [`LogicalEdge::resolve`], so the direction rule stays in one place.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum LogicalEdge {
    Start,
    End,
}

/// The physical side a logical edge resolves to.
///
/// A physical side is produced only by resolving a [`LogicalEdge`] against a
/// text direction. Component-facing layout never accepts a physical side
/// directly; it exists so the renderer can place a resolved edge on the screen.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum PhysicalSide {
    Left,
    Right,
}

impl LogicalEdge {
    /// Resolves the logical edge to a physical side for the given direction.
    ///
    /// In left-to-right text the start edge is the left side; in right-to-left
    /// text it is the right side. This is the single place that maps a logical
    /// edge to a physical side.
    pub fn resolve(self, direction: TextDirection) -> PhysicalSide {
        match (self, direction) {
            (LogicalEdge::Start, TextDirection::LeftToRight) => PhysicalSide::Left,
            (LogicalEdge::Start, TextDirection::RightToLeft) => PhysicalSide::Right,
            (LogicalEdge::End, TextDirection::LeftToRight) => PhysicalSide::Right,
            (LogicalEdge::End, TextDirection::RightToLeft) => PhysicalSide::Left,
        }
    }
}

#[cfg(test)]
mod tests {
    use locale::TextDirection;

    use super::{LogicalEdge, PhysicalSide};

    #[test]
    fn start_and_end_follow_left_to_right() {
        assert_eq!(
            LogicalEdge::Start.resolve(TextDirection::LeftToRight),
            PhysicalSide::Left
        );
        assert_eq!(
            LogicalEdge::End.resolve(TextDirection::LeftToRight),
            PhysicalSide::Right
        );
    }

    #[test]
    fn start_and_end_flip_for_right_to_left() {
        assert_eq!(
            LogicalEdge::Start.resolve(TextDirection::RightToLeft),
            PhysicalSide::Right
        );
        assert_eq!(
            LogicalEdge::End.resolve(TextDirection::RightToLeft),
            PhysicalSide::Left
        );
    }
}
