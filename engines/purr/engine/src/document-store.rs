// @file engines/purr/engine/src/document-store.rs
// @description Owns the engine document store, its identities, and the raw pipeline output.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Engine document store.
//!
//! The store owns every attached document and the raw pipeline output the
//! embedding boundary wraps. A document is stored in a generational-index arena
//! in the Document memory region, so a freed slot rejects a stale handle. The
//! store surfaces opaque `DocumentId`/`DocumentGeneration` identities; the
//! embedding seam re-exports them as opaque handle components and never inspects
//! their contents.
//!
//! At this phase a document holds only its source bytes and its generation, and
//! `render` returns a single `Clear` command in the engine namespace. Later
//! pipeline phases replace the render body without changing these identities or
//! the raw-output shape.

use crate::dom_node::Dom;
use memory::{AccountingRegistry, Arena, ArenaId, Region};
use purr_graphics::{Color, DrawCommand, Extent2d, ProducerNamespace, ResourceUpload};

/// Draw-op identifier space of the engine.
///
/// The engine and the shell each own a distinct namespace so their draw-op
/// streams merge into one submission without an identifier collision. The shell
/// path uses namespace `1`; the engine uses `2`. This is a function, not a
/// constant, because the graphics constructor is not `const` and the graphics
/// interface is reused unchanged.
pub fn engine_producer_namespace() -> ProducerNamespace {
    ProducerNamespace::new(2)
}

/// Upper bound for the byte length of one document source.
///
/// The document subsystem owns this limit and rejects a larger source before it
/// allocates from it. The bound applies even to local fixture bytes, which the
/// security rules still treat as untrusted input.
pub const MAX_SOURCE_BYTES: usize = 8 * 1024 * 1024;

/// Opaque identity of one document in the store.
///
/// The identity pairs the arena slot with the generation the arena issued, so a
/// handle to a freed or reused slot stops resolving. Only the store constructs
/// an identity, and the embedding seam treats it as an opaque value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DocumentId(ArenaId);

/// Marks the lifetime of one document.
///
/// A superseded generation is rejected on render, reusing the graphics and
/// memory generation model. The value is opaque to the embedding seam.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DocumentGeneration(u64);

impl DocumentGeneration {
    /// The generation of the first document a store issues.
    pub const FIRST: Self = Self(1);

    pub fn new(value: u64) -> Self {
        Self(value)
    }

    pub fn value(self) -> u64 {
        self.0
    }

    /// Returns the next generation.
    ///
    /// Uses checked arithmetic. A `u64` generation cannot overflow in practice,
    /// so `None` is unreachable, but the store fails safe instead of wrapping a
    /// generation into an earlier value.
    pub fn next(self) -> Option<Self> {
        self.0.checked_add(1).map(Self)
    }
}

/// Failure the document store reports to its caller.
///
/// The store owns these variants. The embedding seam translates them into its
/// own `SeamError` at the boundary, so no store-internal type crosses it. Each
/// message is a static, factual, non-secret string.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum DocumentError {
    #[error("document source exceeds the maximum size")]
    SourceTooLarge,
    #[error("document handle does not name a known document")]
    UnknownDocument,
    #[error("document handle names a superseded generation")]
    StaleGeneration,
}

/// Raw pipeline output for one document render.
///
/// This is the engine-owned shape the embedding seam wraps into its immutable
/// `DocumentFrame`. It carries the generation it was produced for, the engine
/// namespace, the uploads it needs, and the commands that paint it. It holds no
/// backend type and is serialization-ready.
#[derive(Debug, Clone, PartialEq)]
pub struct EngineFrame {
    pub generation: DocumentGeneration,
    pub producer: ProducerNamespace,
    pub uploads: Vec<ResourceUpload>,
    pub commands: Vec<DrawCommand>,
}

/// One document owned by the store.
///
/// At this phase the document holds its source bytes, the generation it was
/// created with, and an empty DOM tree. Later phases add the style, layout, and
/// paint state behind the same identity.
struct Document {
    generation: DocumentGeneration,
    // The tokenizer reads the source and the tree builder fills the DOM; both are
    // later phases. The store owns them now so the seam holds them behind a
    // stable identity.
    #[allow(dead_code)]
    source: Vec<u8>,
    #[allow(dead_code)]
    dom: Dom,
}

/// Owns every attached document and issues opaque handles to them.
///
/// Documents live in a generational-index arena accounted to the Document
/// region. The store rejects a source above `MAX_SOURCE_BYTES`, rejects a
/// superseded handle on render, and frees a document on destroy.
pub struct DocumentStore {
    documents: Arena<Document>,
    accounting: AccountingRegistry,
    next_generation: DocumentGeneration,
}

impl DocumentStore {
    pub fn new() -> Self {
        Self {
            documents: Arena::new(),
            accounting: AccountingRegistry::new(),
            next_generation: DocumentGeneration::FIRST,
        }
    }

    /// Creates a document from local source bytes.
    ///
    /// Fails closed with `SourceTooLarge` when the source exceeds the bound,
    /// before it copies or allocates from it.
    pub fn create(
        &mut self,
        source: &[u8],
    ) -> Result<(DocumentId, DocumentGeneration), DocumentError> {
        if source.len() > MAX_SOURCE_BYTES {
            return Err(DocumentError::SourceTooLarge);
        }

        let generation = self.next_generation;
        // The arena slot distinguishes documents even if the counter saturates,
        // so a stopped counter is harmless. A `u64` counter cannot reach this in
        // practice.
        if let Some(next) = generation.next() {
            self.next_generation = next;
        }

        let document = Document {
            generation,
            source: source.to_vec(),
            dom: Dom::new(),
        };
        let id = self
            .documents
            .insert_accounted(document, Region::Document, &self.accounting);

        Ok((DocumentId(id), generation))
    }

    /// Runs the pipeline for one document and returns its raw output.
    ///
    /// Rejects a handle whose slot was freed with `StaleGeneration`, and a
    /// handle whose generation does not match the live document with
    /// `UnknownDocument`. The geometry is consumed by later layout and paint
    /// phases; at this phase render clears the content box.
    pub fn render(
        &self,
        id: DocumentId,
        generation: DocumentGeneration,
        _content_extent: Extent2d,
        _device_pixel_ratio: f32,
    ) -> Result<EngineFrame, DocumentError> {
        let Some(document) = self.documents.get(id.0) else {
            return Err(DocumentError::StaleGeneration);
        };

        if document.generation != generation {
            return Err(DocumentError::UnknownDocument);
        }

        Ok(EngineFrame {
            generation,
            producer: engine_producer_namespace(),
            uploads: Vec::new(),
            commands: vec![DrawCommand::Clear {
                color: Color::new(1.0, 1.0, 1.0, 1.0),
            }],
        })
    }

    /// Frees a document.
    ///
    /// Idempotent: a stale, mismatched, or already-freed handle removes nothing
    /// and does not error.
    pub fn destroy(&mut self, id: DocumentId, generation: DocumentGeneration) {
        let matches = self
            .documents
            .get(id.0)
            .is_some_and(|document| document.generation == generation);

        if matches {
            self.documents
                .remove_accounted(id.0, Region::Document, &self.accounting);
        }
    }
}

impl Default for DocumentStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn geometry() -> (Extent2d, f32) {
        (Extent2d::new(800, 600), 1.0)
    }

    #[test]
    fn create_reports_the_engine_namespace_on_render() {
        let mut store = DocumentStore::new();
        let (id, generation) = store.create(b"<html></html>").expect("create succeeds");
        let (extent, dpr) = geometry();

        let frame = store
            .render(id, generation, extent, dpr)
            .expect("render succeeds");

        assert_eq!(frame.generation, generation);
        assert_eq!(frame.producer, engine_producer_namespace());
        assert!(frame.uploads.is_empty());
        assert_eq!(
            frame.commands,
            vec![DrawCommand::Clear {
                color: Color::new(1.0, 1.0, 1.0, 1.0),
            }]
        );
    }

    #[test]
    fn create_rejects_a_source_above_the_bound() {
        let mut store = DocumentStore::new();
        let source = vec![0u8; MAX_SOURCE_BYTES + 1];

        assert_eq!(store.create(&source), Err(DocumentError::SourceTooLarge));
    }

    #[test]
    fn render_after_destroy_rejects_a_stale_handle() {
        let mut store = DocumentStore::new();
        let (id, generation) = store.create(b"<html></html>").expect("create succeeds");
        let (extent, dpr) = geometry();

        store.destroy(id, generation);

        assert_eq!(
            store.render(id, generation, extent, dpr),
            Err(DocumentError::StaleGeneration)
        );
    }

    #[test]
    fn render_with_a_mismatched_generation_is_unknown() {
        let mut store = DocumentStore::new();
        let (id, generation) = store.create(b"<html></html>").expect("create succeeds");
        let (extent, dpr) = geometry();
        let mismatched = DocumentGeneration::new(generation.value() + 1);

        assert_eq!(
            store.render(id, mismatched, extent, dpr),
            Err(DocumentError::UnknownDocument)
        );
    }

    #[test]
    fn destroy_is_idempotent() {
        let mut store = DocumentStore::new();
        let (id, generation) = store.create(b"<html></html>").expect("create succeeds");

        store.destroy(id, generation);
        store.destroy(id, generation);
    }

    #[test]
    fn generation_next_advances_strictly() {
        let current = DocumentGeneration::new(4);
        let advanced = current.next().expect("u64 generation does not overflow");

        assert!(advanced.value() > current.value());
    }
}
