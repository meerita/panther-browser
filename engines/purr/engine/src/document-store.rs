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
//! A document holds its source bytes, its generation, and the DOM the tokenizer
//! and tree builder produce from the source on create. `render` runs style,
//! layout, and paint for the requested viewport and returns the generation-tagged
//! display list (the block and inline backgrounds, the per-glyph textured quads,
//! and the single glyph-atlas upload) in the engine namespace.

use crate::block_layout::{ConstraintSpace, layout_document};
use crate::computed_style::{StyleGeneration, resolve_document_style};
use crate::css_parser::{Origin, Stylesheet, parse_stylesheet};
use crate::dom_node::Dom;
use crate::fragment_tree::LayoutGeneration;
use crate::html_tree_builder::parse;
use crate::layout_unit::{LayoutSize, LayoutUnit};
use crate::paint::paint_document;
use crate::user_agent_styles::parse_user_agent_stylesheet;
use memory::{AccountingRegistry, Arena, ArenaId, Region};
use purr_graphics::{
    Color, DeviceGeneration, DrawCommand, Extent2d, ProducerNamespace, Rect, ResourceGeneration,
    ResourceUpload,
};
use purr_text::BundledFont;

/// Upper bound for the content width layout receives, in CSS pixels.
///
/// The seam viewport extent is untrusted input. The bound keeps the fixed-point
/// conversion of the available inline size in range before layout runs.
const MAX_CONTENT_WIDTH: u32 = 1_000_000;

/// Upper bound for the device pixel ratio paint applies.
///
/// A hostile or malformed ratio is clamped into this range, so paint and glyph
/// rasterization sizing stay bounded.
const MAX_DEVICE_PIXEL_RATIO: f32 = 8.0;

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
/// The document holds its source bytes, the generation it was created with, and
/// the DOM parsed from the source. Render reads the DOM to run style, layout, and
/// paint behind the same identity.
struct Document {
    generation: DocumentGeneration,
    // The source is retained for later re-decoding and diagnostics; it is not read
    // by the render pipeline yet.
    #[allow(dead_code)]
    source: Vec<u8>,
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

        // The tokenizer and tree builder run in lockstep to build the DOM. A
        // hard parse-limit breach (a tokenizer cap or the open-element cap)
        // aborts the parse; the store fails closed to a blank document rather
        // than surfacing a new error, because the seam contract is frozen and a
        // malformed local document is not a seam failure.
        let dom = parse(source).unwrap_or_default();

        let document = Document {
            generation,
            source: source.to_vec(),
            dom,
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
    /// `UnknownDocument`. For a live handle it runs style, layout, and paint for
    /// the viewport geometry and returns the generation-tagged display list. A
    /// malformed local document that overruns a layout cap is not a seam failure,
    /// so the pipeline fails closed to a blank frame rather than surfacing a new
    /// error, keeping the seam contract frozen.
    pub fn render(
        &self,
        id: DocumentId,
        generation: DocumentGeneration,
        content_extent: Extent2d,
        device_pixel_ratio: f32,
    ) -> Result<EngineFrame, DocumentError> {
        let Some(document) = self.documents.get(id.0) else {
            return Err(DocumentError::StaleGeneration);
        };

        if document.generation != generation {
            return Err(DocumentError::UnknownDocument);
        }

        Ok(render_document(
            &document.dom,
            generation,
            content_extent,
            device_pixel_ratio,
        ))
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

    /// Borrows the parsed DOM of a live document for inspection in tests.
    #[cfg(test)]
    fn document_dom(&self, id: DocumentId, generation: DocumentGeneration) -> Option<&Dom> {
        let document = self.documents.get(id.0)?;
        if document.generation != generation {
            return None;
        }
        Some(&document.dom)
    }
}

impl Default for DocumentStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Runs style, layout, and paint for one document, failing closed to a blank frame.
///
/// A cap breach, a numeric overflow, or a font-load failure in the pipeline yields
/// a minimal valid frame (a document-background fill) instead of an error, because
/// the frozen seam carries no render-failure code and a malformed local document is
/// not a seam failure.
fn render_document(
    dom: &Dom,
    generation: DocumentGeneration,
    content_extent: Extent2d,
    device_pixel_ratio: f32,
) -> EngineFrame {
    lower_document(dom, generation, content_extent, device_pixel_ratio)
        .unwrap_or_else(|| fallback_frame(generation, content_extent, device_pixel_ratio))
}

/// Lowers a document to a display list, or `None` on any internal pipeline failure.
fn lower_document(
    dom: &Dom,
    generation: DocumentGeneration,
    content_extent: Extent2d,
    device_pixel_ratio: f32,
) -> Option<EngineFrame> {
    let dpr = normalize_device_pixel_ratio(device_pixel_ratio);

    let author = extract_author_stylesheet(dom);
    let user_agent = parse_user_agent_stylesheet();
    let styles = resolve_document_style(
        dom,
        &user_agent,
        &author,
        StyleGeneration::new(generation.value()),
    );

    let available_inline =
        LayoutUnit::from_px_saturating(clamp_content_width(content_extent.width));
    let constraint = ConstraintSpace::new(available_inline, LayoutSize::Indefinite);
    let tree = layout_document(
        dom,
        &styles,
        &constraint,
        LayoutGeneration::new(generation.value()),
    )
    .ok()?;

    let font = BundledFont::load().ok()?;
    let paint = paint_document(
        &tree,
        &styles,
        &font,
        content_extent,
        dpr,
        ResourceGeneration::new(generation.value()),
        DeviceGeneration::new(1),
    )
    .ok()?;

    Some(EngineFrame {
        generation,
        producer: engine_producer_namespace(),
        uploads: paint.uploads,
        commands: paint.commands,
    })
}

/// A minimal valid frame: the document background over the content extent.
fn fallback_frame(
    generation: DocumentGeneration,
    content_extent: Extent2d,
    device_pixel_ratio: f32,
) -> EngineFrame {
    let dpr = normalize_device_pixel_ratio(device_pixel_ratio);
    EngineFrame {
        generation,
        producer: engine_producer_namespace(),
        uploads: Vec::new(),
        commands: vec![DrawCommand::FillRect {
            rect: Rect::new(
                0.0,
                0.0,
                content_extent.width as f32 * dpr,
                content_extent.height as f32 * dpr,
            ),
            color: Color::new(1.0, 1.0, 1.0, 1.0),
        }],
    }
}

/// Parses the concatenated `<style>` element text as the author stylesheet.
///
/// The walk is an explicit stack, so a deep document does not recurse. Every
/// `<style>` element contributes its text children, in document order. A document
/// with no `<style>` yields an empty author stylesheet.
fn extract_author_stylesheet(dom: &Dom) -> Stylesheet {
    let mut source = String::new();
    let mut stack = vec![dom.root()];
    while let Some(node) = stack.pop() {
        if dom.local_name(node) == Some("style")
            && let Some(children) = dom.children(node)
        {
            for &child in children {
                if let Some(text) = dom.text_data(child) {
                    source.push_str(text);
                }
            }
        }
        if let Some(children) = dom.children(node) {
            for &child in children.iter().rev() {
                stack.push(child);
            }
        }
    }
    parse_stylesheet(&source, Origin::Author)
}

/// Clamps the untrusted content width to the layout bound, in CSS pixels.
fn clamp_content_width(width: u32) -> i32 {
    width.min(MAX_CONTENT_WIDTH) as i32
}

/// Normalizes the device pixel ratio, defaulting a non-finite or non-positive
/// value to one and clamping an extreme value to the bound.
fn normalize_device_pixel_ratio(device_pixel_ratio: f32) -> f32 {
    if device_pixel_ratio.is_finite() && device_pixel_ratio > 0.0 {
        device_pixel_ratio.min(MAX_DEVICE_PIXEL_RATIO)
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom_node::NodeId;
    use purr_graphics::ResourceKind;

    const STYLED_SOURCE: &[u8] = b"<html><head><style>.card{background-color:#eef;width:100px;height:40px}</style></head><body><div class=\"card\">hi</div></body></html>";

    fn geometry() -> (Extent2d, f32) {
        (Extent2d::new(800, 600), 1.0)
    }

    fn element_child(dom: &Dom, parent: NodeId, name: &str) -> Option<NodeId> {
        dom.children(parent)?
            .iter()
            .copied()
            .find(|&child| dom.local_name(child) == Some(name))
    }

    #[test]
    fn create_parses_the_source_into_a_dom() {
        let mut store = DocumentStore::new();
        let (id, generation) = store
            .create(
                b"<!doctype html><html><head><title>t</title></head><body><p>hi</p></body></html>",
            )
            .expect("create succeeds");

        let dom = store
            .document_dom(id, generation)
            .expect("document resolves");
        let html = element_child(dom, dom.root(), "html").expect("html under the root");
        let body = element_child(dom, html, "body").expect("body under html");
        let paragraph = element_child(dom, body, "p").expect("p under body");
        let text = dom.children(paragraph).expect("p resolves")[0];

        assert_eq!(dom.text_data(text), Some("hi"));
    }

    #[test]
    fn render_lowers_the_document_to_a_glyph_atlas_frame() {
        let mut store = DocumentStore::new();
        let (id, generation) = store.create(STYLED_SOURCE).expect("create succeeds");
        let (extent, dpr) = geometry();

        let frame = store
            .render(id, generation, extent, dpr)
            .expect("render succeeds");

        assert_eq!(frame.generation, generation);
        assert_eq!(frame.producer, engine_producer_namespace());

        // Exactly one upload, the glyph atlas.
        assert_eq!(frame.uploads.len(), 1);
        assert_eq!(
            frame.uploads[0].resource.resource_kind(),
            ResourceKind::GlyphAtlas
        );

        // The card background is a fill; the "hi" text produces glyph quads.
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
    fn render_tags_the_frame_with_the_document_generation() {
        let mut store = DocumentStore::new();
        let (first_id, first_generation) = store.create(STYLED_SOURCE).expect("create succeeds");
        let (second_id, second_generation) = store.create(STYLED_SOURCE).expect("create succeeds");
        let (extent, dpr) = geometry();

        let first = store
            .render(first_id, first_generation, extent, dpr)
            .expect("render succeeds");
        let second = store
            .render(second_id, second_generation, extent, dpr)
            .expect("render succeeds");

        assert_eq!(first.generation, first_generation);
        assert_eq!(second.generation, second_generation);
        assert_ne!(first.generation, second.generation);
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
