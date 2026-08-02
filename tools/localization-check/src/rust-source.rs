// @file tools/localization-check/src/rust-source.rs
// @description Reads Rust sources and extracts their string literals in context.
// @created Diego Martín Lafuente <meerita@icloud.com>

use std::fs;
use std::path::{Path, PathBuf};

use crate::check_error::CheckError;

/// One string literal found in a Rust source, with the context a scan needs.
///
/// The context flags record where the literal sits so a scan can honor the
/// documented escape hatch: text in tests, in attributes (which carry canonical
/// error `Display` text), and text passed to a diagnostic or error construction
/// is not user-facing product prose.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StringLiteral {
    pub line: usize,
    pub content: String,
    pub in_test: bool,
    pub in_attribute: bool,
    pub call_context: Option<String>,
}

/// Extracts every double-quoted string literal from a Rust source.
///
/// The scan is a small character state machine. It skips line and block
/// comments so documentation text is never treated as a literal, skips character
/// literals so a quote-bearing character literal (for example `'"'` in a
/// tokenizer) never opens a false string, tracks the nearest enclosing call and
/// attribute, and follows the brace scope a `#[cfg(test)]` item opens so code
/// after a test module is never mistaken for test code. It does not aim to be a
/// full Rust parser; the workspace uses no raw strings in the scanned code, and
/// the scan stays sound for the constructs that are present.
pub fn extract_string_literals(source: &str) -> Vec<StringLiteral> {
    let chars: Vec<char> = source.chars().collect();

    let mut literals = Vec::new();
    let mut index = 0;
    let mut line = 1usize;
    let mut attribute_depth = 0usize;
    let mut call_stack: Vec<String> = Vec::new();
    let mut word = String::new();
    let mut brace_depth = 0usize;
    let mut test_scopes: Vec<usize> = Vec::new();
    let mut pending_test = false;

    while index < chars.len() {
        let current = chars[index];

        if current == '\n' {
            line += 1;
            word.clear();
            index += 1;
            continue;
        }

        if current == '/' && chars.get(index + 1) == Some(&'/') {
            index += 2;
            while index < chars.len() && chars[index] != '\n' {
                index += 1;
            }
            word.clear();
            continue;
        }

        if current == '/' && chars.get(index + 1) == Some(&'*') {
            index += 2;
            let mut depth = 1usize;
            while index < chars.len() && depth > 0 {
                if chars[index] == '\n' {
                    line += 1;
                } else if chars[index] == '/' && chars.get(index + 1) == Some(&'*') {
                    depth += 1;
                    index += 1;
                } else if chars[index] == '*' && chars.get(index + 1) == Some(&'/') {
                    depth -= 1;
                    index += 1;
                }
                index += 1;
            }
            word.clear();
            continue;
        }

        if current == '#' {
            let mut look = index + 1;
            if chars.get(look) == Some(&'!') {
                look += 1;
            }
            if chars.get(look) == Some(&'[') {
                if attribute_is_test_gate(&chars, look) {
                    pending_test = true;
                }
                attribute_depth += 1;
                word.clear();
                index = look + 1;
                continue;
            }
            word.clear();
            index += 1;
            continue;
        }

        if current == '{' {
            if pending_test {
                test_scopes.push(brace_depth);
                pending_test = false;
            }
            brace_depth += 1;
            word.clear();
            index += 1;
            continue;
        }

        if current == '}' {
            brace_depth = brace_depth.saturating_sub(1);
            if test_scopes.last() == Some(&brace_depth) {
                test_scopes.pop();
            }
            word.clear();
            index += 1;
            continue;
        }

        if current == ';' {
            pending_test = false;
            word.clear();
            index += 1;
            continue;
        }

        if current == '[' {
            if attribute_depth > 0 {
                attribute_depth += 1;
            }
            word.clear();
            index += 1;
            continue;
        }

        if current == ']' {
            attribute_depth = attribute_depth.saturating_sub(1);
            word.clear();
            index += 1;
            continue;
        }

        if current == '"' {
            let start_line = line;
            let mut content = String::new();
            index += 1;
            while index < chars.len() {
                let inner = chars[index];
                if inner == '\\' {
                    if let Some(&escaped) = chars.get(index + 1) {
                        content.push(escaped);
                        if escaped == '\n' {
                            line += 1;
                        }
                    }
                    index += 2;
                    continue;
                }
                if inner == '"' {
                    index += 1;
                    break;
                }
                if inner == '\n' {
                    line += 1;
                }
                content.push(inner);
                index += 1;
            }
            literals.push(StringLiteral {
                line: start_line,
                content,
                in_test: !test_scopes.is_empty(),
                in_attribute: attribute_depth > 0,
                call_context: call_stack.last().cloned(),
            });
            word.clear();
            continue;
        }

        if current == '\'' {
            if let Some(next) = skip_character_literal(&chars, index) {
                index = next;
                word.clear();
                continue;
            }
            word.clear();
            index += 1;
            continue;
        }

        if is_word_char(current) {
            word.push(current);
            index += 1;
            continue;
        }

        if current == '(' {
            call_stack.push(word.clone());
            word.clear();
            index += 1;
            continue;
        }

        if current == ')' {
            call_stack.pop();
            word.clear();
            index += 1;
            continue;
        }

        word.clear();
        index += 1;
    }

    literals
}

/// Returns the source with comment and string contents blanked.
///
/// Marker detection runs on this skeleton so a word inside a comment (for
/// example a doc comment that mentions serialization) never counts as real
/// serialization code.
pub fn code_skeleton(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let mut skeleton = String::with_capacity(chars.len());
    let mut index = 0;

    while index < chars.len() {
        let current = chars[index];

        if current == '/' && chars.get(index + 1) == Some(&'/') {
            index += 2;
            while index < chars.len() && chars[index] != '\n' {
                index += 1;
            }
            continue;
        }

        if current == '/' && chars.get(index + 1) == Some(&'*') {
            index += 2;
            let mut depth = 1usize;
            while index < chars.len() && depth > 0 {
                if chars[index] == '/' && chars.get(index + 1) == Some(&'*') {
                    depth += 1;
                    index += 1;
                } else if chars[index] == '*' && chars.get(index + 1) == Some(&'/') {
                    depth -= 1;
                    index += 1;
                }
                index += 1;
            }
            continue;
        }

        if current == '"' {
            skeleton.push('"');
            index += 1;
            while index < chars.len() {
                if chars[index] == '\\' {
                    index += 2;
                    continue;
                }
                if chars[index] == '"' {
                    break;
                }
                index += 1;
            }
            skeleton.push('"');
            index += 1;
            continue;
        }

        skeleton.push(current);
        index += 1;
    }

    skeleton
}

/// Lists the Rust sources under a directory.
///
/// The walk skips the build directory and any hidden directory so it never
/// descends into generated or version-control state.
pub fn rust_files_under(root: &Path) -> Result<Vec<PathBuf>, CheckError> {
    let mut files = Vec::new();
    collect_rust_files(root, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_rust_files(directory: &Path, files: &mut Vec<PathBuf>) -> Result<(), CheckError> {
    if !directory.is_dir() {
        return Ok(());
    }

    let entries = fs::read_dir(directory).map_err(|source| CheckError::ReadDir {
        path: directory.to_path_buf(),
        source,
    })?;

    for entry in entries {
        let entry = entry.map_err(|source| CheckError::ReadDir {
            path: directory.to_path_buf(),
            source,
        })?;
        let path = entry.path();

        if path.is_dir() {
            if is_skipped_directory(&path) {
                continue;
            }
            collect_rust_files(&path, files)?;
            continue;
        }

        if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }

    Ok(())
}

pub fn read_source(path: &Path) -> Result<String, CheckError> {
    fs::read_to_string(path).map_err(|source| CheckError::ReadFile {
        path: path.to_path_buf(),
        source,
    })
}

fn is_skipped_directory(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == "target" || name.starts_with('.'))
}

/// Returns the index past a Rust character literal that opens at `open`.
///
/// A quote can open a character literal (`'x'`, `'\n'`, `'\''`, `'"'`,
/// `'\u{1F600}'`) or a lifetime (`'a`, `'static`). Only a character literal is
/// skipped, so a quote-bearing character literal never opens a false string. A
/// lifetime, or a malformed literal, returns `None` and is treated as a
/// separator.
fn skip_character_literal(chars: &[char], open: usize) -> Option<usize> {
    if chars.get(open) != Some(&'\'') {
        return None;
    }

    match chars.get(open + 1) {
        Some('\\') => {
            let mut index = open + 2;
            if chars.get(index) == Some(&'u') {
                while index < chars.len() && chars[index] != '}' {
                    index += 1;
                }
                index += 1;
            } else {
                index += 1;
            }
            match chars.get(index) {
                Some('\'') => Some(index + 1),
                _ => None,
            }
        }
        Some(_) if chars.get(open + 2) == Some(&'\'') => Some(open + 3),
        _ => None,
    }
}

fn is_word_char(character: char) -> bool {
    character.is_ascii_alphanumeric()
        || character == '_'
        || character == ':'
        || character == '.'
        || character == '!'
}

fn attribute_is_test_gate(chars: &[char], open_bracket: usize) -> bool {
    let mut depth = 0usize;
    let mut index = open_bracket;
    let mut inner = String::new();

    while index < chars.len() {
        match chars[index] {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            other if depth >= 1 => inner.push(other),
            _ => {}
        }
        index += 1;
    }

    let normalized: String = inner
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect();
    normalized.contains("cfg(test)")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skip(source: &str) -> Option<usize> {
        let chars: Vec<char> = source.chars().collect();
        skip_character_literal(&chars, 0)
    }

    #[test]
    fn skips_simple_and_quote_bearing_character_literals() {
        assert_eq!(skip("'a'"), Some(3));
        assert_eq!(skip("'\"'"), Some(3));
        assert_eq!(skip("'\\''"), Some(4));
        assert_eq!(skip("'\\n'"), Some(4));
    }

    #[test]
    fn skips_a_unicode_escape_character_literal() {
        assert_eq!(skip("'\\u{1F600}'"), Some(11));
    }

    #[test]
    fn treats_a_lifetime_as_a_separator() {
        assert_eq!(skip("'static"), None);
        assert_eq!(skip("'a "), None);
    }
}
