// @file foundation/capability-system/src/catalogue.rs
// @description Defines the immutable, validated capability catalogue.
// @created Diego Martín Lafuente <meerita@icloud.com>

use std::collections::HashMap;

use crate::{CapabilityDefinition, CapabilityId};

/// Immutable collection of validated capability definitions.
///
/// A catalogue is built once by [`crate::CatalogueBuilder`] and read many times.
/// It exposes no mutation API: every definition it holds passed validation at
/// construction and stays fixed for the lifetime of the catalogue. Definitions
/// keep their insertion order so reports are deterministic, and an identifier
/// index provides cheap lookup.
#[derive(Debug)]
pub struct Catalogue {
    definitions: Vec<CapabilityDefinition>,
    index: HashMap<CapabilityId, usize>,
}

impl Catalogue {
    pub(crate) fn new(
        definitions: Vec<CapabilityDefinition>,
        index: HashMap<CapabilityId, usize>,
    ) -> Self {
        Self { definitions, index }
    }

    /// Returns the definition for an identifier, or `None` when the catalogue
    /// does not hold it.
    pub fn get(&self, id: CapabilityId) -> Option<&CapabilityDefinition> {
        self.index
            .get(&id)
            .map(|&position| &self.definitions[position])
    }

    pub fn contains(&self, id: CapabilityId) -> bool {
        self.index.contains_key(&id)
    }

    /// Returns every definition in insertion order.
    pub fn definitions(&self) -> &[CapabilityDefinition] {
        &self.definitions
    }
}
