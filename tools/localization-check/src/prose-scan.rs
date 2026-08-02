// @file tools/localization-check/src/prose-scan.rs
// @description Flags user-facing prose literals in foundation and engine crates.
// @created Diego Martín Lafuente <meerita@icloud.com>

use std::path::Path;

use crate::check_error::CheckError;
use crate::finding::{Category, Finding};
use crate::rust_source::{
    StringLiteral, code_skeleton, extract_string_literals, read_source, rust_files_under,
};

/// Minimum count of space-separated word tokens that marks a literal as prose.
///
/// The removed `reason_message` returned whole English sentences. A threshold of
/// three words catches that class of leak while leaving identifiers, stable
/// codes, locale tags, and short labels alone.
const PROSE_WORD_THRESHOLD: usize = 3;

/// Code tokens that mark a file as a serialization or foreign-function boundary.
///
/// Detection runs on the comment-free code skeleton, so a doc comment that only
/// mentions serialization does not match.
const IPC_MARKERS: [&str; 8] = [
    "Serialize",
    "Deserialize",
    "serde",
    "bincode",
    "rkyv",
    "prost",
    "#[repr(C)]",
    "extern \"C\"",
];

/// Scans the given roots for user-facing prose literals.
///
/// Invariant 1 keeps foundation and engine crates prose-free: only typed states,
/// stable codes, and structured arguments leave them. This scan flags any string
/// literal that reads as a sentence, except the documented escape hatch of
/// tests, canonical error text, logs, and diagnostic messages.
pub fn scan_prose(roots: &[&Path]) -> Result<Vec<Finding>, CheckError> {
    let mut findings = Vec::new();

    for root in roots {
        for file in rust_files_under(root)? {
            let source = read_source(&file)?;
            let location_base = file.display().to_string();
            for literal in extract_string_literals(&source) {
                if is_escape_hatch(&literal) || !is_prose(&literal.content) {
                    continue;
                }
                findings.push(Finding::new(
                    Category::ProseLiteral,
                    format!("{location_base}:{}", literal.line),
                    format!("user-facing prose literal: {:?}", literal.content),
                ));
            }
        }
    }

    Ok(findings)
}

/// Scans boundary files for prose that would cross a process boundary.
///
/// The Panther and Purr boundary carries typed states, stable codes, and
/// structured arguments only; it never transports prose as a contract
/// (Invariant 8). Today the split is in-process, so no serialized boundary type
/// exists and the general prose scan already covers these crates. This scan
/// activates a stricter rule for any file that gains a serialization or
/// foreign-function boundary: there prose is flagged even in the error and
/// diagnostic positions the general scan allows, because serialized contract
/// data must stay language-neutral.
pub fn scan_ipc_prose(roots: &[&Path]) -> Result<Vec<Finding>, CheckError> {
    let mut findings = Vec::new();

    for root in roots {
        for file in rust_files_under(root)? {
            let source = read_source(&file)?;
            if !is_boundary_file(&source) {
                continue;
            }
            let location_base = file.display().to_string();
            for literal in extract_string_literals(&source) {
                if literal.in_test || !is_prose(&literal.content) {
                    continue;
                }
                findings.push(Finding::new(
                    Category::IpcProse,
                    format!("{location_base}:{}", literal.line),
                    format!("prose on a serialization boundary: {:?}", literal.content),
                ));
            }
        }
    }

    Ok(findings)
}

fn is_boundary_file(source: &str) -> bool {
    let skeleton = code_skeleton(source);
    IPC_MARKERS.iter().any(|marker| skeleton.contains(marker))
}

fn is_escape_hatch(literal: &StringLiteral) -> bool {
    if literal.in_test || literal.in_attribute {
        return true;
    }

    match literal.call_context.as_deref() {
        Some(context) => is_allowed_call(context),
        None => false,
    }
}

fn is_allowed_call(context: &str) -> bool {
    let segments: Vec<&str> = context
        .split([':', '.'])
        .filter(|part| !part.is_empty())
        .collect();
    let Some(last) = segments.last() else {
        return false;
    };

    if is_error_construction(&segments) {
        return true;
    }

    let macro_name = last.strip_suffix('!');
    if let Some(name) = macro_name {
        return is_diagnostic_macro(name);
    }

    is_diagnostic_method(last)
}

fn is_error_construction(segments: &[&str]) -> bool {
    segments
        .iter()
        .any(|segment| segment.ends_with("Error") || segment.ends_with("Failure"))
}

fn is_diagnostic_macro(name: &str) -> bool {
    matches!(
        name,
        "format"
            | "write"
            | "writeln"
            | "print"
            | "println"
            | "eprint"
            | "eprintln"
            | "panic"
            | "assert"
            | "assert_eq"
            | "assert_ne"
            | "debug_assert"
            | "debug_assert_eq"
            | "debug_assert_ne"
            | "unreachable"
            | "todo"
            | "unimplemented"
            | "vec"
            | "matches"
            | "trace"
            | "debug"
            | "info"
            | "warn"
            | "error"
    )
}

fn is_diagnostic_method(name: &str) -> bool {
    matches!(
        name,
        "expect"
            | "expect_err"
            | "unwrap_err"
            | "unwrap_or"
            | "unwrap_or_else"
            | "ok_or"
            | "ok_or_else"
            | "context"
            | "with_context"
    )
}

/// Reports whether a literal reads as a natural-language sentence.
///
/// A prose literal has at least [`PROSE_WORD_THRESHOLD`] space-separated word
/// tokens. A token counts as a word when it holds at least two ASCII letters and
/// carries no identifier punctuation, so codes, dotted paths, and kebab or snake
/// identifiers do not register as words. Structured code or markup is excluded
/// first, so an embedded stylesheet or code block never counts.
fn is_prose(content: &str) -> bool {
    if is_structured_data(content) {
        return false;
    }

    content
        .split_whitespace()
        .filter(|token| is_word_token(token))
        .count()
        >= PROSE_WORD_THRESHOLD
}

/// Reports whether a literal is structured code or markup, not natural language.
///
/// An embedded stylesheet or code block carries brace-delimited declarations
/// (`selector { property: value; }`). That is engine data, not user-facing prose,
/// and a natural-language message never carries this declaration structure. The
/// rule requires all of a block open, a block close, a declaration separator, and
/// a name-to-value separator, so an ordinary sentence never qualifies.
fn is_structured_data(content: &str) -> bool {
    content.contains('{') && content.contains('}') && content.contains(';') && content.contains(':')
}

fn is_word_token(token: &str) -> bool {
    let letters = token
        .chars()
        .filter(|character| character.is_ascii_alphabetic())
        .count();
    let has_identifier_mark = token
        .chars()
        .any(|character| character == '_' || character == ':');
    letters >= 2 && !has_identifier_mark
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_a_prose_return_in_production_code() {
        let source = r#"
            pub fn reason_message() -> &'static str {
                "The capability is not included in this build."
            }
        "#;
        let literals = extract_string_literals(source);
        assert_eq!(literals.len(), 1);
        assert!(is_prose(&literals[0].content));
        assert!(!is_escape_hatch(&literals[0]));
    }

    #[test]
    fn allows_canonical_error_display_in_attributes() {
        let source = r#"
            #[error("the locale identifier is not a valid Unicode locale identifier")]
            pub struct LocaleParseError;
        "#;
        let literals = extract_string_literals(source);
        assert_eq!(literals.len(), 1);
        assert!(is_escape_hatch(&literals[0]));
    }

    #[test]
    fn allows_diagnostic_text_in_error_construction() {
        let source = r#"
            fn translate() -> ActivationFailure {
                ActivationFailure::new(
                    FailureCategory::ActivationError,
                    "WebGPU backend is not available",
                )
            }
        "#;
        let literals = extract_string_literals(source);
        let diagnostic = literals
            .iter()
            .find(|literal| literal.content.contains("WebGPU"))
            .expect("the diagnostic literal is extracted");
        assert!(is_escape_hatch(diagnostic));
    }

    #[test]
    fn allows_expect_messages_and_identifiers() {
        assert!(!is_prose("es-AR"));
        assert!(!is_prose("purr.author-styles"));
        assert!(!is_prose("CAP_REASON_NOT_COMPILED_IN"));
        assert!(!is_prose("New Tab"));
    }

    #[test]
    fn ignores_prose_inside_tests() {
        let source = concat!(
            "pub fn value() -> u8 { 0 }\n",
            "#[cfg(test)]\n",
            "mod tests {\n",
            "    #[test]\n",
            "    fn checks() {\n",
            "        assert_eq!(value(), 0, \"the value should be zero here\");\n",
            "    }\n",
            "}\n",
        );
        let literals = extract_string_literals(source);
        let inside = literals
            .iter()
            .find(|literal| literal.content.contains("should be zero"))
            .expect("the assert literal is extracted");
        assert!(inside.in_test);
        assert!(is_escape_hatch(inside));
    }

    #[test]
    fn flags_prose_after_a_test_module() {
        let source = concat!(
            "#[cfg(test)]\n",
            "mod tests {\n",
            "    #[test]\n",
            "    fn checks() {\n",
            "        assert_eq!(1, 1, \"these two values should be equal\");\n",
            "    }\n",
            "}\n",
            "pub fn leaked() -> &'static str {\n",
            "    \"The capability is not included in this build.\"\n",
            "}\n",
        );
        let literals = extract_string_literals(source);
        let leaked = literals
            .iter()
            .find(|literal| literal.content.contains("not included"))
            .expect("the leaked literal is extracted");
        assert!(!leaked.in_test);
        assert!(!is_escape_hatch(leaked));
        assert!(is_prose(&leaked.content));
    }

    #[test]
    fn excludes_an_embedded_stylesheet_from_prose() {
        let sheet = "\nbody { display: block; margin: 8px; }\np { display: block; }\n";
        assert!(is_structured_data(sheet));
        assert!(!is_prose(sheet));
    }

    #[test]
    fn a_sentence_with_a_colon_is_still_prose() {
        let sentence = "The capability is not included: rebuild the project.";
        assert!(!is_structured_data(sentence));
        assert!(is_prose(sentence));
    }

    #[test]
    fn a_quote_bearing_character_literal_does_not_open_a_string() {
        let source = concat!(
            "fn classify(current: char) {\n",
            "    match current {\n",
            "        '\"' => start_string(),\n",
            "        '\\'' => start_char(),\n",
            "        _ => {}\n",
            "    }\n",
            "}\n",
            "pub fn leaked() -> &'static str {\n",
            "    \"The capability is not included in this build.\"\n",
            "}\n",
        );
        let literals = extract_string_literals(source);
        assert_eq!(literals.len(), 1);
        assert!(literals[0].content.contains("not included"));
    }

    #[test]
    fn detects_serialization_boundary_only_in_code() {
        let commented = "/// A future split can serialize this snapshot.\npub struct Snapshot;\n";
        assert!(!is_boundary_file(commented));

        let coded = "#[derive(Serialize)]\npub struct Snapshot;\n";
        assert!(is_boundary_file(coded));
    }
}
