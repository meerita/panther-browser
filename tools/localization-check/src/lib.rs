// @file tools/localization-check/src/lib.rs
// @description Library root for the localization validation and no-prose checks.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Localization check.
//!
//! This tool validates what the compile-time `fl!` macro cannot. It parses every
//! shipped Fluent catalogue and reports missing, duplicate, unused, or obsolete
//! keys, invalid syntax, argument mismatches, empty translations, invalid locale
//! tags, and invalid plural selectors. It also scans foundation and engine
//! crates for user-facing prose, which those crates must never hold (Invariant
//! 1), and for prose that would cross a serialization boundary (Invariant 8).
//!
//! Every problem is a [`Finding`]. Any finding fails the run, so the binary
//! exits non-zero and never passes silently on malformed input.

#[path = "catalogue-validation.rs"]
pub mod catalogue_validation;
#[path = "check-error.rs"]
pub mod check_error;
#[path = "finding.rs"]
pub mod finding;
#[path = "prose-scan.rs"]
pub mod prose_scan;
#[path = "rust-source.rs"]
pub mod rust_source;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

pub use check_error::CheckError;
pub use finding::{Category, Finding, Report};

use catalogue_validation::CatalogueInfo;

const REFERENCE_LOCALE: &str = "en";
const CATALOGUE_DIR: &str = "products/panther/localization/i18n";
const PROSE_ROOTS: [&str; 2] = ["foundation", "engines"];
const SOURCE_ROOTS: [&str; 4] = ["foundation", "engines", "products", "apps"];

/// Runs every localization check against the tree at `root`.
///
/// The returned [`Report`] is empty when the tree passes. A missing resource
/// directory or reference catalogue is a [`CheckError`], not a finding, because
/// the check itself cannot run.
pub fn run(root: &Path) -> Result<Report, CheckError> {
    let mut report = Report::default();

    report.extend(check_catalogues(root)?);

    let prose_roots: Vec<PathBuf> = PROSE_ROOTS.iter().map(|name| root.join(name)).collect();
    let prose_refs: Vec<&Path> = prose_roots.iter().map(PathBuf::as_path).collect();
    report.extend(prose_scan::scan_prose(&prose_refs)?);
    report.extend(prose_scan::scan_ipc_prose(&prose_refs)?);

    Ok(report)
}

fn check_catalogues(root: &Path) -> Result<Vec<Finding>, CheckError> {
    let catalogue_dir = root.join(CATALOGUE_DIR);
    if !catalogue_dir.is_dir() {
        return Err(CheckError::MissingResourceDir {
            path: catalogue_dir,
        });
    }

    let mut findings = Vec::new();
    let mut catalogues: BTreeMap<String, (CatalogueInfo, String)> = BTreeMap::new();

    for locale_dir in locale_directories(&catalogue_dir)? {
        let locale = locale_dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_owned();
        let location = locale_dir.display().to_string();

        if let Some(finding) = catalogue_validation::validate_locale_tag(&locale, &location) {
            findings.push(finding);
        }

        let mut catalogue = CatalogueInfo::default();
        for file in ftl_files(&locale_dir)? {
            let file_location = file.display().to_string();
            let bytes = fs::read(&file).map_err(|source| CheckError::ReadFile {
                path: file.clone(),
                source,
            })?;
            let source = match catalogue_validation::validate_bytes(&bytes, &file_location) {
                Ok(source) => source,
                Err(finding) => {
                    findings.push(finding);
                    continue;
                }
            };
            let (info, mut file_findings) =
                catalogue_validation::validate_resource(source, &file_location);
            findings.append(&mut file_findings);
            catalogue.messages.extend(info.messages);
        }

        catalogues.insert(locale, (catalogue, location));
    }

    let Some((reference, reference_location)) = catalogues.get(REFERENCE_LOCALE) else {
        return Err(CheckError::MissingReferenceLocale {
            locale: REFERENCE_LOCALE.to_owned(),
        });
    };

    for (locale, (candidate, location)) in &catalogues {
        if locale == REFERENCE_LOCALE {
            continue;
        }
        findings.extend(catalogue_validation::check_completeness(
            reference, locale, location, candidate,
        ));
    }

    let referenced = collect_referenced_literals(root)?;
    findings.extend(catalogue_validation::find_unused_keys(
        reference,
        reference_location,
        &referenced,
    ));

    Ok(findings)
}

fn collect_referenced_literals(root: &Path) -> Result<BTreeSet<String>, CheckError> {
    let mut literals = BTreeSet::new();
    for name in SOURCE_ROOTS {
        for file in rust_source::rust_files_under(&root.join(name))? {
            let source = rust_source::read_source(&file)?;
            for literal in rust_source::extract_string_literals(&source) {
                literals.insert(literal.content);
            }
        }
    }
    Ok(literals)
}

fn locale_directories(catalogue_dir: &Path) -> Result<Vec<PathBuf>, CheckError> {
    let mut directories = Vec::new();
    let entries = fs::read_dir(catalogue_dir).map_err(|source| CheckError::ReadDir {
        path: catalogue_dir.to_path_buf(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| CheckError::ReadDir {
            path: catalogue_dir.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if path.is_dir() {
            directories.push(path);
        }
    }
    directories.sort();
    Ok(directories)
}

fn ftl_files(locale_dir: &Path) -> Result<Vec<PathBuf>, CheckError> {
    let mut files = Vec::new();
    let entries = fs::read_dir(locale_dir).map_err(|source| CheckError::ReadDir {
        path: locale_dir.to_path_buf(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| CheckError::ReadDir {
            path: locale_dir.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if path.extension().is_some_and(|extension| extension == "ftl") {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("the tool crate sits two levels below the workspace root")
            .to_path_buf()
    }

    #[test]
    fn the_current_tree_passes() {
        let report = run(&workspace_root()).expect("the check runs against the workspace");
        assert!(report.is_empty(), "expected a clean tree, found:\n{report}");
    }
}
