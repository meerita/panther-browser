// @file tools/localization-check/src/check-error.rs
// @description Defines the operational errors the check tool can report.
// @created Diego Martín Lafuente <meerita@icloud.com>

use std::io;
use std::path::PathBuf;

/// An operational failure of the check itself.
///
/// This is distinct from a [`crate::Finding`]: a finding is a localization
/// defect in the tree, while a `CheckError` means the check could not run (a
/// missing directory or an unreadable file). Both fail the run, so the check
/// never passes silently on malformed input.
#[derive(Debug, thiserror::Error)]
pub enum CheckError {
    #[error("localization resource directory not found: {path}")]
    MissingResourceDir { path: PathBuf },

    #[error("reference locale `{locale}` catalogue not found")]
    MissingReferenceLocale { locale: String },

    #[error("cannot read directory {path}: {source}")]
    ReadDir {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("cannot read file {path}: {source}")]
    ReadFile {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}
