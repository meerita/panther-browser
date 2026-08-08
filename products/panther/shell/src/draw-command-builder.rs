// @file products/panther/shell/src/draw-command-builder.rs
// @description Builds the ordered draw-command list that paints the shell chrome.
// @created Diego Martín Lafuente <meerita@icloud.com>

use purr_graphics::{DrawCommand, Rect};
use purr_text::{ONE_PX_RAW, PlacedGlyphRun, TextUnit};

use crate::labels::LabelView;
use crate::region_layout::RegionLayout;
use crate::scale_factor::ScaleFactor;
use crate::shell_region::{
    ACTIVE_TAB_COLOR, CLEAR_COLOR, CLOSE_COLOR, INACTIVE_TAB_COLOR, NEW_TAB_COLOR, ShellRegion,
};
use crate::tab_strip::{TabStripView, strip_layout};

/// Inset from the address-field left edge for left-aligned label text.
///
/// It keeps the placeholder text off the field border. It is a logical length;
/// the paint seam scales it to physical pixels by the active scale.
const ADDRESS_TEXT_INSET: f32 = 8.0;

/// Horizontal alignment of a label run inside its region.
enum TextAlignment {
    Left,
    Centered,
}

/// Builds the ordered colored-rectangle list that paints the chrome.
///
/// The layout is logical; the builder is the single paint seam that scales every
/// emitted rectangle to physical pixels by `scale`, so every fixed chrome
/// dimension is correct on any display density (D1). The list starts with one
/// `Clear` to the background, then one `FillRect` per region in `ShellRegion::ALL`
/// order (top bar first, viewport last), so a control paints over the top bar band
/// it sits in. The hovered and focused regions take the hover and focus color, so
/// the interaction state is visible (D1, D3). The `Tab` band then holds the
/// dynamic strip: one fill per slot (the active slot highlighted), one fill per
/// close sub-rect, and the new-tab button, all painted over the band. The label
/// runs paint last, one `TexturedQuad` per glyph, so the chrome text draws over
/// the region fills. The capacity is reserved from the fixed region count, the
/// bounded strip element count, and the total placed glyph count, so the list
/// never reallocates.
pub fn build_commands(
    layout: &RegionLayout,
    hovered: Option<ShellRegion>,
    focused: Option<ShellRegion>,
    tabs: TabStripView,
    labels: &LabelView,
    scale: ScaleFactor,
) -> Vec<DrawCommand> {
    let glyph_count: usize = labels
        .runs()
        .iter()
        .map(|(_, run)| run.glyphs().len())
        .sum();
    let mut commands =
        Vec::with_capacity(ShellRegion::ALL.len() + 2 + 2 * tabs.tab_count() + glyph_count);

    commands.push(DrawCommand::Clear { color: CLEAR_COLOR });

    for region in ShellRegion::ALL {
        let color = region.display_color(hovered == Some(region), focused == Some(region));
        commands.push(DrawCommand::FillRect {
            rect: scale.scale_rect(layout.rect(region)),
            color,
        });
    }

    let strip = strip_layout(layout.rect(ShellRegion::Tab), tabs.tab_count());

    for (index, slot) in strip.slots.iter().enumerate() {
        let color = if tabs.active() == Some(index) {
            ACTIVE_TAB_COLOR
        } else {
            INACTIVE_TAB_COLOR
        };
        commands.push(DrawCommand::FillRect {
            rect: scale.scale_rect(*slot),
            color,
        });
    }

    for close in &strip.closes {
        commands.push(DrawCommand::FillRect {
            rect: scale.scale_rect(*close),
            color: CLOSE_COLOR,
        });
    }

    commands.push(DrawCommand::FillRect {
        rect: scale.scale_rect(strip.new_tab),
        color: NEW_TAB_COLOR,
    });

    push_label_quads(layout, labels, scale, &mut commands);

    commands
}

/// Paints each region label as one textured quad per placed glyph.
///
/// The address label is left-aligned inside its field; the navigation labels are
/// centered in their controls. Every run is centered vertically on its region.
/// A run with no placed glyph paints nothing. The placed-run geometry is already
/// physical, so the builder anchors each run on the physical region rectangle.
fn push_label_quads(
    layout: &RegionLayout,
    labels: &LabelView,
    scale: ScaleFactor,
    commands: &mut Vec<DrawCommand>,
) {
    for (region, run) in labels.runs() {
        if run.glyphs().is_empty() {
            continue;
        }

        let physical_rect = scale.scale_rect(layout.rect(*region));
        let start_x = run_start_x(physical_rect, run, alignment(*region), scale);
        let baseline = run_baseline(physical_rect, run);
        push_run(run, start_x, baseline, commands);
    }
}

/// Emits the textured quads of one placed run along a baseline.
///
/// The pen starts at `start_x` and advances by each glyph paint advance. Each
/// quad offsets from the pen and baseline by the glyph bearings and samples the
/// run atlas at the glyph source rectangle, converted from physical texels to
/// texel coordinates.
fn push_run(run: &PlacedGlyphRun, start_x: f32, baseline: f32, commands: &mut Vec<DrawCommand>) {
    let mut pen = start_x;
    for glyph in run.glyphs() {
        let source = glyph.source();
        commands.push(DrawCommand::TexturedQuad {
            rect: Rect::new(
                pen + glyph.left() as f32,
                baseline + glyph.top() as f32,
                source.width as f32,
                source.height as f32,
            ),
            texture: run.atlas(),
            source: Rect::new(
                source.x as f32,
                source.y as f32,
                source.width as f32,
                source.height as f32,
            ),
        });
        pen += to_px(glyph.advance());
    }
}

/// The alignment of a label inside its region.
///
/// The address field reads left to right from its edge; every control label sits
/// centered. The match is exhaustive, so a new region must choose an alignment.
fn alignment(region: ShellRegion) -> TextAlignment {
    match region {
        ShellRegion::AddressField => TextAlignment::Left,
        ShellRegion::NavigationBack
        | ShellRegion::NavigationForward
        | ShellRegion::NavigationReload
        | ShellRegion::TopBar
        | ShellRegion::Tab
        | ShellRegion::Viewport => TextAlignment::Centered,
    }
}

/// The pen start for a run given its physical region rectangle and alignment.
///
/// A left run insets from the region left edge by the scaled inset. A centered
/// run starts so its total advance is centered in the region width. The rectangle
/// and the run advance are physical, so the scaled inset keeps the seam uniform.
fn run_start_x(
    physical_rect: Rect,
    run: &PlacedGlyphRun,
    alignment: TextAlignment,
    scale: ScaleFactor,
) -> f32 {
    match alignment {
        TextAlignment::Left => physical_rect.x + scale.scale_length(ADDRESS_TEXT_INSET),
        TextAlignment::Centered => {
            physical_rect.x + (physical_rect.width - to_px(run.total_advance())) / 2.0
        }
    }
}

/// The baseline that centers a run vertically on its region.
///
/// The text block spans the ascent above the baseline and the descent below it.
/// Centering that block on the region places the baseline at the region top plus
/// half the free vertical space plus the ascent.
fn run_baseline(rect: Rect, run: &PlacedGlyphRun) -> f32 {
    let ascent = to_px(run.ascent());
    let descent = to_px(run.descent());
    rect.y + (rect.height - (ascent + descent)) / 2.0 + ascent
}

/// Converts a fixed-point text length to a surface-pixel length.
fn to_px(unit: TextUnit) -> f32 {
    unit.raw() as f32 / ONE_PX_RAW as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::region_layout::layout;
    use crate::scale_factor::CHROME_FONT_SIZE;
    use purr_graphics::Extent2d;

    fn built(hovered: Option<ShellRegion>, focused: Option<ShellRegion>) -> Vec<DrawCommand> {
        let layout = layout(Extent2d::new(1280, 800));
        build_commands(
            &layout,
            hovered,
            focused,
            TabStripView::default(),
            &LabelView::default(),
            ScaleFactor::ONE,
        )
    }

    #[test]
    fn first_command_clears_to_background() {
        let commands = built(None, None);

        assert_eq!(commands[0], DrawCommand::Clear { color: CLEAR_COLOR });
    }

    #[test]
    fn empty_view_fills_every_region_and_the_new_tab_button() {
        let commands = built(None, None);

        let fills = commands
            .iter()
            .filter(|command| matches!(command, DrawCommand::FillRect { .. }))
            .count();

        assert_eq!(fills, ShellRegion::ALL.len() + 1);
        assert_eq!(commands.len(), ShellRegion::ALL.len() + 2);
    }

    #[test]
    fn fill_order_follows_the_fixed_region_order() {
        let extent = Extent2d::new(1280, 800);
        let placed = layout(extent);
        let commands = build_commands(
            &placed,
            None,
            None,
            TabStripView::default(),
            &LabelView::default(),
            ScaleFactor::ONE,
        );

        for (index, region) in ShellRegion::ALL.iter().enumerate() {
            assert_eq!(
                commands[index + 1],
                DrawCommand::FillRect {
                    rect: placed.rect(*region),
                    color: region.base_color(),
                },
                "{region:?} paints out of order"
            );
        }
    }

    #[test]
    fn each_region_fill_rectangle_equals_the_region_rectangle() {
        let extent = Extent2d::new(1024, 768);
        let placed = layout(extent);
        let commands = build_commands(
            &placed,
            None,
            None,
            TabStripView::default(),
            &LabelView::default(),
            ScaleFactor::ONE,
        );

        for (index, region) in ShellRegion::ALL.iter().enumerate() {
            let DrawCommand::FillRect { rect, .. } = commands[index + 1] else {
                panic!("{region:?} is not a fill");
            };
            assert_eq!(rect, placed.rect(*region));
        }
    }

    #[test]
    fn hovered_region_uses_the_hover_color() {
        let region = ShellRegion::AddressField;
        let commands = built(Some(region), None);
        let index = ShellRegion::ALL.iter().position(|r| *r == region).unwrap() + 1;

        assert_eq!(
            commands[index],
            DrawCommand::FillRect {
                rect: layout(Extent2d::new(1280, 800)).rect(region),
                color: region.display_color(true, false),
            }
        );
        assert_ne!(region.display_color(true, false), region.base_color());
    }

    #[test]
    fn focused_region_uses_the_focus_color() {
        let region = ShellRegion::Viewport;
        let commands = built(None, Some(region));
        let index = ShellRegion::ALL.iter().position(|r| *r == region).unwrap() + 1;

        assert_eq!(
            commands[index],
            DrawCommand::FillRect {
                rect: layout(Extent2d::new(1280, 800)).rect(region),
                color: region.display_color(false, true),
            }
        );
        assert_ne!(region.display_color(false, true), region.base_color());
    }

    #[test]
    fn focus_takes_precedence_over_hover() {
        let region = ShellRegion::AddressField;
        let commands = built(Some(region), Some(region));
        let index = ShellRegion::ALL.iter().position(|r| *r == region).unwrap() + 1;

        let DrawCommand::FillRect { color, .. } = commands[index] else {
            panic!("{region:?} is not a fill");
        };
        assert_eq!(color, region.display_color(true, true));
        assert_eq!(color, region.display_color(false, true));
    }

    fn fills_after_regions(commands: &[DrawCommand]) -> &[DrawCommand] {
        &commands[ShellRegion::ALL.len() + 1..]
    }

    #[test]
    fn strip_emits_one_slot_one_close_per_tab_and_one_new_tab_fill() {
        let placed = layout(Extent2d::new(1280, 800));
        let commands = build_commands(
            &placed,
            None,
            None,
            TabStripView::new(3, Some(0)),
            &LabelView::default(),
            ScaleFactor::ONE,
        );

        let strip = fills_after_regions(&commands);

        assert_eq!(strip.len(), 3 + 3 + 1);
    }

    #[test]
    fn active_slot_uses_the_active_color_and_an_inactive_slot_does_not() {
        let placed = layout(Extent2d::new(1280, 800));
        let commands = build_commands(
            &placed,
            None,
            None,
            TabStripView::new(3, Some(1)),
            &LabelView::default(),
            ScaleFactor::ONE,
        );

        let strip = fills_after_regions(&commands);

        let color_of = |command: &DrawCommand| match command {
            DrawCommand::FillRect { color, .. } => *color,
            _ => panic!("strip element is not a fill"),
        };
        assert_eq!(color_of(&strip[1]), ACTIVE_TAB_COLOR);
        assert_eq!(color_of(&strip[0]), INACTIVE_TAB_COLOR);
        assert_ne!(ACTIVE_TAB_COLOR, INACTIVE_TAB_COLOR);
    }

    use purr_text::{
        BundledFont, CmapOneToOneAdapter, GlyphKey, GlyphRunGeneration, GlyphRunId, ShapingRequest,
        TextShapingAdapter, build_glyph_atlas,
    };

    fn label_size() -> TextUnit {
        CHROME_FONT_SIZE
    }

    fn placed_run(font: &BundledFont, text: &str, resource_id: u64) -> PlacedGlyphRun {
        let size = label_size();
        let run = CmapOneToOneAdapter
            .shape(ShapingRequest {
                font,
                text,
                size,
                run_id: GlyphRunId::new(1),
                generation: GlyphRunGeneration::new(1),
            })
            .expect("the label shapes");

        let keys: Vec<GlyphKey> = run
            .glyphs()
            .iter()
            .map(|glyph| GlyphKey::new(glyph.glyph(), size))
            .collect();
        let atlas = build_glyph_atlas(
            font,
            &keys,
            purr_graphics::ProducerNamespace::new(3),
            purr_graphics::ResourceId::new(resource_id),
            purr_graphics::ResourceGeneration::new(1),
            purr_graphics::DeviceGeneration::new(1),
        )
        .expect("the atlas builds");
        let metrics = font.metrics(size).expect("metrics scale in range");

        PlacedGlyphRun::from_shaped_run(&run, &atlas, metrics)
    }

    fn textured_quads(commands: &[DrawCommand]) -> Vec<DrawCommand> {
        commands
            .iter()
            .filter(|command| matches!(command, DrawCommand::TexturedQuad { .. }))
            .copied()
            .collect()
    }

    fn first_quad_x(commands: &[DrawCommand]) -> f32 {
        let quads = textured_quads(commands);
        let DrawCommand::TexturedQuad { rect, .. } = quads[0] else {
            panic!("the first label command is not a textured quad");
        };
        rect.x
    }

    #[test]
    fn a_label_view_emits_one_textured_quad_per_glyph_sampling_the_run_atlas() {
        let font = BundledFont::load().expect("the bundled font parses");
        let run = placed_run(&font, "Ab", 1);
        let placed = layout(Extent2d::new(1280, 800));
        let view = LabelView::new(vec![(ShellRegion::AddressField, run.clone())]);

        let commands = build_commands(
            &placed,
            None,
            None,
            TabStripView::default(),
            &view,
            ScaleFactor::ONE,
        );

        let quads = textured_quads(&commands);
        assert_eq!(quads.len(), run.glyphs().len());
        for quad in &quads {
            let DrawCommand::TexturedQuad { texture, .. } = quad else {
                unreachable!("filtered to textured quads");
            };
            assert_eq!(*texture, run.atlas());
        }
    }

    #[test]
    fn the_address_label_is_left_aligned_and_a_nav_label_is_centered() {
        let font = BundledFont::load().expect("the bundled font parses");
        let run = placed_run(&font, "Ab", 1);
        let placed = layout(Extent2d::new(1280, 800));
        let first_left = run.glyphs()[0].left() as f32;

        let address = LabelView::new(vec![(ShellRegion::AddressField, run.clone())]);
        let address_commands = build_commands(
            &placed,
            None,
            None,
            TabStripView::default(),
            &address,
            ScaleFactor::ONE,
        );
        let address_rect = placed.rect(ShellRegion::AddressField);
        let expected_address = address_rect.x + ADDRESS_TEXT_INSET + first_left;
        assert!((first_quad_x(&address_commands) - expected_address).abs() < 0.01);

        let nav = LabelView::new(vec![(ShellRegion::NavigationBack, run.clone())]);
        let nav_commands = build_commands(
            &placed,
            None,
            None,
            TabStripView::default(),
            &nav,
            ScaleFactor::ONE,
        );
        let nav_rect = placed.rect(ShellRegion::NavigationBack);
        let expected_nav =
            nav_rect.x + (nav_rect.width - to_px(run.total_advance())) / 2.0 + first_left;
        assert!((first_quad_x(&nav_commands) - expected_nav).abs() < 0.01);

        assert_ne!(first_quad_x(&address_commands), first_quad_x(&nav_commands));
    }

    #[test]
    fn an_empty_label_view_emits_no_textured_quad() {
        let commands = built(None, None);

        assert!(textured_quads(&commands).is_empty());
    }

    #[test]
    fn a_scale_of_two_scales_every_fill_and_the_label_anchor() {
        let font = BundledFont::load().expect("the bundled font parses");
        let run = placed_run(&font, "Ab", 1);
        let placed = layout(Extent2d::new(1280, 800));
        let scale = ScaleFactor::from_winit(2.0);
        let view = LabelView::new(vec![(ShellRegion::AddressField, run.clone())]);

        let commands = build_commands(&placed, None, None, TabStripView::default(), &view, scale);

        for (index, region) in ShellRegion::ALL.iter().enumerate() {
            let DrawCommand::FillRect { rect, .. } = commands[index + 1] else {
                panic!("{region:?} is not a fill");
            };
            assert_eq!(rect, scale.scale_rect(placed.rect(*region)));
        }

        let logical_strip = strip_layout(placed.rect(ShellRegion::Tab), 0);
        let DrawCommand::FillRect { rect: new_tab, .. } = commands[ShellRegion::ALL.len() + 1]
        else {
            panic!("the new-tab element is not a fill");
        };
        assert_eq!(new_tab, scale.scale_rect(logical_strip.new_tab));

        let physical_address = scale.scale_rect(placed.rect(ShellRegion::AddressField));
        let expected_x = physical_address.x
            + scale.scale_length(ADDRESS_TEXT_INSET)
            + run.glyphs()[0].left() as f32;
        assert!((first_quad_x(&commands) - expected_x).abs() < 0.01);
    }
}
