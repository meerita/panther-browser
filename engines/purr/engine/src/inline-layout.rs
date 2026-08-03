// @file engines/purr/engine/src/inline-layout.rs
// @description Lays out the inline content of a block into line fragments with whitespace line breaking.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Inline formatting context.
//!
//! This stage fills the inline content of one block box. It shapes each inline text
//! node into a [`GlyphRun`], flattens the inline subtree into a stream of glyph
//! cells, breaks the stream into lines at whitespace opportunities, and slices each
//! run at a break so the slice keeps its cluster map. Each line becomes an
//! immutable [`LineFragment`] holding text fragments and inline-box background
//! fragments in document-local coordinates.
//!
//! Line metrics come from the font metrics and the used `line-height`. The block
//! advances by the line height for every line, and every line shares one baseline
//! measured from the top of the line box. Left-to-right, horizontal writing, one
//! font: bidi, complex-script breaking, hyphenation, and justification are out of
//! scope for M2.
//!
//! Every count is bounded. Inline-item, line, and text-fragment caps use checked
//! arithmetic and abort with a typed [`LayoutError`] instead of a panic, and the
//! line loop is bounded by the finite segment list, so a hostile document cannot
//! force an unbounded relayout. The stage owns no pixels: it produces geometry
//! only, and no glyph texture coordinate enters a fragment.

// The block-layout stage and the paint stage consume the inline flow and the line
// fragments. This phase builds them and exercises them through the unit tests, so
// some entry points are otherwise unused in a non-test build.
#![allow(dead_code)]

use crate::block_layout::{LayoutContext, account_fragment, parse_px_length};
use crate::css_parser::PropertyId;
use crate::dom_node::{NodeId, NodeKind};
use crate::fragment_tree::{BoxFragment, LineFragment, LineItem, TextFragment};
use crate::layout_tree::{LayoutBox, LayoutError};
use crate::layout_unit::{LayoutUnit, LogicalPoint, LogicalRect, LogicalSize, ONE_PX_RAW};
use crate::text_unit_conversion::{to_layout_unit, to_text_unit};
use purr_text::{FontMetrics, GlyphRun, GlyphRunGeneration, GlyphRunId, ShapingRequest};

/// Upper bound for the number of inline items in one formatting context.
///
/// Each inline text run and each inline box counts as one item. The bound rejects
/// a document with an implausible inline-item count before the line breaker runs.
pub const MAX_INLINE_ITEMS: usize = 65_536;

/// Upper bound for the number of line boxes in one formatting context.
pub const MAX_LINES: usize = 65_536;

/// Upper bound for the number of text fragments in one document.
pub const MAX_TEXT_FRAGMENTS: usize = 65_536;

/// The absolute medium font size, used when no element style resolves one.
const DEFAULT_FONT_SIZE: LayoutUnit = LayoutUnit::from_raw(16 * ONE_PX_RAW);

/// The mutable per-layout counters that enforce the layout caps.
///
/// One instance threads through the whole layout of a document. Every count uses
/// checked arithmetic, so an adversarial document fails closed instead of wrapping
/// a counter. `next_run_id` hands each shaped run a distinct identity.
pub(crate) struct LayoutCounters {
    pub(crate) fragments: usize,
    pub(crate) inline_items: usize,
    pub(crate) lines: usize,
    pub(crate) text_fragments: usize,
    pub(crate) next_run_id: u32,
}

impl LayoutCounters {
    pub(crate) fn new() -> Self {
        Self {
            fragments: 0,
            inline_items: 0,
            lines: 0,
            text_fragments: 0,
            next_run_id: 1,
        }
    }
}

/// The immutable inline content of one block: its lines and their total height.
///
/// The lines are positioned relative to the block border-box origin, so the block
/// layout translates them into document-local coordinates when it flattens the
/// fragment tree. The content height is the sum of the line heights.
pub(crate) struct InlineFlow {
    pub(crate) lines: Vec<LineFragment>,
    pub(crate) content_height: LayoutUnit,
}

/// Lays out the inline content of one block box into line fragments.
///
/// `content_origin` is the top-left of the content area relative to the block
/// border-box origin, so the lines already include the block padding offset.
/// `available_inline` is the content width the lines fill.
pub(crate) fn layout_inline(
    ctx: &LayoutContext,
    layout_box: &LayoutBox,
    content_origin: LogicalPoint,
    available_inline: LayoutUnit,
    counters: &mut LayoutCounters,
) -> Result<InlineFlow, LayoutError> {
    let mut runs = Vec::new();
    collect_runs(
        ctx,
        layout_box.children(),
        layout_box.node(),
        None,
        &mut runs,
        counters,
    )?;

    let cells = build_cells(&runs);
    if cells.is_empty() {
        return Ok(InlineFlow {
            lines: Vec::new(),
            content_height: LayoutUnit::ZERO,
        });
    }

    let segments = build_segments(&cells);
    let line_ranges = break_lines(&cells, &segments, available_inline)?;

    let mut lines = Vec::with_capacity(line_ranges.len());
    let mut block_y = content_origin.y;
    let mut content_height = LayoutUnit::ZERO;
    for (start, end) in line_ranges {
        account_line(counters)?;
        let line = emit_line(
            ctx,
            &runs,
            &cells,
            start,
            end,
            content_origin.x,
            block_y,
            counters,
        )?;
        let line_height = line.rect().size.height;
        block_y = block_y
            .checked_add(line_height)
            .ok_or(LayoutError::Overflow)?;
        content_height = content_height
            .checked_add(line_height)
            .ok_or(LayoutError::Overflow)?;
        lines.push(line);
    }

    Ok(InlineFlow {
        lines,
        content_height,
    })
}

/// One shaped inline text run and the style it was laid out with.
struct InlineRun {
    run: GlyphRun,
    text: String,
    inline_box: Option<NodeId>,
    line_height: LayoutUnit,
    ascent: LayoutUnit,
    descent: LayoutUnit,
}

/// The resolved inline typography of one styling element.
struct InlineStyle {
    font_size: LayoutUnit,
    line_height: LayoutUnit,
    metrics: FontMetrics,
}

/// Walks the inline subtree and shapes each text node into an [`InlineRun`].
///
/// `current_element` is the nearest element whose style supplies the typography.
/// `inline_box` is the nearest enclosing inline element that paints a background or
/// padding, so a text run records which inline box, if any, sits behind it.
fn collect_runs(
    ctx: &LayoutContext,
    boxes: &[LayoutBox],
    current_element: Option<NodeId>,
    inline_box: Option<NodeId>,
    runs: &mut Vec<InlineRun>,
    counters: &mut LayoutCounters,
) -> Result<(), LayoutError> {
    for child in boxes.iter().filter(|child| child.is_inline_level()) {
        let Some(node) = child.node() else {
            continue;
        };
        match ctx.dom.kind(node) {
            Some(NodeKind::Element) => {
                account_inline_item(counters)?;
                let next_inline_box = if inline_box_paints(ctx, node) {
                    Some(node)
                } else {
                    inline_box
                };
                collect_runs(
                    ctx,
                    child.children(),
                    Some(node),
                    next_inline_box,
                    runs,
                    counters,
                )?;
            }
            Some(NodeKind::Text) => {
                account_inline_item(counters)?;
                let Some(text) = ctx.dom.text_data(node) else {
                    continue;
                };
                if text.is_empty() {
                    continue;
                }
                let style = resolve_inline_style(ctx, current_element)?;
                let run = shape_run(ctx, text, style.font_size, counters)?;
                runs.push(InlineRun {
                    run,
                    text: text.to_owned(),
                    inline_box,
                    line_height: style.line_height,
                    ascent: to_layout_unit(style.metrics.ascent()),
                    descent: to_layout_unit(style.metrics.descent()),
                });
            }
            _ => {}
        }
    }
    Ok(())
}

/// Shapes one text run through the shaping adapter with a fresh run identity.
fn shape_run(
    ctx: &LayoutContext,
    text: &str,
    font_size: LayoutUnit,
    counters: &mut LayoutCounters,
) -> Result<GlyphRun, LayoutError> {
    let run_id = GlyphRunId::new(counters.next_run_id);
    counters.next_run_id = counters
        .next_run_id
        .checked_add(1)
        .ok_or(LayoutError::TooManyInlineItems)?;
    ctx.adapter
        .shape(ShapingRequest {
            font: ctx.font,
            text,
            size: to_text_unit(font_size),
            run_id,
            generation: GlyphRunGeneration::new(ctx.layout_generation.value()),
        })
        .map_err(|_| LayoutError::TextShapingFailed)
}

/// Resolves the font size, line height, and font metrics of a styling element.
fn resolve_inline_style(
    ctx: &LayoutContext,
    element: Option<NodeId>,
) -> Result<InlineStyle, LayoutError> {
    let style = element.and_then(|node| ctx.styles.get(node));
    let font_size = style
        .map(|style| parse_font_size(style.get(PropertyId::FontSize)))
        .unwrap_or(DEFAULT_FONT_SIZE);
    let metrics = ctx
        .font
        .metrics(to_text_unit(font_size))
        .ok_or(LayoutError::TextShapingFailed)?;
    let line_height_value = style
        .map(|style| style.get(PropertyId::LineHeight))
        .unwrap_or("normal");
    let line_height = resolve_line_height(line_height_value, font_size, metrics)?;
    Ok(InlineStyle {
        font_size,
        line_height,
        metrics,
    })
}

/// A pixel font size, defaulting to the medium size for a non-pixel value.
fn parse_font_size(value: &str) -> LayoutUnit {
    parse_px_length(value).unwrap_or(DEFAULT_FONT_SIZE)
}

/// Resolves the used `line-height` for a font size and metrics.
///
/// `normal` uses the font default line height, a pixel length uses that length, and
/// a unitless multiplier scales the font size. Any other value falls back to the
/// font default line height.
fn resolve_line_height(
    value: &str,
    font_size: LayoutUnit,
    metrics: FontMetrics,
) -> Result<LayoutUnit, LayoutError> {
    let value = value.trim();
    if value == "normal" {
        return Ok(to_layout_unit(metrics.line_height()));
    }
    if let Some(length) = parse_px_length(value) {
        return Ok(length.max(LayoutUnit::ZERO));
    }
    if let Some((numerator, denominator)) = parse_unitless(value) {
        let scaled = i64::from(font_size.raw())
            .checked_mul(numerator)
            .ok_or(LayoutError::Overflow)?;
        return LayoutUnit::from_raw_ratio(scaled, denominator).ok_or(LayoutError::Overflow);
    }
    Ok(to_layout_unit(metrics.line_height()))
}

/// Parses a unitless number into a numerator and denominator, or `None`.
///
/// An integer such as `2` becomes `(2, 1)`; a decimal such as `1.5` becomes
/// `(15, 10)`, so the scaling stays integer-only and deterministic.
fn parse_unitless(value: &str) -> Option<(i64, i64)> {
    if let Ok(integer) = value.parse::<i64>() {
        return Some((integer, 1));
    }
    let (integer_part, fraction_part) = value.split_once('.')?;
    if fraction_part.is_empty() || !fraction_part.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let integer: i64 = if integer_part.is_empty() {
        0
    } else {
        integer_part.parse().ok()?
    };
    let fraction: i64 = fraction_part.parse().ok()?;
    let mut denominator: i64 = 1;
    for _ in 0..fraction_part.len() {
        denominator = denominator.checked_mul(10)?;
    }
    let numerator = integer.checked_mul(denominator)?.checked_add(fraction)?;
    Some((numerator, denominator))
}

/// One shaped glyph flattened out of its run for line breaking.
struct GlyphCell {
    run_index: usize,
    glyph_pos: usize,
    advance: LayoutUnit,
    is_space: bool,
    inline_box: Option<NodeId>,
}

/// Flattens the shaped runs into a single ordered stream of glyph cells.
fn build_cells(runs: &[InlineRun]) -> Vec<GlyphCell> {
    let mut cells = Vec::new();
    for (run_index, run) in runs.iter().enumerate() {
        for glyph_pos in 0..run.run.len() {
            let Some(glyph) = run.run.glyph(glyph_pos) else {
                continue;
            };
            let source = run.run.source_index(glyph_pos).unwrap_or(0);
            let is_space = run
                .text
                .get(source..)
                .and_then(|rest| rest.chars().next())
                .map(|character| character.is_whitespace())
                .unwrap_or(false);
            cells.push(GlyphCell {
                run_index,
                glyph_pos,
                advance: to_layout_unit(glyph.advance()),
                is_space,
                inline_box: run.inline_box,
            });
        }
    }
    cells
}

/// One word and its trailing collapsible spaces in the glyph-cell stream.
///
/// `start..word_end` is the visible word; `word_end..end` is the trailing space run
/// that provides a break opportunity after the word.
struct Segment {
    start: usize,
    word_end: usize,
    end: usize,
}

/// Splits the glyph-cell stream into words with trailing spaces.
///
/// Leading spaces before the first word collapse away, so a line never starts with
/// a space. Interior spaces stay with the preceding word as its trailing run.
fn build_segments(cells: &[GlyphCell]) -> Vec<Segment> {
    let mut segments = Vec::new();
    let count = cells.len();
    let mut index = 0;
    while index < count && cells[index].is_space {
        index += 1;
    }
    while index < count {
        let start = index;
        while index < count && !cells[index].is_space {
            index += 1;
        }
        let word_end = index;
        while index < count && cells[index].is_space {
            index += 1;
        }
        segments.push(Segment {
            start,
            word_end,
            end: index,
        });
    }
    segments
}

/// Breaks the segments into line ranges greedily at whitespace opportunities.
///
/// A line grows until the next word would exceed the available inline size, then it
/// breaks before that word. Each returned range excludes the trailing spaces of its
/// last word. The loop is bounded by the finite segment list, so it always
/// terminates. A single word wider than the available size is not broken (no
/// hyphenation at M2) and simply overflows its line.
fn break_lines(
    cells: &[GlyphCell],
    segments: &[Segment],
    available: LayoutUnit,
) -> Result<Vec<(usize, usize)>, LayoutError> {
    let mut lines = Vec::new();
    let mut line_start: Option<usize> = None;
    let mut line_word_end = 0;
    let mut used = LayoutUnit::ZERO;

    for segment in segments {
        let word_width = advance_sum(cells, segment.start, segment.word_end)?;
        if let Some(start) = line_start {
            let projected = used.checked_add(word_width).ok_or(LayoutError::Overflow)?;
            if projected > available {
                lines.push((start, line_word_end));
                line_start = None;
                used = LayoutUnit::ZERO;
            }
        }
        if line_start.is_none() {
            line_start = Some(segment.start);
        }
        used = used.checked_add(word_width).ok_or(LayoutError::Overflow)?;
        line_word_end = segment.word_end;
        let space_width = advance_sum(cells, segment.word_end, segment.end)?;
        used = used.checked_add(space_width).ok_or(LayoutError::Overflow)?;
    }

    if let Some(start) = line_start {
        lines.push((start, line_word_end));
    }
    Ok(lines)
}

/// Builds one immutable line fragment from a glyph-cell range.
#[allow(clippy::too_many_arguments)]
fn emit_line(
    ctx: &LayoutContext,
    runs: &[InlineRun],
    cells: &[GlyphCell],
    start: usize,
    end: usize,
    origin_x: LayoutUnit,
    line_top: LayoutUnit,
    counters: &mut LayoutCounters,
) -> Result<LineFragment, LayoutError> {
    let positions = cell_positions(cells, start, end, origin_x)?;
    let used_width = positions
        .last()
        .copied()
        .unwrap_or(origin_x)
        .checked_sub(origin_x)
        .ok_or(LayoutError::Overflow)?;

    let mut line_height = LayoutUnit::ZERO;
    let mut ascent = LayoutUnit::ZERO;
    let mut descent = LayoutUnit::ZERO;
    for cell in &cells[start..end] {
        let run = &runs[cell.run_index];
        line_height = line_height.max(run.line_height);
        ascent = ascent.max(run.ascent);
        descent = descent.max(run.descent);
    }
    let baseline = line_baseline(line_height, ascent, descent)?;

    let mut items = Vec::new();
    emit_inline_boxes(
        ctx,
        cells,
        start,
        end,
        &positions,
        line_top,
        line_height,
        &mut items,
        counters,
    )?;
    emit_text_fragments(
        runs, cells, start, end, &positions, line_top, counters, &mut items,
    )?;

    let rect = LogicalRect::new(
        LogicalPoint::new(origin_x, line_top),
        LogicalSize::new(used_width, line_height),
    );
    Ok(LineFragment::new(rect, baseline, items))
}

/// The inline start position of each cell plus the line end, from `origin_x`.
fn cell_positions(
    cells: &[GlyphCell],
    start: usize,
    end: usize,
    origin_x: LayoutUnit,
) -> Result<Vec<LayoutUnit>, LayoutError> {
    let mut positions = Vec::with_capacity(end - start + 1);
    let mut x = origin_x;
    positions.push(x);
    for cell in &cells[start..end] {
        x = x.checked_add(cell.advance).ok_or(LayoutError::Overflow)?;
        positions.push(x);
    }
    Ok(positions)
}

/// Emits the text fragments of a line, grouped by their source run.
#[allow(clippy::too_many_arguments)]
fn emit_text_fragments(
    runs: &[InlineRun],
    cells: &[GlyphCell],
    start: usize,
    end: usize,
    positions: &[LayoutUnit],
    line_top: LayoutUnit,
    counters: &mut LayoutCounters,
    items: &mut Vec<LineItem>,
) -> Result<(), LayoutError> {
    let mut cell = start;
    while cell < end {
        let run_index = cells[cell].run_index;
        let group_start = cell;
        while cell < end && cells[cell].run_index == run_index {
            cell += 1;
        }
        let first_glyph = cells[group_start].glyph_pos;
        let last_glyph = cells[cell - 1].glyph_pos;
        let slice = runs[run_index]
            .run
            .slice(first_glyph, last_glyph + 1)
            .ok_or(LayoutError::Overflow)?;
        account_text_fragment(counters)?;
        let position = LogicalPoint::new(positions[group_start - start], line_top);
        items.push(LineItem::Text(TextFragment::new(slice, position)));
    }
    Ok(())
}

/// Emits the inline-box background fragments of a line, grouped by inline box.
#[allow(clippy::too_many_arguments)]
fn emit_inline_boxes(
    ctx: &LayoutContext,
    cells: &[GlyphCell],
    start: usize,
    end: usize,
    positions: &[LayoutUnit],
    line_top: LayoutUnit,
    line_height: LayoutUnit,
    items: &mut Vec<LineItem>,
    counters: &mut LayoutCounters,
) -> Result<(), LayoutError> {
    let mut cell = start;
    while cell < end {
        let inline_box = cells[cell].inline_box;
        let group_start = cell;
        while cell < end && cells[cell].inline_box == inline_box {
            cell += 1;
        }
        let Some(node) = inline_box else {
            continue;
        };
        if !inline_box_paints(ctx, node) {
            continue;
        }

        let padding = resolve_inline_padding(ctx, node);
        let left = positions[group_start - start]
            .checked_sub(padding.left)
            .ok_or(LayoutError::Overflow)?;
        let right = positions[cell - start]
            .checked_add(padding.right)
            .ok_or(LayoutError::Overflow)?;
        let width = right.checked_sub(left).ok_or(LayoutError::Overflow)?;

        account_fragment(counters)?;
        let rect = LogicalRect::new(
            LogicalPoint::new(left, line_top),
            LogicalSize::new(width, line_height),
        );
        items.push(LineItem::InlineBox(BoxFragment::leaf(Some(node), rect)));
    }
    Ok(())
}

/// The horizontal padding of an inline box, defaulting to zero.
struct InlinePadding {
    left: LayoutUnit,
    right: LayoutUnit,
}

/// Resolves the non-negative left and right padding of an inline box.
fn resolve_inline_padding(ctx: &LayoutContext, node: NodeId) -> InlinePadding {
    let Some(style) = ctx.styles.get(node) else {
        return InlinePadding {
            left: LayoutUnit::ZERO,
            right: LayoutUnit::ZERO,
        };
    };
    InlinePadding {
        left: parse_px_length(style.get(PropertyId::PaddingLeft))
            .unwrap_or(LayoutUnit::ZERO)
            .max(LayoutUnit::ZERO),
        right: parse_px_length(style.get(PropertyId::PaddingRight))
            .unwrap_or(LayoutUnit::ZERO)
            .max(LayoutUnit::ZERO),
    }
}

/// Whether an inline box paints a background or padding worth a fragment.
fn inline_box_paints(ctx: &LayoutContext, node: NodeId) -> bool {
    let Some(style) = ctx.styles.get(node) else {
        return false;
    };
    let background = style.get(PropertyId::BackgroundColor).trim();
    let has_background = !background.is_empty() && background != "transparent";
    let padding = resolve_inline_padding(ctx, node);
    has_background || padding.left > LayoutUnit::ZERO || padding.right > LayoutUnit::ZERO
}

/// The baseline distance from the top of a line box.
///
/// The half-leading splits the extra space above and below the font, so the
/// baseline sits `ascent` below the top plus half the leading. A line height below
/// the font height yields a negative half-leading, which is valid.
fn line_baseline(
    line_height: LayoutUnit,
    ascent: LayoutUnit,
    descent: LayoutUnit,
) -> Result<LayoutUnit, LayoutError> {
    let font_height = ascent.checked_add(descent).ok_or(LayoutError::Overflow)?;
    let leading = line_height
        .checked_sub(font_height)
        .ok_or(LayoutError::Overflow)?;
    let half_leading = leading.checked_div_int(2).ok_or(LayoutError::Overflow)?;
    ascent
        .checked_add(half_leading)
        .ok_or(LayoutError::Overflow)
}

/// The checked sum of the advances of a glyph-cell range.
fn advance_sum(cells: &[GlyphCell], start: usize, end: usize) -> Result<LayoutUnit, LayoutError> {
    let mut sum = LayoutUnit::ZERO;
    for cell in &cells[start..end] {
        sum = sum.checked_add(cell.advance).ok_or(LayoutError::Overflow)?;
    }
    Ok(sum)
}

/// Counts one inline item against the inline-item cap with checked arithmetic.
pub(crate) fn account_inline_item(counters: &mut LayoutCounters) -> Result<(), LayoutError> {
    let next = counters
        .inline_items
        .checked_add(1)
        .ok_or(LayoutError::TooManyInlineItems)?;
    if next > MAX_INLINE_ITEMS {
        return Err(LayoutError::TooManyInlineItems);
    }
    counters.inline_items = next;
    Ok(())
}

/// Counts one line against the line cap with checked arithmetic.
pub(crate) fn account_line(counters: &mut LayoutCounters) -> Result<(), LayoutError> {
    let next = counters
        .lines
        .checked_add(1)
        .ok_or(LayoutError::TooManyLines)?;
    if next > MAX_LINES {
        return Err(LayoutError::TooManyLines);
    }
    counters.lines = next;
    Ok(())
}

/// Counts one text fragment against the text-fragment cap with checked arithmetic.
pub(crate) fn account_text_fragment(counters: &mut LayoutCounters) -> Result<(), LayoutError> {
    let next = counters
        .text_fragments
        .checked_add(1)
        .ok_or(LayoutError::TooManyTextFragments)?;
    if next > MAX_TEXT_FRAGMENTS {
        return Err(LayoutError::TooManyTextFragments);
    }
    counters.text_fragments = next;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_inline_item_cap_rejects_an_over_limit_count_without_panicking() {
        let mut counters = LayoutCounters::new();
        counters.inline_items = MAX_INLINE_ITEMS;
        assert_eq!(
            account_inline_item(&mut counters),
            Err(LayoutError::TooManyInlineItems)
        );
    }

    #[test]
    fn the_line_cap_rejects_an_over_limit_count_without_panicking() {
        let mut counters = LayoutCounters::new();
        counters.lines = MAX_LINES;
        assert_eq!(account_line(&mut counters), Err(LayoutError::TooManyLines));
    }

    #[test]
    fn the_text_fragment_cap_rejects_an_over_limit_count_without_panicking() {
        let mut counters = LayoutCounters::new();
        counters.text_fragments = MAX_TEXT_FRAGMENTS;
        assert_eq!(
            account_text_fragment(&mut counters),
            Err(LayoutError::TooManyTextFragments)
        );
    }

    #[test]
    fn a_unitless_line_height_scales_the_font_size() {
        let font = purr_text::BundledFont::load().expect("the bundled font parses");
        let font_size = LayoutUnit::from_px(20).expect("in range");
        let metrics = font
            .metrics(to_text_unit(font_size))
            .expect("metrics in range");

        // 1.5 * 20 px = 30 px.
        let resolved = resolve_line_height("1.5", font_size, metrics).expect("in range");
        assert_eq!(resolved, LayoutUnit::from_px(30).expect("in range"));

        // An integer multiplier and a pixel length resolve directly.
        assert_eq!(
            resolve_line_height("2", font_size, metrics).expect("in range"),
            LayoutUnit::from_px(40).expect("in range")
        );
        assert_eq!(
            resolve_line_height("24px", font_size, metrics).expect("in range"),
            LayoutUnit::from_px(24).expect("in range")
        );
        // `normal` falls back to the font default line height.
        assert_eq!(
            resolve_line_height("normal", font_size, metrics).expect("in range"),
            to_layout_unit(metrics.line_height())
        );
    }
}
