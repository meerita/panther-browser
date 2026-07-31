// @file foundation/capability-system/src/catalogue-builder.rs
// @description Builds and validates an immutable capability catalogue.
// @created Diego Martín Lafuente <meerita@icloud.com>

use std::collections::{HashMap, VecDeque};

use crate::{CapabilityDefinition, CapabilityId, Catalogue, CatalogueBuildError, Owner};

/// Collects capability definitions and validates them in one place.
///
/// The builder is the single construction point for a [`Catalogue`]. Every
/// declaration flows through [`CatalogueBuilder::build`], which either produces
/// an immutable catalogue or rejects the whole set with the first violation it
/// finds. Adding a definition never validates; validation runs once at build.
#[derive(Debug, Default)]
pub struct CatalogueBuilder {
    definitions: Vec<CapabilityDefinition>,
}

impl CatalogueBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, definition: CapabilityDefinition) -> &mut Self {
        self.definitions.push(definition);
        self
    }

    pub fn extend(
        &mut self,
        definitions: impl IntoIterator<Item = CapabilityDefinition>,
    ) -> &mut Self {
        self.definitions.extend(definitions);
        self
    }

    /// Validates the collected definitions and produces an immutable catalogue.
    ///
    /// Rejects duplicate identifiers, an identifier whose namespace does not
    /// match its owner, a dependency on an identifier that is not present, and a
    /// dependency cycle. Returns the first violation and never a partial result.
    pub fn build(self) -> Result<Catalogue, CatalogueBuildError> {
        let definitions = self.definitions;

        let index = build_index(&definitions)?;
        validate_namespaces(&definitions)?;
        let adjacency = build_dependency_graph(&definitions, &index)?;
        validate_acyclic(&definitions, &adjacency)?;

        Ok(Catalogue::new(definitions, index))
    }
}

fn build_index(
    definitions: &[CapabilityDefinition],
) -> Result<HashMap<CapabilityId, usize>, CatalogueBuildError> {
    let mut index = HashMap::with_capacity(definitions.len());
    for (position, definition) in definitions.iter().enumerate() {
        if index.insert(definition.id, position).is_some() {
            return Err(CatalogueBuildError::DuplicateId(definition.id));
        }
    }
    Ok(index)
}

fn validate_namespaces(definitions: &[CapabilityDefinition]) -> Result<(), CatalogueBuildError> {
    for definition in definitions {
        let namespace = definition.id.owner_namespace();
        if Owner::from_namespace(namespace) != Some(definition.owner) {
            return Err(CatalogueBuildError::NamespaceOwnerMismatch(definition.id));
        }
    }
    Ok(())
}

/// Builds the dependency adjacency list by definition position and rejects a
/// dependency on an identifier that the catalogue does not hold.
fn build_dependency_graph(
    definitions: &[CapabilityDefinition],
    index: &HashMap<CapabilityId, usize>,
) -> Result<Vec<Vec<usize>>, CatalogueBuildError> {
    let mut adjacency = vec![Vec::new(); definitions.len()];
    for (position, definition) in definitions.iter().enumerate() {
        for dependency in definition.dependencies {
            let Some(&target) = index.get(dependency) else {
                return Err(CatalogueBuildError::UnknownDependency {
                    dependent: definition.id,
                    dependency: *dependency,
                });
            };
            adjacency[position].push(target);
        }
    }
    Ok(adjacency)
}

/// Rejects a dependency cycle with a Kahn topological pass in O(V + E). A node
/// that still has incoming edges after the pass belongs to a cycle. A
/// self-dependency is a one-node cycle and keeps its own incoming edge.
fn validate_acyclic(
    definitions: &[CapabilityDefinition],
    adjacency: &[Vec<usize>],
) -> Result<(), CatalogueBuildError> {
    let node_count = adjacency.len();

    let mut in_degree = vec![0usize; node_count];
    for targets in adjacency {
        for &target in targets {
            in_degree[target] += 1;
        }
    }

    let mut queue: VecDeque<usize> = (0..node_count)
        .filter(|&position| in_degree[position] == 0)
        .collect();

    while let Some(position) = queue.pop_front() {
        for &target in &adjacency[position] {
            in_degree[target] -= 1;
            if in_degree[target] == 0 {
                queue.push_back(target);
            }
        }
    }

    match in_degree.iter().position(|&degree| degree > 0) {
        Some(position) => Err(CatalogueBuildError::DependencyCycle(
            definitions[position].id,
        )),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::CatalogueBuilder;
    use crate::{
        CapabilityDefinition, CapabilityId, CatalogueBuildError, Category, Maturity, Owner,
    };

    const AUTHOR_STYLES: CapabilityId = CapabilityId::new("purr.author-styles");
    const STYLE_ENGINE: CapabilityId = CapabilityId::new("purr.style-engine");
    const NODE_A: CapabilityId = CapabilityId::new("purr.node-a");
    const NODE_B: CapabilityId = CapabilityId::new("purr.node-b");

    const NO_DEPENDENCIES: &[CapabilityId] = &[];
    const ON_AUTHOR_STYLES: &[CapabilityId] = &[AUTHOR_STYLES];
    const ON_NODE_B: &[CapabilityId] = &[NODE_B];
    const ON_NODE_A: &[CapabilityId] = &[NODE_A];
    const ON_UNKNOWN: &[CapabilityId] = &[CapabilityId::new("purr.absent")];
    const ON_STYLE_ENGINE: &[CapabilityId] = &[STYLE_ENGINE];

    fn definition(
        id: CapabilityId,
        owner: Owner,
        dependencies: &'static [CapabilityId],
    ) -> CapabilityDefinition {
        CapabilityDefinition {
            id,
            owner,
            category: Category::EngineService,
            maturity: Maturity::Stable,
            dependencies,
            is_mandatory: false,
            is_built: true,
        }
    }

    #[test]
    fn valid_catalogue_builds_and_looks_up_definitions() {
        let mut builder = CatalogueBuilder::new();
        builder
            .add(definition(AUTHOR_STYLES, Owner::Purr, NO_DEPENDENCIES))
            .add(definition(STYLE_ENGINE, Owner::Purr, ON_AUTHOR_STYLES));

        let catalogue = builder.build().expect("a valid catalogue should build");

        let found = catalogue
            .get(STYLE_ENGINE)
            .expect("the style engine definition should be present");
        assert_eq!(found.id, STYLE_ENGINE);
        assert_eq!(found.dependencies, ON_AUTHOR_STYLES);

        assert!(catalogue.contains(AUTHOR_STYLES));
        assert!(catalogue.get(CapabilityId::new("purr.absent")).is_none());
        assert_eq!(catalogue.definitions().len(), 2);
    }

    #[test]
    fn duplicate_id_is_rejected() {
        let mut builder = CatalogueBuilder::new();
        builder
            .add(definition(AUTHOR_STYLES, Owner::Purr, NO_DEPENDENCIES))
            .add(definition(AUTHOR_STYLES, Owner::Purr, NO_DEPENDENCIES));

        assert_eq!(
            builder.build().unwrap_err(),
            CatalogueBuildError::DuplicateId(AUTHOR_STYLES)
        );
    }

    #[test]
    fn namespace_owner_mismatch_is_rejected() {
        let mut builder = CatalogueBuilder::new();
        builder.add(definition(AUTHOR_STYLES, Owner::Panther, NO_DEPENDENCIES));

        assert_eq!(
            builder.build().unwrap_err(),
            CatalogueBuildError::NamespaceOwnerMismatch(AUTHOR_STYLES)
        );
    }

    #[test]
    fn unknown_dependency_is_rejected() {
        let mut builder = CatalogueBuilder::new();
        builder.add(definition(STYLE_ENGINE, Owner::Purr, ON_UNKNOWN));

        assert_eq!(
            builder.build().unwrap_err(),
            CatalogueBuildError::UnknownDependency {
                dependent: STYLE_ENGINE,
                dependency: CapabilityId::new("purr.absent"),
            }
        );
    }

    #[test]
    fn self_dependency_is_rejected_as_a_cycle() {
        let mut builder = CatalogueBuilder::new();
        builder.add(definition(STYLE_ENGINE, Owner::Purr, ON_STYLE_ENGINE));

        assert!(matches!(
            builder.build().unwrap_err(),
            CatalogueBuildError::DependencyCycle(_)
        ));
    }

    #[test]
    fn two_node_cycle_is_rejected() {
        let mut builder = CatalogueBuilder::new();
        builder
            .add(definition(NODE_A, Owner::Purr, ON_NODE_B))
            .add(definition(NODE_B, Owner::Purr, ON_NODE_A));

        assert!(matches!(
            builder.build().unwrap_err(),
            CatalogueBuildError::DependencyCycle(_)
        ));
    }
}
