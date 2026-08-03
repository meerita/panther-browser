// @file engines/purr/text/src/bundled-font.rs
// @description Loads the bundled font, parses the M2 tables with hardened fail-closed parsing, and exposes a capability handle.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Bundled font access.
//!
//! The engine ships one font embedded in the binary. There is no filesystem read
//! and no font enumeration: the only way to reach a font is [`BundledFont::load`],
//! which returns a capability handle (an id plus generation) with `Restricted`
//! visibility, never a path and never a value the DOM can observe. This keeps the
//! font surface off the fingerprinting entropy budget.
//!
//! Parsing is hardened and fail-closed. Every table offset and length is bounded
//! against the file, every read is range-checked, and every size calculation uses
//! checked arithmetic. Malformed input returns a typed [`FontError`] and never
//! panics. The parser reads only the tables the M2 path needs: the header metrics
//! (`head`, `hhea`, `maxp`), the horizontal advances (`hmtx`), the Unicode `cmap`,
//! and the outline tables (`glyf`, `loca`). It keeps the `glyf` bytes and the
//! resolved `loca` offsets so [`BundledFont::outline`] decodes one glyph on demand
//! for the rasterizer; the outline itself is decoded per glyph, not on load.

// Font metrics and advances are consumed by the shaping adapter and, from a later
// phase, by inline layout. This phase adds the loader and exercises it through the
// unit tests below, so a few accessors are otherwise unused in a non-test build.
#![allow(dead_code)]

use crate::pixel_unit::TextUnit;
use memory::{AccountingRegistry, ByteCount, Region};

/// Upper bound for the number of tables in the font directory.
///
/// A real font has a few dozen tables. The bound rejects a malformed directory
/// that claims an implausible table count before any allocation.
const MAX_TABLE_COUNT: usize = 4_096;

/// Upper bound for the number of points in one simple glyph.
///
/// A real Latin glyph has at most a few hundred points. The bound rejects a
/// malformed glyph that claims an implausible point count before any allocation.
const MAX_GLYPH_POINTS: usize = 20_000;

// Simple-glyph flag bits, from the TrueType `glyf` table format.
const FLAG_ON_CURVE: u8 = 0x01;
const FLAG_X_SHORT: u8 = 0x02;
const FLAG_Y_SHORT: u8 = 0x04;
const FLAG_REPEAT: u8 = 0x08;
const FLAG_X_SAME_OR_POSITIVE: u8 = 0x10;
const FLAG_Y_SAME_OR_POSITIVE: u8 = 0x20;

/// The bundled font bytes, embedded in the binary. No filesystem read.
static FONT_BYTES: &[u8] = include_bytes!("../resources/katex-typewriter-regular.ttf");

/// The handle of the single bundled font.
///
/// The id and generation are fixed, so the bundled handle is deterministic across
/// runs. A later milestone that manages several faces advances the generation on
/// replacement.
const BUNDLED_HANDLE: FontHandle = FontHandle {
    id: FontFaceId(1),
    generation: FontFaceGeneration(1),
};

/// Stable identity of one font face.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FontFaceId(u32);

impl FontFaceId {
    pub fn value(self) -> u32 {
        self.0
    }
}

/// Generation of one font face.
///
/// A replacement of the underlying font data advances the generation, so a handle
/// to a superseded face stops matching. The bundled font never changes at M2, so
/// its generation is constant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FontFaceGeneration(u64);

impl FontFaceGeneration {
    pub fn value(self) -> u64 {
        self.0
    }
}

/// A capability handle to a font face.
///
/// The handle is an id plus generation, never a path and never DOM-visible. A
/// consumer names a font only through this handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FontHandle {
    id: FontFaceId,
    generation: FontFaceGeneration,
}

impl FontHandle {
    pub fn id(self) -> FontFaceId {
        self.id
    }

    pub fn generation(self) -> FontFaceGeneration {
        self.generation
    }
}

/// The visibility scope of a font.
///
/// At M2 the only scope is `Restricted`: bundled fonts only, no enumeration of the
/// platform environment. Broader scopes are a later, permission-gated concern.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontVisibility {
    Restricted,
}

/// The identity of one glyph in a font.
///
/// A glyph index is a position in the font's glyph set. It is never a text index
/// or a code point: the newtype keeps the two from being interchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlyphIndex(u16);

impl GlyphIndex {
    /// The `.notdef` glyph, index zero, used when a code point has no glyph.
    pub const NOTDEF: Self = Self(0);

    pub fn new(value: u16) -> Self {
        Self(value)
    }

    pub fn value(self) -> u16 {
        self.0
    }
}

/// One point of a glyph outline, in font design units.
///
/// The coordinates are integers in the font grid (`units_per_em`). A point is
/// either on the curve (a contour vertex) or off the curve (a quadratic control
/// point); the rasterizer flattens the off-curve control points into segments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutlinePoint {
    pub x: i32,
    pub y: i32,
    pub on_curve: bool,
}

/// A decoded glyph outline in font design units.
///
/// The outline is a list of closed contours, each an ordered list of points in
/// font grid units. An empty outline (no contours) is valid: a space glyph and an
/// unsupported composite glyph both decode to no contours, so they rasterize to a
/// blank mask. The outline holds no pixels; scaling and filling belong to the
/// rasterizer.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GlyphOutline {
    contours: Vec<Vec<OutlinePoint>>,
}

impl GlyphOutline {
    /// The closed contours of the outline, in font design units.
    pub fn contours(&self) -> &[Vec<OutlinePoint>] {
        &self.contours
    }

    /// Whether the outline has no contours (a blank glyph).
    pub fn is_empty(&self) -> bool {
        self.contours.is_empty()
    }
}

/// Font metrics for one pixel size, in fixed-point `TextUnit`.
///
/// Ascent and descent are positive distances from the baseline (up and down). The
/// line height is the derived default: ascent plus descent plus the font line gap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FontMetrics {
    ascent: TextUnit,
    descent: TextUnit,
    line_height: TextUnit,
}

impl FontMetrics {
    pub fn ascent(self) -> TextUnit {
        self.ascent
    }

    pub fn descent(self) -> TextUnit {
        self.descent
    }

    pub fn line_height(self) -> TextUnit {
        self.line_height
    }
}

/// Failure the font parser reports.
///
/// Each variant is a static, factual, non-secret message. The parser fails closed
/// on any malformed input rather than trusting a declared size.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum FontError {
    #[error("the font directory is malformed")]
    MalformedDirectory,
    #[error("a required font table is missing")]
    MissingTable,
    #[error("a font table is malformed or truncated")]
    MalformedTable,
    #[error("the font has an invalid units-per-em value")]
    InvalidUnitsPerEm,
    #[error("the font horizontal metrics are inconsistent")]
    InvalidHorizontalMetrics,
    #[error("the font has no supported Unicode cmap subtable")]
    UnsupportedCmap,
}

/// The bundled font, with the parsed metrics, advances, and Unicode cmap.
///
/// All fields are owned and immutable after loading. The struct retains no raw
/// byte slice: it holds only the parsed data the M2 path needs. The owned parsed
/// data is accounted to the Fonts memory region.
pub struct BundledFont {
    handle: FontHandle,
    visibility: FontVisibility,
    units_per_em: u16,
    ascent_font_units: i16,
    descent_font_units: i16,
    line_gap_font_units: i16,
    glyph_count: u16,
    horizontal_metric_count: u16,
    advances: Vec<u16>,
    cmap: CmapSubtable,
    glyf: Vec<u8>,
    loca: Vec<u32>,
    accounting: AccountingRegistry,
}

impl BundledFont {
    /// Loads the single bundled font.
    ///
    /// This is the only font source. It never reads the filesystem and never
    /// enumerates the platform environment.
    pub fn load() -> Result<Self, FontError> {
        parse(FONT_BYTES, BUNDLED_HANDLE, FontVisibility::Restricted)
    }

    /// The capability handle of this font.
    pub fn handle(&self) -> FontHandle {
        self.handle
    }

    /// The visibility scope of this font.
    pub fn visibility(&self) -> FontVisibility {
        self.visibility
    }

    /// The font design grid size (font units per em).
    pub fn units_per_em(&self) -> u16 {
        self.units_per_em
    }

    /// The resident bytes the parsed font data holds in the Fonts region.
    pub fn resident_font_bytes(&self) -> ByteCount {
        self.accounting.view().region(Region::Fonts).resident
    }

    /// The glyph a code point maps to, or [`GlyphIndex::NOTDEF`] when the font has
    /// no glyph for it. The mapping is deterministic.
    pub fn glyph_for(&self, character: char) -> GlyphIndex {
        GlyphIndex(self.cmap.glyph(character as u32))
    }

    /// The horizontal advance of a glyph at a pixel size, or `None` when the glyph
    /// index is out of range or the scaled value overflows.
    pub fn advance(&self, glyph: GlyphIndex, size: TextUnit) -> Option<TextUnit> {
        let index = glyph.value();
        if index >= self.glyph_count {
            return None;
        }
        let last = self.horizontal_metric_count - 1;
        let advance_index = index.min(last) as usize;
        let advance_font_units = *self.advances.get(advance_index)?;
        scale_font_units(i32::from(advance_font_units), size, self.units_per_em)
    }

    /// The glyph count of the font.
    pub fn glyph_count(&self) -> u16 {
        self.glyph_count
    }

    /// The decoded outline of a glyph, in font design units.
    ///
    /// An out-of-range glyph index, an empty glyph (such as a space), and an
    /// unsupported composite glyph all decode to an empty outline, so they
    /// rasterize to a blank mask. A malformed `glyf` entry fails closed with a
    /// typed error and never panics. The decoding is deterministic.
    pub fn outline(&self, glyph: GlyphIndex) -> Result<GlyphOutline, FontError> {
        let index = glyph.value() as usize;
        if index >= self.glyph_count as usize {
            return Ok(GlyphOutline::default());
        }

        let start = *self.loca.get(index).ok_or(FontError::MalformedTable)? as usize;
        let end = *self.loca.get(index + 1).ok_or(FontError::MalformedTable)? as usize;
        if end <= start {
            return Ok(GlyphOutline::default());
        }

        let data = self.glyf.get(start..end).ok_or(FontError::MalformedTable)?;
        parse_simple_glyph(data)
    }

    /// The font metrics at a pixel size, or `None` when a scaled value overflows.
    ///
    /// The derived line height is ascent plus descent plus the font line gap.
    pub fn metrics(&self, size: TextUnit) -> Option<FontMetrics> {
        let ascent = scale_font_units(i32::from(self.ascent_font_units), size, self.units_per_em)?;
        let descent_up = i32::from(self.descent_font_units).abs();
        let descent = scale_font_units(descent_up, size, self.units_per_em)?;
        let line_gap_units = i32::from(self.line_gap_font_units).max(0);
        let line_gap = scale_font_units(line_gap_units, size, self.units_per_em)?;
        let line_height = ascent.checked_add(descent)?.checked_add(line_gap)?;
        Some(FontMetrics {
            ascent,
            descent,
            line_height,
        })
    }
}

/// Scales a font-unit length to a fixed-point pixel length.
///
/// The pixel length is `units * size / units_per_em`. The math stays integer-only
/// through `TextUnit`, so the result is deterministic. Returns `None` when the
/// value does not fit the fixed-point range.
fn scale_font_units(units: i32, size: TextUnit, units_per_em: u16) -> Option<TextUnit> {
    let numerator = i64::from(units) * i64::from(size.raw());
    TextUnit::from_raw_ratio(numerator, i64::from(units_per_em))
}

/// A parsed Unicode `cmap` format-4 subtable.
///
/// The subtable maps a Basic Multilingual Plane code point to a glyph index. The
/// segment arrays are validated at parse time; lookup uses only checked indexing.
struct CmapSubtable {
    end_codes: Vec<u16>,
    start_codes: Vec<u16>,
    id_deltas: Vec<i16>,
    id_range_offsets: Vec<u16>,
    glyph_id_array: Vec<u16>,
}

impl CmapSubtable {
    /// The glyph a code point maps to, or zero (`.notdef`) when it is unmapped.
    ///
    /// Follows the format-4 lookup: locate the segment covering the code point,
    /// then resolve the glyph through the delta or the glyph-index array. Every
    /// index is bounds-checked, so a malformed table can only yield `.notdef`.
    fn glyph(&self, code_point: u32) -> u16 {
        if code_point > 0xFFFF {
            return 0;
        }
        let code = code_point as u16;

        let segment = match self.end_codes.binary_search(&code) {
            Ok(index) => index,
            Err(index) => index,
        };
        let Some(&start) = self.start_codes.get(segment) else {
            return 0;
        };
        if code < start {
            return 0;
        }

        let id_delta = self.id_deltas[segment];
        let id_range_offset = self.id_range_offsets[segment];
        if id_range_offset == 0 {
            return (i32::from(code) + i32::from(id_delta)) as u16;
        }

        let Some(offset_words) = (id_range_offset as usize)
            .checked_div(2)
            .and_then(|words| words.checked_add((code - start) as usize))
        else {
            return 0;
        };
        let Some(combined_index) = segment.checked_add(offset_words) else {
            return 0;
        };

        let raw_glyph = if combined_index < self.id_range_offsets.len() {
            self.id_range_offsets[combined_index]
        } else {
            match self
                .glyph_id_array
                .get(combined_index - self.id_range_offsets.len())
            {
                Some(&glyph) => glyph,
                None => return 0,
            }
        };
        if raw_glyph == 0 {
            0
        } else {
            (i32::from(raw_glyph) + i32::from(id_delta)) as u16
        }
    }
}

/// One entry of the font table directory.
struct TableRecord {
    offset: usize,
    length: usize,
}

/// Parses a font into the M2 data set, or fails closed.
///
/// The handle and visibility are supplied by the caller so the bundled loader and
/// the tests share one parser. The parser trusts no declared length: every read is
/// bounded against `bytes`.
fn parse(
    bytes: &[u8],
    handle: FontHandle,
    visibility: FontVisibility,
) -> Result<BundledFont, FontError> {
    let table_count = read_u16(bytes, 4).ok_or(FontError::MalformedDirectory)? as usize;
    if table_count > MAX_TABLE_COUNT {
        return Err(FontError::MalformedDirectory);
    }
    let directory_end = 12usize
        .checked_add(
            table_count
                .checked_mul(16)
                .ok_or(FontError::MalformedDirectory)?,
        )
        .ok_or(FontError::MalformedDirectory)?;
    if directory_end > bytes.len() {
        return Err(FontError::MalformedDirectory);
    }

    let mut head = None;
    let mut hhea = None;
    let mut maxp = None;
    let mut hmtx = None;
    let mut cmap = None;
    let mut glyf = None;
    let mut loca = None;

    for index in 0..table_count {
        let record_offset = 12 + index * 16;
        let tag = bytes
            .get(record_offset..record_offset + 4)
            .ok_or(FontError::MalformedDirectory)?;
        let offset =
            read_u32(bytes, record_offset + 8).ok_or(FontError::MalformedDirectory)? as usize;
        let length =
            read_u32(bytes, record_offset + 12).ok_or(FontError::MalformedDirectory)? as usize;
        let end = offset
            .checked_add(length)
            .ok_or(FontError::MalformedDirectory)?;
        if end > bytes.len() {
            return Err(FontError::MalformedDirectory);
        }

        let record = TableRecord { offset, length };
        match tag {
            b"head" => head = Some(record),
            b"hhea" => hhea = Some(record),
            b"maxp" => maxp = Some(record),
            b"hmtx" => hmtx = Some(record),
            b"cmap" => cmap = Some(record),
            b"glyf" => glyf = Some(record),
            b"loca" => loca = Some(record),
            _ => {}
        }
    }

    let head = head.ok_or(FontError::MissingTable)?;
    let hhea = hhea.ok_or(FontError::MissingTable)?;
    let maxp = maxp.ok_or(FontError::MissingTable)?;
    let hmtx = hmtx.ok_or(FontError::MissingTable)?;
    let cmap = cmap.ok_or(FontError::MissingTable)?;
    let glyf = glyf.ok_or(FontError::MissingTable)?;
    let loca = loca.ok_or(FontError::MissingTable)?;

    if head.length < 54 {
        return Err(FontError::MalformedTable);
    }
    let units_per_em = read_u16(bytes, head.offset + 18).ok_or(FontError::MalformedTable)?;
    if units_per_em == 0 {
        return Err(FontError::InvalidUnitsPerEm);
    }
    let index_to_loc_format = read_i16(bytes, head.offset + 50).ok_or(FontError::MalformedTable)?;

    if hhea.length < 36 {
        return Err(FontError::MalformedTable);
    }
    let ascent_font_units = read_i16(bytes, hhea.offset + 4).ok_or(FontError::MalformedTable)?;
    let descent_font_units = read_i16(bytes, hhea.offset + 6).ok_or(FontError::MalformedTable)?;
    let line_gap_font_units = read_i16(bytes, hhea.offset + 8).ok_or(FontError::MalformedTable)?;
    let horizontal_metric_count =
        read_u16(bytes, hhea.offset + 34).ok_or(FontError::MalformedTable)?;

    if maxp.length < 6 {
        return Err(FontError::MalformedTable);
    }
    let glyph_count = read_u16(bytes, maxp.offset + 4).ok_or(FontError::MalformedTable)?;

    if horizontal_metric_count == 0 || horizontal_metric_count > glyph_count {
        return Err(FontError::InvalidHorizontalMetrics);
    }
    let advances = parse_advances(bytes, &hmtx, horizontal_metric_count)?;
    let cmap = parse_cmap(bytes, &cmap)?;

    let loca_offsets = parse_loca(bytes, &loca, glyph_count, index_to_loc_format)?;
    let glyf_bytes = bytes
        .get(glyf.offset..glyf.offset + glyf.length)
        .ok_or(FontError::MalformedTable)?
        .to_vec();
    if let Some(&last) = loca_offsets.last()
        && last as usize > glyf_bytes.len()
    {
        return Err(FontError::MalformedTable);
    }

    let accounting = AccountingRegistry::new();
    accounting.record_allocation(Region::Fonts, parsed_data_bytes(&advances, &cmap));
    accounting.record_allocation(
        Region::Fonts,
        outline_data_bytes(&glyf_bytes, &loca_offsets),
    );

    Ok(BundledFont {
        handle,
        visibility,
        units_per_em,
        ascent_font_units,
        descent_font_units,
        line_gap_font_units,
        glyph_count,
        horizontal_metric_count,
        advances,
        cmap,
        glyf: glyf_bytes,
        loca: loca_offsets,
        accounting,
    })
}

/// Reads the glyph location offsets from the `loca` table.
///
/// The short format stores half-offsets (multiplied by two); the long format
/// stores byte offsets directly. The table must be long enough for one entry per
/// glyph plus a terminating entry, and the offsets must not decrease, or parsing
/// fails closed.
fn parse_loca(
    bytes: &[u8],
    loca: &TableRecord,
    glyph_count: u16,
    index_to_loc_format: i16,
) -> Result<Vec<u32>, FontError> {
    let entry_count = (glyph_count as usize)
        .checked_add(1)
        .ok_or(FontError::MalformedTable)?;
    let (entry_size, is_long) = match index_to_loc_format {
        0 => (2usize, false),
        1 => (4usize, true),
        _ => return Err(FontError::MalformedTable),
    };
    let needed = entry_count
        .checked_mul(entry_size)
        .ok_or(FontError::MalformedTable)?;
    if needed > loca.length {
        return Err(FontError::MalformedTable);
    }

    let mut offsets = Vec::with_capacity(entry_count);
    for index in 0..entry_count {
        let element = loca.offset + index * entry_size;
        let value = if is_long {
            read_u32(bytes, element).ok_or(FontError::MalformedTable)?
        } else {
            let half = read_u16(bytes, element).ok_or(FontError::MalformedTable)? as u32;
            half.checked_mul(2).ok_or(FontError::MalformedTable)?
        };
        offsets.push(value);
    }

    if offsets.windows(2).any(|pair| pair[1] < pair[0]) {
        return Err(FontError::MalformedTable);
    }
    Ok(offsets)
}

/// Decodes one simple glyph into contours of points in font design units.
///
/// The parser follows the TrueType `glyf` simple-glyph format: the contour end
/// points, the flag run with its repeat encoding, then the delta-encoded x and y
/// coordinates. Every read is bounded against the glyph data, every coordinate
/// accumulation is checked, and the point count is capped, so malformed data fails
/// closed without a panic. A composite glyph (a negative contour count) is not
/// decoded at M2 and yields an empty outline.
fn parse_simple_glyph(data: &[u8]) -> Result<GlyphOutline, FontError> {
    let contour_count = read_i16(data, 0).ok_or(FontError::MalformedTable)?;
    if contour_count <= 0 {
        return Ok(GlyphOutline::default());
    }
    let contour_count = contour_count as usize;

    let mut end_points = Vec::with_capacity(contour_count);
    for contour in 0..contour_count {
        let element = 10 + contour * 2;
        end_points.push(read_u16(data, element).ok_or(FontError::MalformedTable)? as usize);
    }
    let last_point = *end_points.last().ok_or(FontError::MalformedTable)?;
    let point_count = last_point
        .checked_add(1)
        .filter(|&count| count <= MAX_GLYPH_POINTS)
        .ok_or(FontError::MalformedTable)?;

    let instruction_length_offset = 10 + contour_count * 2;
    let instruction_length =
        read_u16(data, instruction_length_offset).ok_or(FontError::MalformedTable)? as usize;
    let mut offset = instruction_length_offset
        .checked_add(2)
        .and_then(|value| value.checked_add(instruction_length))
        .ok_or(FontError::MalformedTable)?;

    let flags = read_glyph_flags(data, &mut offset, point_count)?;
    let xs = read_glyph_coordinates(
        data,
        &mut offset,
        &flags,
        FLAG_X_SHORT,
        FLAG_X_SAME_OR_POSITIVE,
    )?;
    let ys = read_glyph_coordinates(
        data,
        &mut offset,
        &flags,
        FLAG_Y_SHORT,
        FLAG_Y_SAME_OR_POSITIVE,
    )?;

    let mut contours = Vec::with_capacity(contour_count);
    let mut start = 0usize;
    for &end in &end_points {
        if end < start || end >= point_count {
            return Err(FontError::MalformedTable);
        }
        let mut contour = Vec::with_capacity(end - start + 1);
        for point in start..=end {
            contour.push(OutlinePoint {
                x: xs[point],
                y: ys[point],
                on_curve: flags[point] & FLAG_ON_CURVE != 0,
            });
        }
        contours.push(contour);
        start = end + 1;
    }
    Ok(GlyphOutline { contours })
}

/// Reads the flag run of a simple glyph, expanding the repeat encoding.
fn read_glyph_flags(
    data: &[u8],
    offset: &mut usize,
    point_count: usize,
) -> Result<Vec<u8>, FontError> {
    let mut flags = Vec::with_capacity(point_count);
    while flags.len() < point_count {
        let flag = *data.get(*offset).ok_or(FontError::MalformedTable)?;
        *offset += 1;
        flags.push(flag);
        if flag & FLAG_REPEAT != 0 {
            let repeat = *data.get(*offset).ok_or(FontError::MalformedTable)?;
            *offset += 1;
            for _ in 0..repeat {
                if flags.len() >= point_count {
                    break;
                }
                flags.push(flag);
            }
        }
    }
    Ok(flags)
}

/// Reads one delta-encoded coordinate axis of a simple glyph.
///
/// A short coordinate is one byte with its sign in the same-or-positive flag; a
/// long coordinate is a signed 16-bit delta; a cleared short flag with a set
/// same-or-positive flag repeats the previous value. Each accumulation is checked.
fn read_glyph_coordinates(
    data: &[u8],
    offset: &mut usize,
    flags: &[u8],
    short_flag: u8,
    same_or_positive_flag: u8,
) -> Result<Vec<i32>, FontError> {
    let mut coordinates = Vec::with_capacity(flags.len());
    let mut value = 0i32;
    for &flag in flags {
        if flag & short_flag != 0 {
            let delta = *data.get(*offset).ok_or(FontError::MalformedTable)? as i32;
            *offset += 1;
            let signed = if flag & same_or_positive_flag != 0 {
                delta
            } else {
                -delta
            };
            value = value.checked_add(signed).ok_or(FontError::MalformedTable)?;
        } else if flag & same_or_positive_flag == 0 {
            let delta = read_i16(data, *offset).ok_or(FontError::MalformedTable)? as i32;
            *offset += 2;
            value = value.checked_add(delta).ok_or(FontError::MalformedTable)?;
        }
        coordinates.push(value);
    }
    Ok(coordinates)
}

/// The resident size of the owned parsed font data, in bytes.
///
/// The advances and the cmap segment arrays are the heap allocations the font
/// owns. Their size is accounted to the Fonts region.
fn parsed_data_bytes(advances: &[u16], cmap: &CmapSubtable) -> ByteCount {
    let element_count = advances.len()
        + cmap.end_codes.len()
        + cmap.start_codes.len()
        + cmap.id_deltas.len()
        + cmap.id_range_offsets.len()
        + cmap.glyph_id_array.len();
    ByteCount::new(element_count as u64 * 2)
}

/// The resident size of the owned outline data, in bytes.
///
/// The glyf bytes and the resolved loca offsets are the outline allocations the
/// font owns. Their size is accounted to the Fonts region.
fn outline_data_bytes(glyf: &[u8], loca: &[u32]) -> ByteCount {
    let bytes = glyf.len() as u64 + loca.len() as u64 * 4;
    ByteCount::new(bytes)
}

/// Reads the per-glyph advance widths from `hmtx`.
///
/// Only the first `metric_count` glyphs carry an advance; later glyphs reuse the
/// last one. The table must be long enough for the metrics, or parsing fails.
fn parse_advances(
    bytes: &[u8],
    hmtx: &TableRecord,
    metric_count: u16,
) -> Result<Vec<u16>, FontError> {
    let needed = (metric_count as usize)
        .checked_mul(4)
        .ok_or(FontError::MalformedTable)?;
    if needed > hmtx.length {
        return Err(FontError::MalformedTable);
    }

    let mut advances = Vec::with_capacity(metric_count as usize);
    for index in 0..metric_count as usize {
        let offset = hmtx.offset + index * 4;
        advances.push(read_u16(bytes, offset).ok_or(FontError::MalformedTable)?);
    }
    Ok(advances)
}

/// Parses the first Unicode format-4 subtable of the `cmap` table.
///
/// The `cmap` header lists encoding records; the parser selects a Unicode record
/// whose subtable is format 4 and validates every segment array against the table
/// bounds. A `cmap` without such a subtable fails closed.
fn parse_cmap(bytes: &[u8], cmap: &TableRecord) -> Result<CmapSubtable, FontError> {
    let base = cmap.offset;
    let record_count = read_u16(bytes, base + 2).ok_or(FontError::MalformedTable)? as usize;

    for index in 0..record_count {
        let record_offset = base
            .checked_add(4)
            .and_then(|value| value.checked_add(index.checked_mul(8)?))
            .ok_or(FontError::MalformedTable)?;
        let platform = read_u16(bytes, record_offset).ok_or(FontError::MalformedTable)?;
        let encoding = read_u16(bytes, record_offset + 2).ok_or(FontError::MalformedTable)?;
        if !is_unicode_encoding(platform, encoding) {
            continue;
        }

        let subtable_relative =
            read_u32(bytes, record_offset + 4).ok_or(FontError::MalformedTable)? as usize;
        let subtable_offset = base
            .checked_add(subtable_relative)
            .ok_or(FontError::MalformedTable)?;
        let format = read_u16(bytes, subtable_offset).ok_or(FontError::MalformedTable)?;
        if format != 4 {
            continue;
        }
        return parse_cmap_format4(bytes, subtable_offset);
    }

    Err(FontError::UnsupportedCmap)
}

/// Whether a platform/encoding pair names a Unicode Basic Multilingual Plane cmap.
fn is_unicode_encoding(platform: u16, encoding: u16) -> bool {
    // Platform 0 (Unicode) any BMP encoding, or platform 3 (Windows) encoding 1
    // (Unicode BMP).
    platform == 0 || (platform == 3 && encoding == 1)
}

/// Parses a format-4 cmap subtable with bounded, checked segment arrays.
fn parse_cmap_format4(bytes: &[u8], offset: usize) -> Result<CmapSubtable, FontError> {
    let length = read_u16(bytes, offset + 2).ok_or(FontError::MalformedTable)? as usize;
    let subtable_end = offset
        .checked_add(length)
        .ok_or(FontError::MalformedTable)?;
    if length < 16 || subtable_end > bytes.len() {
        return Err(FontError::MalformedTable);
    }

    let segment_count_doubled =
        read_u16(bytes, offset + 6).ok_or(FontError::MalformedTable)? as usize;
    if segment_count_doubled == 0 || !segment_count_doubled.is_multiple_of(2) {
        return Err(FontError::MalformedTable);
    }
    let segment_count = segment_count_doubled / 2;

    let end_codes_offset = offset + 14;
    let start_codes_offset = end_codes_offset + segment_count_doubled + 2;
    let id_deltas_offset = start_codes_offset + segment_count_doubled;
    let id_range_offsets_offset = id_deltas_offset + segment_count_doubled;
    let glyph_id_array_offset = id_range_offsets_offset + segment_count_doubled;
    if glyph_id_array_offset > subtable_end {
        return Err(FontError::MalformedTable);
    }

    let end_codes = read_u16_array(bytes, end_codes_offset, segment_count)?;
    let start_codes = read_u16_array(bytes, start_codes_offset, segment_count)?;
    let id_deltas = read_i16_array(bytes, id_deltas_offset, segment_count)?;
    let id_range_offsets = read_u16_array(bytes, id_range_offsets_offset, segment_count)?;

    if end_codes.last() != Some(&0xFFFF) {
        return Err(FontError::MalformedTable);
    }

    let glyph_id_count = (subtable_end - glyph_id_array_offset) / 2;
    let glyph_id_array = read_u16_array(bytes, glyph_id_array_offset, glyph_id_count)?;

    Ok(CmapSubtable {
        end_codes,
        start_codes,
        id_deltas,
        id_range_offsets,
        glyph_id_array,
    })
}

fn read_u16_array(bytes: &[u8], offset: usize, count: usize) -> Result<Vec<u16>, FontError> {
    let mut values = Vec::with_capacity(count);
    for index in 0..count {
        let element = offset
            .checked_add(index.checked_mul(2).ok_or(FontError::MalformedTable)?)
            .ok_or(FontError::MalformedTable)?;
        values.push(read_u16(bytes, element).ok_or(FontError::MalformedTable)?);
    }
    Ok(values)
}

fn read_i16_array(bytes: &[u8], offset: usize, count: usize) -> Result<Vec<i16>, FontError> {
    let mut values = Vec::with_capacity(count);
    for index in 0..count {
        let element = offset
            .checked_add(index.checked_mul(2).ok_or(FontError::MalformedTable)?)
            .ok_or(FontError::MalformedTable)?;
        values.push(read_i16(bytes, element).ok_or(FontError::MalformedTable)?);
    }
    Ok(values)
}

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    let end = offset.checked_add(2)?;
    let slice = bytes.get(offset..end)?;
    Some(u16::from_be_bytes([slice[0], slice[1]]))
}

fn read_i16(bytes: &[u8], offset: usize) -> Option<i16> {
    read_u16(bytes, offset).map(|value| value as i16)
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let end = offset.checked_add(4)?;
    let slice = bytes.get(offset..end)?;
    Some(u32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_font_loads_with_a_restricted_handle() {
        let font = BundledFont::load().expect("the bundled font parses");
        assert_eq!(font.visibility(), FontVisibility::Restricted);
        assert_eq!(font.handle().id().value(), 1);
        assert_eq!(font.handle().generation().value(), 1);
        assert_eq!(font.units_per_em(), 1000);
    }

    #[test]
    fn parsed_font_data_is_accounted_to_the_fonts_region() {
        let font = BundledFont::load().expect("the bundled font parses");
        assert!(font.resident_font_bytes().get() > 0);
    }

    #[test]
    fn cmap_maps_latin_code_points_to_distinct_glyphs() {
        let font = BundledFont::load().expect("the bundled font parses");
        assert_eq!(font.glyph_for('A').value(), 36);
        assert_eq!(font.glyph_for('a').value(), 68);
        assert_eq!(font.glyph_for(' ').value(), 3);
        assert_eq!(font.glyph_for('0').value(), 19);
    }

    #[test]
    fn advances_scale_from_font_units_to_layout_units() {
        let font = BundledFont::load().expect("the bundled font parses");
        let size = TextUnit::from_px(16).expect("in range");
        let advance = font
            .advance(font.glyph_for('A'), size)
            .expect("the glyph has an advance");
        // 525 font units at 16 px over 1000 upm = 8.4 px = 537.6/64 rounded to 538.
        assert_eq!(advance.raw(), 538);
    }

    #[test]
    fn font_metrics_are_positive_layout_units() {
        let font = BundledFont::load().expect("the bundled font parses");
        let size = TextUnit::from_px(16).expect("in range");
        let metrics = font.metrics(size).expect("metrics scale in range");

        assert!(metrics.ascent().raw() > 0);
        assert!(metrics.descent().raw() > 0);
        assert!(metrics.line_height().raw() > 0);
        assert_eq!(metrics.ascent().raw(), 711);
        assert_eq!(metrics.descent().raw(), 234);
        assert_eq!(metrics.line_height().raw(), 1037);
    }

    #[test]
    fn a_letter_glyph_decodes_to_a_non_empty_outline() {
        let font = BundledFont::load().expect("the bundled font parses");
        let outline = font
            .outline(font.glyph_for('A'))
            .expect("the glyph decodes");
        assert!(!outline.is_empty());
        assert_eq!(outline.contours().len(), 2);
        assert!(outline.contours().iter().all(|contour| !contour.is_empty()));
    }

    #[test]
    fn a_space_glyph_decodes_to_an_empty_outline() {
        let font = BundledFont::load().expect("the bundled font parses");
        let outline = font
            .outline(font.glyph_for(' '))
            .expect("the glyph decodes");
        assert!(outline.is_empty());
    }

    #[test]
    fn an_out_of_range_glyph_decodes_to_an_empty_outline() {
        let font = BundledFont::load().expect("the bundled font parses");
        let outline = font
            .outline(GlyphIndex::new(u16::MAX))
            .expect("an out-of-range glyph is blank");
        assert!(outline.is_empty());
    }

    #[test]
    fn an_out_of_range_glyph_has_no_advance() {
        let font = BundledFont::load().expect("the bundled font parses");
        let size = TextUnit::from_px(16).expect("in range");
        assert_eq!(font.advance(GlyphIndex::new(u16::MAX), size), None);
    }

    #[test]
    fn truncated_font_input_fails_closed_without_panic() {
        let truncated = &FONT_BYTES[..32];
        let result = parse(truncated, BUNDLED_HANDLE, FontVisibility::Restricted);
        assert!(result.is_err());
    }

    #[test]
    fn a_directory_claiming_too_many_tables_fails_closed() {
        let mut bytes = FONT_BYTES.to_vec();
        // Overwrite the table count with an implausible value.
        bytes[4] = 0xFF;
        bytes[5] = 0xFF;
        let result = parse(&bytes, BUNDLED_HANDLE, FontVisibility::Restricted);
        assert!(matches!(result, Err(FontError::MalformedDirectory)));
    }

    #[test]
    fn empty_input_fails_closed() {
        let result = parse(&[], BUNDLED_HANDLE, FontVisibility::Restricted);
        assert!(result.is_err());
    }
}
