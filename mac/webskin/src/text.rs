//! Text for TEXT and PLAYLIST elements, laid out and drawn with the Mac's own fonts.
//! Skins name Windows fonts; macOS ships Tahoma, Verdana, Arial and friends, and anything
//! missing falls back to Tahoma, as WMP's font fallback did.

use cosmic_text::{Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, Style, SwashCache, Weight, Wrap};
use std::cell::RefCell;
use std::collections::HashMap;

/// The system's fonts, the glyph cache, and skin font names resolved to installed families.
type Fonts = (FontSystem, SwashCache, HashMap<String, Option<String>>);

thread_local! {
    static FONTS: RefCell<Option<Fonts>> = const { RefCell::new(None) };
}

fn with_fonts<T>(f: impl FnOnce(&mut FontSystem, &mut SwashCache, &mut HashMap<String, Option<String>>) -> T) -> T {
    FONTS.with(|cell| {
        let mut slot = cell.borrow_mut();
        let (fs, cache, families) = slot.get_or_insert_with(|| (FontSystem::new(), SwashCache::new(), HashMap::new()));
        f(fs, cache, families)
    })
}

/// WMP font sizes are points at 96 dpi, which Windows rounded to whole pixels.
pub fn points_to_px(pt: f64) -> f32 {
    (pt.max(1.0) * 96.0 / 72.0).round() as f32
}

/// A line's height for a font of `px` pixels, as Windows rounded Tahoma's ascent plus descent.
fn line_height(px: f32) -> f32 {
    (px * 1.21).round()
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextStyle {
    pub face: String,
    /// Size in logical pixels.
    pub px: f32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

impl TextStyle {
    /// From a WMP fontStyle string ("bold italic underline").
    pub fn new(face: &str, points: f64, style: &str) -> Self {
        let s = style.to_lowercase();
        TextStyle {
            face: face.to_string(),
            px: points_to_px(points),
            bold: s.contains("bold"),
            italic: s.contains("italic"),
            underline: s.contains("underline"),
        }
    }
}

/// The installed family for a skin's font name, or Tahoma.
fn resolve_family(fs: &mut FontSystem, families: &mut HashMap<String, Option<String>>, face: &str) -> Option<String> {
    let key = face.trim().to_lowercase();
    if let Some(hit) = families.get(&key) {
        return hit.clone();
    }
    let find = |fs: &FontSystem, name: &str| {
        fs.db().faces().find_map(|f| f.families.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(n, _)| n.clone()))
    };
    let hit = find(fs, face.trim()).or_else(|| find(fs, "Tahoma")).or_else(|| find(fs, "Verdana"));
    families.insert(key, hit.clone());
    hit
}

fn layout(
    fs: &mut FontSystem,
    families: &mut HashMap<String, Option<String>>,
    text: &str,
    style: &TextStyle,
    scale: f32,
    wrap: Option<f32>,
) -> Buffer {
    let px = style.px * scale;
    let mut buffer = Buffer::new(fs, Metrics::new(px, line_height(style.px) * scale));
    let family = resolve_family(fs, families, &style.face);
    let attrs = Attrs::new()
        .family(family.as_deref().map_or(Family::SansSerif, Family::Name))
        .weight(if style.bold { Weight::BOLD } else { Weight::NORMAL })
        .style(if style.italic { Style::Italic } else { Style::Normal });
    buffer.set_wrap(if wrap.is_some() { Wrap::WordOrGlyph } else { Wrap::None });
    buffer.set_size(wrap.map(|w| w * scale), None);
    buffer.set_text(text, &attrs, Shaping::Advanced, None);
    buffer.shape_until_scroll(fs, false);
    buffer
}

/// (width, height) in logical pixels; `wrap` gives a width to wrap words at.
pub fn measure(text: &str, style: &TextStyle, wrap: Option<f32>) -> (f32, f32) {
    if text.is_empty() {
        return (0.0, line_height(style.px));
    }
    with_fonts(|fs, _, families| {
        let b = layout(fs, families, text, style, 1.0, wrap);
        let (w, lines) = b.layout_runs().fold((0f32, 0usize), |(w, n), run| (w.max(run.line_w), n + 1));
        (w, lines.max(1) as f32 * line_height(style.px))
    })
}

/// The longest prefix of `text` plus "…" that fits in `width`.
pub fn ellipsize(text: &str, style: &TextStyle, width: f32) -> String {
    if measure(text, style, None).0 <= width {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let (mut lo, mut hi) = (0usize, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let candidate: String = chars[..mid].iter().collect::<String>() + "…";
        if measure(&candidate, style, None).0 <= width { lo = mid } else { hi = mid - 1 }
    }
    chars[..lo].iter().collect::<String>() + "…"
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// Glyph coverage, in device pixels relative to the text origin: (x, y, w, h, alpha).
pub struct Coverage {
    pub spans: Vec<(i32, i32, u32, u32, u8)>,
}

/// Rasterizes text at `scale` device pixels per logical pixel. With `wrap`, lines are aligned
/// within that width.
pub fn rasterize(text: &str, style: &TextStyle, scale: f32, wrap: Option<(f32, Align)>) -> Coverage {
    with_fonts(|fs, cache, families| {
        let mut buffer = layout(fs, families, text, style, scale, wrap.map(|(w, _)| w));
        let mut spans = Vec::new();
        let (width, align) = wrap.map_or((0.0, Align::Left), |(w, a)| (w * scale, a));
        let offsets: Vec<f32> = buffer
            .layout_runs()
            .map(|run| match align {
                Align::Left => 0.0,
                Align::Center => ((width - run.line_w) / 2.0).max(0.0),
                Align::Right => (width - run.line_w).max(0.0),
            })
            .collect();
        let line_h = buffer.metrics().line_height;
        let underlines: Vec<(i32, i32, u32)> = if style.underline {
            buffer
                .layout_runs()
                .zip(&offsets)
                .map(|(run, off)| (*off as i32, (run.line_y + (style.px * scale * 0.12).max(1.0)) as i32, run.line_w as u32))
                .collect()
        } else {
            vec![]
        };
        buffer.draw(fs, cache, Color::rgb(255, 255, 255), |x, y, w, h, c| {
            let line = ((y as f32) / line_h).floor().max(0.0) as usize;
            let dx = offsets.get(line).copied().unwrap_or(0.0) as i32;
            if c.a() > 0 {
                spans.push((x + dx, y, w, h, c.a()));
            }
        });
        let thickness = (scale.round() as u32).max(1);
        spans.extend(underlines.into_iter().map(|(x, y, w)| (x, y, w, thickness, 255)));
        Coverage { spans }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measures_and_ellipsizes() {
        let style = TextStyle::new("Tahoma", 8.0, "normal");
        let (w, h) = measure("Hello world", &style, None);
        assert!(w > 30.0 && w < 90.0, "width {w}");
        assert!((10.0..20.0).contains(&h), "height {h}");
        let short = ellipsize("Hello world, this is long", &style, 40.0);
        assert!(short.ends_with('…') && measure(&short, &style, None).0 <= 40.0);
        let (_, wrapped) = measure("Hello world, this is long", &style, Some(40.0));
        assert!(wrapped > h);
    }
}
