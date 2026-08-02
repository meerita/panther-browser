// @file engines/purr/embedding/src/document-attachment.rs
// @description Defines the Panther-to-Purr document-attachment seam.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Document-attachment seam.
//!
//! This is the narrow, prose-free, generation-tagged boundary that carries a
//! document into the engine and a renderable result back. A product attaches a
//! source, produces a frame for a viewport, and detaches. The engine owns the
//! document; the product owns the viewport and holds only an opaque handle.
//!
//! The seam carries no document lifecycle state (no readiness, visibility,
//! freeze, or navigation). `produce` is synchronous and returns an owned,
//! immutable, generation-tagged frame, so the caller never holds a borrow into
//! engine state. A superseded generation is rejected. Errors are a typed
//! `SeamError` with stable reason codes; no engine-internal or dependency error
//! type crosses the boundary.

use purr_engine::{
    DocumentError, DocumentGeneration, DocumentId, DocumentStore, EngineFrame,
    engine_producer_namespace,
};
use purr_graphics::{
    DrawCommand, Extent2d, MAX_DRAW_COMMANDS, MAX_RESOURCE_UPLOADS, ProducerNamespace,
    ResourceUpload,
};

/// Opaque handle to an attached document.
///
/// The handle names the document, the generation it was attached at, and the
/// engine draw-op namespace. It is a value type the product holds across
/// resizes; the geometry is a per-render input, not part of the handle. The
/// engine owns the backing document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DocumentHandle {
    document: DocumentId,
    generation: DocumentGeneration,
    producer: ProducerNamespace,
}

impl DocumentHandle {
    pub fn generation(self) -> DocumentGeneration {
        self.generation
    }

    pub fn producer(self) -> ProducerNamespace {
        self.producer
    }
}

/// Geometry of the content viewport for one render.
///
/// The content box crosses in CSS pixels; the device pixel ratio is consumed by
/// paint and glyph rasterization. The engine lays out in document-local space at
/// origin, so the product owns the translate to the viewport origin, the clip,
/// and the device-pixel scale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewportGeometry {
    pub content_extent: Extent2d,
    pub device_pixel_ratio: f32,
}

/// Owned immutable result of one render.
///
/// The frame is a self-contained snapshot: the generation it was produced for,
/// the engine namespace, the uploads it needs, and the commands that paint it,
/// all in document-local coordinates. The product merges it with the shell
/// chrome into one submission. A frame from a superseded generation is rejected
/// before it reaches the compositor.
#[derive(Debug, Clone, PartialEq)]
pub struct DocumentFrame {
    pub generation: DocumentGeneration,
    pub producer: ProducerNamespace,
    pub uploads: Vec<ResourceUpload>,
    pub commands: Vec<DrawCommand>,
}

impl DocumentFrame {
    /// Rejects an over-bound frame.
    ///
    /// A command or upload count above the graphics bound, or an upload with an
    /// invalid descriptor, is a `FrameRejected`. The exact pixel-buffer-length
    /// match is enforced once by `FrameSubmission::validate` when the product
    /// builds the submission, so the seam does not duplicate that check.
    pub fn validate(&self) -> Result<(), SeamError> {
        if self.commands.len() > MAX_DRAW_COMMANDS {
            return Err(SeamError::FrameRejected);
        }

        if self.uploads.len() > MAX_RESOURCE_UPLOADS {
            return Err(SeamError::FrameRejected);
        }

        for upload in &self.uploads {
            upload
                .descriptor
                .validate()
                .map_err(|_| SeamError::FrameRejected)?;
        }

        Ok(())
    }
}

/// Failure the seam reports to the product.
///
/// The seam owns these variants. They are stable reason codes, prose-free, and
/// carry no engine-internal or dependency error. Each message is a static,
/// factual, non-secret string for developer diagnostics only.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum SeamError {
    #[error("document source exceeds the maximum size")]
    SourceTooLarge,
    #[error("document handle does not name a known document")]
    UnknownDocument,
    #[error("document handle names a superseded generation")]
    StaleGeneration,
    #[error("produced frame was rejected by validation")]
    FrameRejected,
}

/// Translates an engine document error into the seam vocabulary.
///
/// Keeps the engine error type out of the seam's public surface.
fn translate(error: DocumentError) -> SeamError {
    match error {
        DocumentError::SourceTooLarge => SeamError::SourceTooLarge,
        DocumentError::UnknownDocument => SeamError::UnknownDocument,
        DocumentError::StaleGeneration => SeamError::StaleGeneration,
    }
}

/// Drives the document-attachment seam for one product.
///
/// The session owns the engine document store. It is the single entry point the
/// product uses to attach a document, produce a frame, and detach.
pub struct DocumentSession {
    store: DocumentStore,
}

impl DocumentSession {
    pub fn new() -> Self {
        Self {
            store: DocumentStore::new(),
        }
    }

    /// Attaches a document from local source bytes.
    ///
    /// Fails closed with `SourceTooLarge` when the source exceeds the bound.
    pub fn attach(&mut self, source: &[u8]) -> Result<DocumentHandle, SeamError> {
        let (document, generation) = self.store.create(source).map_err(translate)?;

        Ok(DocumentHandle {
            document,
            generation,
            producer: engine_producer_namespace(),
        })
    }

    /// Produces an owned immutable frame for the given viewport.
    ///
    /// Rejects a superseded handle with `StaleGeneration`. The returned frame is
    /// a self-contained value; the product never holds a borrow into engine
    /// state.
    pub fn produce(
        &mut self,
        handle: &DocumentHandle,
        geometry: ViewportGeometry,
    ) -> Result<DocumentFrame, SeamError> {
        let raw: EngineFrame = self
            .store
            .render(
                handle.document,
                handle.generation,
                geometry.content_extent,
                geometry.device_pixel_ratio,
            )
            .map_err(translate)?;

        let frame = DocumentFrame {
            generation: raw.generation,
            producer: raw.producer,
            uploads: raw.uploads,
            commands: raw.commands,
        };
        frame.validate()?;

        Ok(frame)
    }

    /// Detaches a document and invalidates its handle.
    ///
    /// Explicit and idempotent: a second detach of the same handle does nothing
    /// and does not error.
    pub fn detach(&mut self, handle: DocumentHandle) {
        self.store.destroy(handle.document, handle.generation);
    }
}

impl Default for DocumentSession {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use purr_graphics::{DrawCommand, ResourceKind};

    const SOURCE: &[u8] = b"<!doctype html><html></html>";
    const STYLED_SOURCE: &[u8] = b"<!doctype html><html><head><style>.card{background-color:#eef;width:120px;height:40px}</style></head><body><div class=\"card\">hello world</div></body></html>";

    fn geometry() -> ViewportGeometry {
        ViewportGeometry {
            content_extent: Extent2d::new(800, 600),
            device_pixel_ratio: 1.0,
        }
    }

    #[test]
    fn attach_returns_a_handle_matching_the_document() {
        let mut session = DocumentSession::new();

        let handle = session.attach(SOURCE).expect("attach succeeds");

        assert_eq!(handle.producer(), engine_producer_namespace());
        assert_eq!(handle.generation(), DocumentGeneration::FIRST);
    }

    #[test]
    fn attach_rejects_a_source_above_the_bound() {
        let mut session = DocumentSession::new();
        let source = vec![0u8; purr_engine::MAX_SOURCE_BYTES + 1];

        assert_eq!(session.attach(&source), Err(SeamError::SourceTooLarge));
    }

    #[test]
    fn produce_returns_a_valid_frame_for_the_handle_generation() {
        let mut session = DocumentSession::new();
        let handle = session.attach(SOURCE).expect("attach succeeds");

        let frame = session
            .produce(&handle, geometry())
            .expect("produce succeeds");

        assert_eq!(frame.generation, handle.generation());
        assert_eq!(frame.producer, engine_producer_namespace());
        assert_eq!(frame.validate(), Ok(()));
    }

    #[test]
    fn produce_lowers_the_document_to_a_validated_frame() {
        let mut session = DocumentSession::new();
        let handle = session.attach(STYLED_SOURCE).expect("attach succeeds");

        let frame = session
            .produce(&handle, geometry())
            .expect("produce succeeds");

        assert_eq!(frame.validate(), Ok(()));
        assert_eq!(frame.generation, handle.generation());

        // Exactly one glyph-atlas upload.
        assert_eq!(frame.uploads.len(), 1);
        assert_eq!(
            frame.uploads[0].resource.resource_kind(),
            ResourceKind::GlyphAtlas
        );

        // A background fill and at least one glyph quad.
        assert!(
            frame
                .commands
                .iter()
                .any(|command| matches!(command, DrawCommand::FillRect { .. }))
        );
        assert!(
            frame
                .commands
                .iter()
                .any(|command| matches!(command, DrawCommand::TexturedQuad { .. }))
        );
    }

    #[test]
    fn a_new_generation_produces_a_frame_tagged_with_the_new_generation() {
        let mut session = DocumentSession::new();
        let first = session.attach(STYLED_SOURCE).expect("attach succeeds");
        let second = session.attach(STYLED_SOURCE).expect("attach succeeds");

        let first_frame = session
            .produce(&first, geometry())
            .expect("produce succeeds");
        let second_frame = session
            .produce(&second, geometry())
            .expect("produce succeeds");

        assert_eq!(first_frame.generation, first.generation());
        assert_eq!(second_frame.generation, second.generation());
        assert_ne!(first_frame.generation, second_frame.generation);
    }

    #[test]
    fn produce_after_detach_rejects_a_superseded_generation() {
        let mut session = DocumentSession::new();
        let handle = session.attach(SOURCE).expect("attach succeeds");

        session.detach(handle);

        assert_eq!(
            session.produce(&handle, geometry()),
            Err(SeamError::StaleGeneration)
        );
    }

    #[test]
    fn detach_is_idempotent() {
        let mut session = DocumentSession::new();
        let handle = session.attach(SOURCE).expect("attach succeeds");

        session.detach(handle);
        session.detach(handle);
    }
}
