// @file engines/purr/engine/src/css-tokenizer.rs
// @description Defines the minimal CSS tokenizer for the M2 style pipeline.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Minimal CSS tokenizer.
//!
//! The tokenizer turns stylesheet text into the token set the M2 parser needs:
//! identifiers, hashes, delimiters, braces, a colon, a semicolon, a comma,
//! strings, numbers, dimensions, and percentages. It is an untrusted-input
//! parser, so it runs iteratively with a bounded cursor and never recurses. Each
//! step advances the cursor by at least one code point, so tokenization always
//! terminates, even on truncated input such as an unclosed comment or string.
//!
//! Whitespace is a token separator only and is not emitted; comments are skipped.
//! Numeric text is kept as a string, so the tokenizer performs no floating-point
//! conversion and the parser stays free of exact float comparison.

// The parser is the first consumer of these tokens. This module is exercised
// through the parser and its tests, so some items are otherwise unused in a
// non-test build.
#![allow(dead_code)]

/// One CSS token from the M2 subset.
///
/// The set is closed for the subset. A `Hash` carries the name after `#`, used
/// for an id selector or a color. A `Dimension` keeps its numeric text and unit
/// separate; a `Number` and a `Percentage` keep their numeric text. Numeric text
/// stays a string so no floating-point conversion happens at this stage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CssToken {
    Ident(String),
    Hash(String),
    Delim(char),
    Number(String),
    Dimension { value: String, unit: String },
    Percentage(String),
    StringToken(String),
    Colon,
    Semicolon,
    Comma,
    LeftBrace,
    RightBrace,
    Eof,
}

/// Tokenizes stylesheet text into the M2 token set.
///
/// The input is bounded by the document source cap, so the token vector is
/// bounded. Whitespace separates tokens and is not emitted; comments are
/// skipped. An unterminated comment or string consumes to the end of the input
/// without a panic.
pub fn tokenize(source: &str) -> Vec<CssToken> {
    let input: Vec<char> = source.chars().collect();
    let mut cursor = Cursor { input, position: 0 };
    let mut tokens = Vec::new();
    while let Some(token) = cursor.next_token() {
        tokens.push(token);
    }
    tokens
}

struct Cursor {
    input: Vec<char>,
    position: usize,
}

impl Cursor {
    fn next_token(&mut self) -> Option<CssToken> {
        self.skip_trivia();
        let current = self.peek()?;
        let token = match current {
            '{' => self.single(CssToken::LeftBrace),
            '}' => self.single(CssToken::RightBrace),
            ':' => self.single(CssToken::Colon),
            ';' => self.single(CssToken::Semicolon),
            ',' => self.single(CssToken::Comma),
            '"' | '\'' => self.consume_string(current),
            '#' => self.consume_hash(),
            '.' if self.next_is_digit() => self.consume_numeric(),
            '.' => self.single(CssToken::Delim('.')),
            '+' if self.next_is_numeric_start() => self.consume_numeric(),
            '-' if self.next_is_numeric_start() => self.consume_numeric(),
            '-' if self.next_is_ident_start() => self.consume_ident(),
            digit if digit.is_ascii_digit() => self.consume_numeric(),
            start if is_ident_start(start) => self.consume_ident(),
            other => self.single(CssToken::Delim(other)),
        };
        Some(token)
    }

    /// Consumes one code point and returns the given token.
    fn single(&mut self, token: CssToken) -> CssToken {
        self.advance();
        token
    }

    fn consume_ident(&mut self) -> CssToken {
        CssToken::Ident(self.take_ident())
    }

    fn consume_hash(&mut self) -> CssToken {
        self.advance();
        CssToken::Hash(self.take_ident())
    }

    fn consume_string(&mut self, quote: char) -> CssToken {
        self.advance();
        let mut value = String::new();
        while let Some(current) = self.peek() {
            if current == quote {
                self.advance();
                break;
            }
            value.push(current);
            self.advance();
        }
        CssToken::StringToken(value)
    }

    fn consume_numeric(&mut self) -> CssToken {
        let value = self.take_number();
        match self.peek() {
            Some('%') => {
                self.advance();
                CssToken::Percentage(value)
            }
            Some(unit_start) if is_ident_start(unit_start) => {
                let unit = self.take_ident();
                CssToken::Dimension { value, unit }
            }
            _ => CssToken::Number(value),
        }
    }

    fn take_number(&mut self) -> String {
        let mut value = String::new();
        if matches!(self.peek(), Some('+' | '-')) {
            if let Some(sign) = self.peek() {
                value.push(sign);
            }
            self.advance();
        }
        self.take_digits(&mut value);
        if matches!(self.peek(), Some('.')) {
            value.push('.');
            self.advance();
            self.take_digits(&mut value);
        }
        value
    }

    fn take_digits(&mut self, value: &mut String) {
        while let Some(digit) = self.peek() {
            if !digit.is_ascii_digit() {
                break;
            }
            value.push(digit);
            self.advance();
        }
    }

    fn take_ident(&mut self) -> String {
        let mut name = String::new();
        while let Some(current) = self.peek() {
            if !is_ident_char(current) {
                break;
            }
            name.push(current);
            self.advance();
        }
        name
    }

    /// Skips whitespace and comments before the next token.
    fn skip_trivia(&mut self) {
        loop {
            match self.peek() {
                Some(current) if current.is_whitespace() => self.advance(),
                Some('/') if self.peek_at(1) == Some('*') => self.skip_comment(),
                _ => break,
            }
        }
    }

    fn skip_comment(&mut self) {
        self.advance();
        self.advance();
        while let Some(current) = self.peek() {
            if current == '*' && self.peek_at(1) == Some('/') {
                self.advance();
                self.advance();
                break;
            }
            self.advance();
        }
    }

    fn next_is_digit(&self) -> bool {
        self.peek_at(1).is_some_and(|c| c.is_ascii_digit())
    }

    fn next_is_numeric_start(&self) -> bool {
        match self.peek_at(1) {
            Some(c) if c.is_ascii_digit() => true,
            Some('.') => self.peek_at(2).is_some_and(|c| c.is_ascii_digit()),
            _ => false,
        }
    }

    fn next_is_ident_start(&self) -> bool {
        self.peek_at(1).is_some_and(is_ident_start)
    }

    fn peek(&self) -> Option<char> {
        self.input.get(self.position).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<char> {
        self.position
            .checked_add(offset)
            .and_then(|index| self.input.get(index))
            .copied()
    }

    fn advance(&mut self) {
        self.position = self.position.saturating_add(1);
    }
}

fn is_ident_start(value: char) -> bool {
    value.is_ascii_alphabetic() || value == '_' || value == '-' || !value.is_ascii()
}

fn is_ident_char(value: char) -> bool {
    value.is_ascii_alphanumeric() || value == '_' || value == '-' || !value.is_ascii()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizes_a_class_rule() {
        let tokens = tokenize(".card { color: #eee; }");

        assert_eq!(
            tokens,
            vec![
                CssToken::Delim('.'),
                CssToken::Ident("card".to_owned()),
                CssToken::LeftBrace,
                CssToken::Ident("color".to_owned()),
                CssToken::Colon,
                CssToken::Hash("eee".to_owned()),
                CssToken::Semicolon,
                CssToken::RightBrace,
            ]
        );
    }

    #[test]
    fn tokenizes_a_dimension_and_a_percentage() {
        let tokens = tokenize("8px 50%");

        assert_eq!(
            tokens,
            vec![
                CssToken::Dimension {
                    value: "8".to_owned(),
                    unit: "px".to_owned(),
                },
                CssToken::Percentage("50".to_owned()),
            ]
        );
    }

    #[test]
    fn a_leading_dot_digit_is_a_number_not_a_class() {
        let tokens = tokenize(".5");

        assert_eq!(tokens, vec![CssToken::Number(".5".to_owned())]);
    }

    #[test]
    fn an_unterminated_comment_consumes_to_the_end() {
        let tokens = tokenize("a /* open");

        assert_eq!(tokens, vec![CssToken::Ident("a".to_owned())]);
    }
}
