// @file foundation/capability-system/src/category.rs
// @description Defines the capability classification category.
// @created Diego Martín Lafuente <meerita@icloud.com>

/// Coarse classification of a capability.
///
/// The category is definition metadata. It is echoed in diagnostic reports and
/// does not drive resolution.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Category {
    WebPlatform,
    EngineService,
    DeveloperTooling,
    ProductFeature,
}
