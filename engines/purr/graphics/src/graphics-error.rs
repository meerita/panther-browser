// @file engines/purr/graphics/src/graphics-error.rs
// @description Defines the typed failures the graphics interface can return.
// @created Diego Martín Lafuente <meerita@icloud.com>

/// Failure the graphics interface reports to its callers.
///
/// The interface owns these variants. A backend adapter translates its own
/// dependency failure into one of them at its boundary, so no raw backend error
/// and no native-API detail ever reaches this type. Each message is a static,
/// factual, non-secret string.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum GraphicsError {
    #[error("operation is not supported by the backend")]
    Unsupported,
    #[error("resource or pipeline descriptor is invalid")]
    InvalidDescriptor,
    #[error("referenced resource does not exist")]
    ResourceNotFound,
    #[error("graphics device was lost")]
    DeviceLost,
    #[error("frame submission was rejected")]
    SubmissionRejected,
}
