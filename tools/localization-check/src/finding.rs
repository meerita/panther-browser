// @file tools/localization-check/src/finding.rs
// @description Defines the validation findings the checks report.
// @created Diego Martín Lafuente <meerita@icloud.com>

use std::fmt;

/// One validation problem found by a check.
///
/// A finding is not an operational error. It records a localization defect that
/// must fail the build. The [`Category`] carries a stable code so the meaning is
/// machine-readable, while `location` and `detail` explain the specific case.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    category: Category,
    location: String,
    detail: String,
}

impl Finding {
    pub fn new(category: Category, location: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            category,
            location: location.into(),
            detail: detail.into(),
        }
    }

    pub fn category(&self) -> Category {
        self.category
    }
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[{}] {}: {}",
            self.category.code(),
            self.location,
            self.detail
        )
    }
}

/// Stable classification of a [`Finding`].
///
/// Each variant maps to a stable `SCREAMING_SNAKE_CASE` code so a report line
/// stays machine-readable even as the human detail text changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    CatalogueSyntax,
    CatalogueEncoding,
    ResourceTooLarge,
    DuplicateKey,
    MissingKey,
    ObsoleteKey,
    UnusedKey,
    ArgumentMismatch,
    EmptyTranslation,
    InvalidLocaleTag,
    InvalidSelector,
    ProseLiteral,
    IpcProse,
}

impl Category {
    pub fn code(self) -> &'static str {
        match self {
            Category::CatalogueSyntax => "CATALOGUE_SYNTAX",
            Category::CatalogueEncoding => "CATALOGUE_ENCODING",
            Category::ResourceTooLarge => "RESOURCE_TOO_LARGE",
            Category::DuplicateKey => "DUPLICATE_KEY",
            Category::MissingKey => "MISSING_KEY",
            Category::ObsoleteKey => "OBSOLETE_KEY",
            Category::UnusedKey => "UNUSED_KEY",
            Category::ArgumentMismatch => "ARGUMENT_MISMATCH",
            Category::EmptyTranslation => "EMPTY_TRANSLATION",
            Category::InvalidLocaleTag => "INVALID_LOCALE_TAG",
            Category::InvalidSelector => "INVALID_SELECTOR",
            Category::ProseLiteral => "PROSE_LITERAL",
            Category::IpcProse => "IPC_PROSE",
        }
    }
}

/// The collected result of a validation run.
///
/// A report with no findings means the tree passes. Any finding fails the run,
/// so the caller exits non-zero and never passes silently on malformed input.
#[derive(Default)]
pub struct Report {
    findings: Vec<Finding>,
}

impl Report {
    pub fn push(&mut self, finding: Finding) {
        self.findings.push(finding);
    }

    pub fn extend(&mut self, findings: impl IntoIterator<Item = Finding>) {
        self.findings.extend(findings);
    }

    pub fn is_empty(&self) -> bool {
        self.findings.is_empty()
    }

    pub fn len(&self) -> usize {
        self.findings.len()
    }

    pub fn findings(&self) -> &[Finding] {
        &self.findings
    }
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for finding in &self.findings {
            writeln!(f, "{finding}")?;
        }
        Ok(())
    }
}
