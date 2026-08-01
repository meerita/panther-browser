// @file products/panther/localization/src/localization-error.rs
// @description Defines the typed errors the localization crate owns.
// @created Diego Martín Lafuente <meerita@icloud.com>

/// A failure the localization crate owns.
///
/// The crate treats every translation resource as untrusted input. On any of
/// these failures the caller falls back to the `en` reference locale and never
/// crashes, so these variants say why a resource could not be used, never what
/// the untrusted content was. The messages are canonical technical English and
/// carry no resource content and no sensitive values.
#[derive(Clone, PartialEq, Eq, Debug, thiserror::Error)]
pub enum LocalizationError {
    /// No resource exists for the requested locale and name.
    #[error("the requested localization resource is unavailable")]
    ResourceUnavailable,

    /// A resource exists but is not valid and cannot be used.
    #[error("the localization resource is not valid")]
    InvalidResource,
}
