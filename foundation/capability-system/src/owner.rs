// @file foundation/capability-system/src/owner.rs
// @description Defines the capability owner and its namespace mapping.
// @created Diego Martín Lafuente <meerita@icloud.com>

/// Owner of a capability.
///
/// Each capability belongs to exactly one owner. The owner is derived from the
/// identifier namespace and validated when a catalogue is built.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Owner {
    Panther,
    Purr,
}

impl Owner {
    /// Maps an owner namespace to its owner, or `None` when the namespace is
    /// not recognised.
    pub fn from_namespace(namespace: &str) -> Option<Self> {
        match namespace {
            "panther" => Some(Self::Panther),
            "purr" => Some(Self::Purr),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Owner;

    #[test]
    fn known_namespaces_map_to_owners() {
        assert_eq!(Owner::from_namespace("panther"), Some(Owner::Panther));
        assert_eq!(Owner::from_namespace("purr"), Some(Owner::Purr));
    }

    #[test]
    fn unknown_namespace_maps_to_none() {
        assert_eq!(Owner::from_namespace("chrome"), None);
    }
}
