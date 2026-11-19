use std::{
    borrow::Cow,
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

use gpui::{FontRun, LineLayout, PlatformTextSystem, RenderGlyphParams, font, point, px, rgb};
use gpui_apple::AppleTextSystem;

use super::{DEFAULT_SAMPLE, FAMILIES, WEIGHTS};

fn glyph_masks(system: &AppleTextSystem, line: &LineLayout, ink: u32) -> anyhow::Result<u64> {
    let mut hash = DefaultHasher::new();
    for run in &line.runs {
        for glyph in &run.glyphs {
            let params = RenderGlyphParams {
                font_id: run.font_id,
                glyph_id: glyph.id,
                font_size: line.font_size,
                subpixel_variant: point(0, 0),
                scale_factor: 2.,
                is_emoji: glyph.is_emoji,
                subpixel_rendering: false,
                dilation: system.glyph_dilation_for_color(rgb(ink).into()),
            };
            let bounds = system.glyph_raster_bounds(&params)?;
            if !bounds.is_empty() {
                bounds.hash(&mut hash);
                system.rasterize_glyph(&params, bounds)?.1.hash(&mut hash);
            }
        }
    }
    Ok(hash.finish())
}

pub(super) fn run(bytes: Cow<'static, [u8]>) -> anyhow::Result<()> {
    let system = AppleTextSystem::new();
    system.add_fonts(vec![bytes])?;
    for (_, family) in FAMILIES {
        for weight in WEIGHTS {
            let mut face = font(family);
            face.weight = weight;
            let font_id = system.font_id(&face)?;
            for pixels in [11., 13.] {
                let line = system.layout_line(
                    DEFAULT_SAMPLE,
                    px(pixels),
                    &[FontRun {
                        len: DEFAULT_SAMPLE.len(),
                        font_id,
                    }],
                );
                let light = glyph_masks(&system, &line, 0x0a0a0a)?;
                let dark = glyph_masks(&system, &line, 0xfafafa)?;
                println!(
                    "{family}\t{}\t{pixels}\t{:.5}\t{light:x}\t{dark:x}",
                    weight.0,
                    f32::from(line.width)
                );
            }
        }
    }
    Ok(())
}
