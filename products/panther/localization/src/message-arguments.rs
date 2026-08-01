// @file products/panther/localization/src/message-arguments.rs
// @description Carries the named arguments an adapter supplies to a message.
// @created Diego Martín Lafuente <meerita@icloud.com>

/// A single named argument value supplied to a message.
///
/// The set is intentionally small. An adapter supplies structured data, never
/// pre-formatted prose, so the message template controls all wording. Number
/// and date formatting is applied later against the resolved locale.
#[derive(Clone, PartialEq, Debug)]
pub enum MessageArgument {
    Text(String),
    Integer(i64),
}

/// The named arguments an adapter supplies for a message.
///
/// Arguments are supplied as data so that the presentation boundary, not the
/// low-level state, decides wording and formatting. The names match the
/// placeholders in the message template.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct MessageArguments {
    entries: Vec<(&'static str, MessageArgument)>,
}

impl MessageArguments {
    /// Creates an empty argument set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a named argument and returns the set, so calls can chain.
    pub fn with(mut self, name: &'static str, value: MessageArgument) -> Self {
        self.entries.push((name, value));
        self
    }

    /// Returns the named arguments in insertion order.
    pub fn entries(&self) -> &[(&'static str, MessageArgument)] {
        &self.entries
    }
}
