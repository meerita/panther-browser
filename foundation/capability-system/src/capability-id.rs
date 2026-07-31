// @file foundation/capability-system/src/capability-id.rs
// @description Defines the capability identifier newtype.
// @created Diego Martín Lafuente <meerita@icloud.com>

/// Stable identifier for one capability.
///
/// The inner text is a namespaced identifier such as `purr.author-styles`. The
/// text before the first `.` is the owner namespace. Construction is `const`
/// because identifiers are declared locally as constants and never built from
/// owned or remote data.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct CapabilityId(&'static str);

impl CapabilityId {
    pub const fn new(id: &'static str) -> Self {
        Self(id)
    }

    pub const fn as_str(&self) -> &'static str {
        self.0
    }

    /// Returns the text before the first `.`, or the whole identifier when no
    /// `.` is present.
    pub fn owner_namespace(&self) -> &'static str {
        match self.0.split_once('.') {
            Some((namespace, _)) => namespace,
            None => self.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CapabilityId;

    #[test]
    fn owner_namespace_returns_text_before_first_dot() {
        let id = CapabilityId::new("purr.author-styles");
        assert_eq!(id.owner_namespace(), "purr");
    }

    #[test]
    fn owner_namespace_without_dot_returns_whole_identifier() {
        let id = CapabilityId::new("standalone");
        assert_eq!(id.owner_namespace(), "standalone");
    }

    #[test]
    fn owner_namespace_uses_only_the_first_dot() {
        let id = CapabilityId::new("panther.privacy.tracker-blocking");
        assert_eq!(id.owner_namespace(), "panther");
    }
}
