// @file tools/localization-check/src/catalogue-validation.rs
// @description Validates Fluent catalogues and non-reference completeness.
// @created Diego Martín Lafuente <meerita@icloud.com>

use std::collections::{BTreeMap, BTreeSet};

use fluent_syntax::ast::{
    CallArguments, Entry, Expression, InlineExpression, Pattern, PatternElement, VariantKey,
};
use fluent_syntax::parser;
use unic_langid::LanguageIdentifier;

use crate::finding::{Category, Finding};

/// Maximum size of a single catalogue file, in bytes.
///
/// A shipped catalogue is untrusted input, so its size is bounded before it is
/// parsed. The bound matches the runtime loader so the check and the loader
/// agree on what is acceptable.
pub const MAX_RESOURCE_BYTES: usize = 256 * 1024;

/// Upper bound on syntax findings reported per file.
///
/// A malformed file can produce many cascading parser errors. The report stays
/// bounded so a hostile or corrupt catalogue cannot flood the output.
const MAX_SYNTAX_FINDINGS: usize = 20;

const CLDR_CATEGORIES: [&str; 6] = ["zero", "one", "two", "few", "many", "other"];

/// The variables one message interpolates.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MessageInfo {
    pub variables: BTreeSet<String>,
}

/// The parsed contents of one catalogue, keyed by message identifier.
#[derive(Clone, Debug, Default)]
pub struct CatalogueInfo {
    pub messages: BTreeMap<String, MessageInfo>,
}

impl CatalogueInfo {
    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.messages.keys()
    }
}

/// Reports a finding when the directory name is not a valid locale tag.
pub fn validate_locale_tag(locale: &str, location: &str) -> Option<Finding> {
    match locale.parse::<LanguageIdentifier>() {
        Ok(_) => None,
        Err(_) => Some(Finding::new(
            Category::InvalidLocaleTag,
            location,
            format!("`{locale}` is not a valid locale identifier"),
        )),
    }
}

/// Reports the size and encoding findings of raw catalogue bytes.
///
/// Oversized or non-UTF-8 input fails closed: the caller stops before it trusts
/// the bytes, so malformed input never passes silently.
pub fn validate_bytes<'a>(bytes: &'a [u8], location: &str) -> Result<&'a str, Finding> {
    if bytes.len() > MAX_RESOURCE_BYTES {
        return Err(Finding::new(
            Category::ResourceTooLarge,
            location,
            format!(
                "catalogue is {} bytes, over the {MAX_RESOURCE_BYTES} byte limit",
                bytes.len()
            ),
        ));
    }

    core::str::from_utf8(bytes).map_err(|_| {
        Finding::new(
            Category::CatalogueEncoding,
            location,
            "catalogue is not valid UTF-8",
        )
    })
}

/// Parses one catalogue and reports its intrinsic problems.
///
/// The returned [`CatalogueInfo`] feeds the completeness check even when parsing
/// found errors, so a partial catalogue still reports its missing keys.
pub fn validate_resource(source: &str, location: &str) -> (CatalogueInfo, Vec<Finding>) {
    let mut findings = Vec::new();

    let resource = match parser::parse(source) {
        Ok(resource) => resource,
        Err((resource, errors)) => {
            for error in errors.iter().take(MAX_SYNTAX_FINDINGS) {
                let line = line_of(source, error.pos.start);
                findings.push(Finding::new(
                    Category::CatalogueSyntax,
                    format!("{location}:{line}"),
                    error.kind.to_string(),
                ));
            }
            resource
        }
    };

    let mut catalogue = CatalogueInfo::default();

    for entry in &resource.body {
        let Entry::Message(message) = entry else {
            continue;
        };
        let key = message.id.name.to_string();

        if catalogue.messages.contains_key(&key) {
            findings.push(Finding::new(
                Category::DuplicateKey,
                location,
                format!("message `{key}` is defined more than once"),
            ));
            continue;
        }

        let mut variables = BTreeSet::new();
        let mut has_content = false;

        if let Some(pattern) = &message.value {
            collect_pattern_variables(pattern, &mut variables);
            collect_selector_findings(pattern, &key, location, &mut findings);
            has_content = pattern_has_content(pattern);
        }

        for attribute in &message.attributes {
            collect_pattern_variables(&attribute.value, &mut variables);
            collect_selector_findings(&attribute.value, &key, location, &mut findings);
            has_content = has_content || pattern_has_content(&attribute.value);
        }

        if !has_content {
            findings.push(Finding::new(
                Category::EmptyTranslation,
                location,
                format!("message `{key}` has no text"),
            ));
        }

        catalogue.messages.insert(key, MessageInfo { variables });
    }

    (catalogue, findings)
}

/// Reports the completeness and argument problems of a non-reference locale.
pub fn check_completeness(
    reference: &CatalogueInfo,
    locale: &str,
    location: &str,
    candidate: &CatalogueInfo,
) -> Vec<Finding> {
    let mut findings = Vec::new();

    for (key, reference_message) in &reference.messages {
        match candidate.messages.get(key) {
            None => findings.push(Finding::new(
                Category::MissingKey,
                location,
                format!("locale `{locale}` is missing message `{key}`"),
            )),
            Some(candidate_message) => {
                if candidate_message.variables != reference_message.variables {
                    findings.push(Finding::new(
                        Category::ArgumentMismatch,
                        location,
                        format!(
                            "message `{key}` uses arguments {:?} but the reference uses {:?}",
                            candidate_message.variables, reference_message.variables
                        ),
                    ));
                }
            }
        }
    }

    for key in candidate.messages.keys() {
        if !reference.messages.contains_key(key) {
            findings.push(Finding::new(
                Category::ObsoleteKey,
                location,
                format!("locale `{locale}` defines message `{key}` that is not in the reference"),
            ));
        }
    }

    findings
}

/// Reports reference keys that no source references.
pub fn find_unused_keys(
    reference: &CatalogueInfo,
    location: &str,
    referenced_literals: &BTreeSet<String>,
) -> Vec<Finding> {
    reference
        .keys()
        .filter(|key| !referenced_literals.contains(*key))
        .map(|key| {
            Finding::new(
                Category::UnusedKey,
                location,
                format!("reference message `{key}` is not used by any source"),
            )
        })
        .collect()
}

fn collect_pattern_variables(pattern: &Pattern<&str>, variables: &mut BTreeSet<String>) {
    for element in &pattern.elements {
        if let PatternElement::Placeable { expression } = element {
            collect_expression_variables(expression, variables);
        }
    }
}

fn collect_expression_variables(expression: &Expression<&str>, variables: &mut BTreeSet<String>) {
    match expression {
        Expression::Inline(inline) => collect_inline_variables(inline, variables),
        Expression::Select { selector, variants } => {
            collect_inline_variables(selector, variables);
            for variant in variants {
                collect_pattern_variables(&variant.value, variables);
            }
        }
    }
}

fn collect_inline_variables(inline: &InlineExpression<&str>, variables: &mut BTreeSet<String>) {
    match inline {
        InlineExpression::VariableReference { id } => {
            variables.insert(id.name.to_string());
        }
        InlineExpression::Placeable { expression } => {
            collect_expression_variables(expression, variables);
        }
        InlineExpression::FunctionReference { arguments, .. } => {
            collect_call_variables(arguments, variables);
        }
        InlineExpression::TermReference {
            arguments: Some(arguments),
            ..
        } => {
            collect_call_variables(arguments, variables);
        }
        _ => {}
    }
}

fn collect_call_variables(arguments: &CallArguments<&str>, variables: &mut BTreeSet<String>) {
    for positional in &arguments.positional {
        collect_inline_variables(positional, variables);
    }
    for named in &arguments.named {
        collect_inline_variables(&named.value, variables);
    }
}

fn collect_selector_findings(
    pattern: &Pattern<&str>,
    key: &str,
    location: &str,
    findings: &mut Vec<Finding>,
) {
    for element in &pattern.elements {
        let PatternElement::Placeable { expression } = element else {
            continue;
        };
        if let Expression::Select { variants, .. } = expression {
            for variant in variants {
                if let VariantKey::Identifier { name } = &variant.key
                    && !CLDR_CATEGORIES.contains(name)
                {
                    findings.push(Finding::new(
                        Category::InvalidSelector,
                        location,
                        format!("message `{key}` uses unknown plural category `{name}`"),
                    ));
                }
                collect_selector_findings(&variant.value, key, location, findings);
            }
        }
    }
}

fn pattern_has_content(pattern: &Pattern<&str>) -> bool {
    pattern.elements.iter().any(|element| match element {
        PatternElement::Placeable { .. } => true,
        PatternElement::TextElement { value } => !value.trim().is_empty(),
    })
}

fn line_of(source: &str, byte_offset: usize) -> usize {
    let bounded = byte_offset.min(source.len());
    source[..bounded]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count()
        + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(source: &str) -> CatalogueInfo {
        validate_resource(source, "fixture.ftl").0
    }

    #[test]
    fn reports_a_missing_non_reference_key() {
        let reference = info("greeting = Hello\nfarewell = Goodbye\n");
        let candidate = info("greeting = Hola\n");
        let findings = check_completeness(&reference, "es", "es.ftl", &candidate);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].category(), Category::MissingKey);
    }

    #[test]
    fn reports_an_obsolete_non_reference_key() {
        let reference = info("greeting = Hello\n");
        let candidate = info("greeting = Hola\nextra = Extra\n");
        let findings = check_completeness(&reference, "es", "es.ftl", &candidate);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].category(), Category::ObsoleteKey);
    }

    #[test]
    fn reports_argument_mismatch() {
        let reference = info("welcome = Hello { $name }\n");
        let candidate = info("welcome = Hola\n");
        let findings = check_completeness(&reference, "es", "es.ftl", &candidate);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].category(), Category::ArgumentMismatch);
    }

    #[test]
    fn accepts_a_complete_locale() {
        let reference = info("welcome = Hello { $name }\n");
        let candidate = info("welcome = Hola { $name }\n");
        let findings = check_completeness(&reference, "es", "es.ftl", &candidate);
        assert!(findings.is_empty());
    }

    #[test]
    fn reports_invalid_syntax() {
        let (_, findings) = validate_resource("= not a valid message\n", "broken.ftl");
        assert!(
            findings
                .iter()
                .any(|finding| finding.category() == Category::CatalogueSyntax)
        );
    }

    #[test]
    fn reports_unknown_plural_category() {
        let source = "count =\n    { $n ->\n        [singular] one\n       *[other] many\n    }\n";
        let (_, findings) = validate_resource(source, "plural.ftl");
        assert!(
            findings
                .iter()
                .any(|finding| finding.category() == Category::InvalidSelector)
        );
    }

    #[test]
    fn accepts_valid_plural_categories() {
        let source = "count =\n    { $n ->\n        [one] one\n       *[other] many\n    }\n";
        let (_, findings) = validate_resource(source, "plural.ftl");
        assert!(findings.is_empty());
    }

    #[test]
    fn reports_an_unused_reference_key() {
        let reference = info("used = Used\nunused = Unused\n");
        let mut literals = BTreeSet::new();
        literals.insert("used".to_owned());
        let findings = find_unused_keys(&reference, "en.ftl", &literals);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].category(), Category::UnusedKey);
    }

    #[test]
    fn rejects_oversized_and_non_utf8_bytes() {
        let oversized = vec![b'a'; MAX_RESOURCE_BYTES + 1];
        assert!(validate_bytes(&oversized, "big.ftl").is_err());
        assert!(validate_bytes(&[0xff, 0xfe], "bad.ftl").is_err());
    }

    #[test]
    fn rejects_an_invalid_locale_tag() {
        assert!(validate_locale_tag("not a locale", "dir").is_some());
        assert!(validate_locale_tag("en", "dir").is_none());
        assert!(validate_locale_tag("ar", "dir").is_none());
    }
}
