//! Decoded skin images, with WMP's colour keying, hue shift and animated GIFs.

use crate::package::SkinPackage;
use crate::resources;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use tiny_skia::{Pixmap, PremultipliedColorU8};

/// A decoded skin image: the source pixels (for colour lookups) plus ready-to-draw frames.
pub struct SkinBitmap {
    pub width: u32,
    pub height: u32,
    /// Source pixels as 0xAARRGGBB, before any colour keying.
    pub original: Rc<Vec<u32>>,
    /// Drawable frames after keying/tinting; one for still images.
    pub frames: Vec<Pixmap>,
    /// Per-frame display time in ms (animated GIFs).
    pub delays: Vec<u32>,
    /// GIF loop count: 0 = forever, n = play n times.
    pub loops: u32,
}

impl SkinBitmap {
    fn new(width: u32, height: u32, original: Rc<Vec<u32>>, frames: Vec<Pixmap>, delays: Vec<u32>, loops: u32) -> Self {
        Self { width, height, original, frames, delays, loops }
    }

    pub fn from_argb(width: u32, height: u32, argb: Vec<u32>) -> Option<Self> {
        let frame = to_pixmap(width, height, &argb)?;
        Some(Self::new(width, height, Rc::new(argb), vec![frame], vec![], 1))
    }

    pub fn is_animated(&self) -> bool {
        self.frames.len() > 1
    }

    pub fn source(&self) -> &Pixmap {
        &self.frames[0]
    }

    /// The frame to show `elapsed_ms` after the animation started, and how long until it changes
    /// (None once a finite animation has finished, or for still images).
    pub fn frame_at(&self, elapsed_ms: f64) -> (&Pixmap, Option<f64>) {
        let total: u32 = self.delays.iter().sum();
        if !self.is_animated() || total == 0 {
            return (self.source(), None);
        }
        let last = self.frames.last().expect("animated");
        if self.loops > 0 && elapsed_ms >= f64::from(total) * f64::from(self.loops) {
            return (last, None);
        }
        let mut t = elapsed_ms % f64::from(total);
        for (frame, &delay) in self.frames.iter().zip(&self.delays) {
            if t < f64::from(delay) {
                return (frame, Some(f64::from(delay) - t));
            }
            t -= f64::from(delay);
        }
        (last, None)
    }

    /// RGB (no alpha) at x,y, or None outside.
    pub fn rgb(&self, x: i32, y: i32) -> Option<u32> {
        self.index(x, y).map(|i| self.original[i] & 0xFF_FFFF)
    }

    /// Drawn alpha at x,y (0 outside).
    pub fn alpha(&self, x: i32, y: i32) -> u8 {
        self.index(x, y).map_or(0, |i| self.source().pixels()[i].alpha())
    }

    fn index(&self, x: i32, y: i32) -> Option<usize> {
        (x >= 0 && y >= 0 && (x as u32) < self.width && (y as u32) < self.height).then(|| y as usize * self.width as usize + x as usize)
    }

    /// The image's shape: every pixel except the key colour, clear ones included (a frame image's
    /// see-through middle belongs to the shape; its colour-keyed outside does not).
    pub fn shape_of(&self, key: u32) -> Option<SkinBitmap> {
        let px: Vec<u32> = self.original.iter().map(|&c| if matches_key(c & 0xFF_FFFF, key) { 0 } else { 0xFF00_0000 }).collect();
        SkinBitmap::from_argb(self.width, self.height, px)
    }

    /// A copy whose pixels outside `keep` (by mapping colour) are cleared: one element of a BUTTONGROUP.
    pub fn masked_by(&self, map: &SkinBitmap, color: u32) -> Option<SkinBitmap> {
        let mut frame = self.source().clone();
        let w = self.width as i32;
        for (i, px) in frame.pixels_mut().iter_mut().enumerate() {
            let (x, y) = (i as i32 % w, i as i32 / w);
            if map.rgb(x, y) != Some(color) {
                *px = PremultipliedColorU8::TRANSPARENT;
            }
        }
        Some(Self::new(self.width, self.height, self.original.clone(), vec![frame], vec![], 1))
    }
}

/// Premultiplies 0xAARRGGBB pixels into a drawable pixmap.
pub fn to_pixmap(width: u32, height: u32, argb: &[u32]) -> Option<Pixmap> {
    let mut pm = Pixmap::new(width.max(1), height.max(1))?;
    for (dst, &c) in pm.pixels_mut().iter_mut().zip(argb) {
        let a = (c >> 24) as u8;
        let mul = |v: u32| ((v & 255) * u32::from(a) / 255) as u8;
        *dst = PremultipliedColorU8::from_rgba(mul(c >> 16), mul(c >> 8), mul(c), a).expect("premultiplied");
    }
    Some(pm)
}

struct Decoded {
    width: u32,
    height: u32,
    frames: Vec<Rc<Vec<u32>>>,
    delays: Vec<u32>,
    loops: u32,
}

/// The current track's artwork, as 0xAARRGGBB pixels.
pub struct Artwork {
    pub width: u32,
    pub height: u32,
    pub argb: Vec<u32>,
}

/// The current artwork and its album-art images by size.
type ArtCache = (Option<Rc<Artwork>>, HashMap<u32, Rc<SkinBitmap>>);

/// Per-skin image cache with colour-key and hue/saturation variants.
pub struct SkinImages {
    package: Rc<SkinPackage>,
    decoded: RefCell<HashMap<String, Option<Rc<Decoded>>>>,
    variants: RefCell<HashMap<String, Option<Rc<SkinBitmap>>>>,
    artwork: RefCell<ArtCache>,
}

/// WMP's built-in dynamic images ("WMPImage_AlbumArtLarge", "WMPImage_AdBanner", ...).
pub fn is_dynamic(name: &str) -> bool {
    name.trim_start().len() >= 9 && name.trim_start()[..9].eq_ignore_ascii_case("WMPImage_")
}

impl SkinImages {
    pub fn new(package: Rc<SkinPackage>) -> Self {
        Self { package, decoded: RefCell::default(), variants: RefCell::default(), artwork: RefCell::default() }
    }

    /// A new track's artwork (None = no art): WMPImage_AlbumArt* follow it.
    pub fn set_artwork(&self, art: Option<Rc<Artwork>>) {
        *self.artwork.borrow_mut() = (art, HashMap::new());
    }

    /// Image without keying, for mapping/position/clipping lookups.
    pub fn get_raw(&self, name: &str) -> Option<Rc<SkinBitmap>> {
        self.get(name, None, 0.0, 1.0, None)
    }

    /// Image with `key` (transparencyColor or clippingColor) made fully transparent, and an optional
    /// hue shift (fraction of a turn) and saturation multiplier applied.
    pub fn get(&self, name: &str, key: Option<u32>, hue_shift: f64, saturation: f64, second_key: Option<u32>) -> Option<Rc<SkinBitmap>> {
        let name = name.trim();
        if name.is_empty() {
            return None;
        }
        if is_dynamic(name) {
            return name[9..].to_ascii_lowercase().starts_with("albumart").then(|| self.album_art(name));
        }
        let cache_key = format!("{}|{key:?}|{second_key:?}|{hue_shift:.3}|{saturation:.3}", name.to_lowercase());
        if let Some(v) = self.variants.borrow().get(&cache_key) {
            return v.clone();
        }
        let made = self.decode(name).and_then(|d| {
            let tint = hue_shift.abs() > 0.0001 || (saturation - 1.0).abs() > 0.0001;
            let process = |src: &[u32]| -> Option<Pixmap> {
                let px: Vec<u32> = src
                    .iter()
                    .map(|&c| {
                        let rgb = c & 0xFF_FFFF;
                        if key.is_some_and(|k| matches_key(rgb, k)) || second_key.is_some_and(|k| matches_key(rgb, k)) {
                            return 0;
                        }
                        let rgb = if tint { tint_rgb(rgb, hue_shift, saturation) } else { rgb };
                        (c & 0xFF00_0000) | rgb
                    })
                    .collect();
                to_pixmap(d.width, d.height, &px)
            };
            let frames = d.frames.iter().map(|f| process(f)).collect::<Option<Vec<_>>>()?;
            Some(Rc::new(SkinBitmap::new(d.width, d.height, d.frames[0].clone(), frames, d.delays.clone(), d.loops)))
        });
        self.variants.borrow_mut().insert(cache_key, made.clone());
        made
    }

    fn decode(&self, name: &str) -> Option<Rc<Decoded>> {
        let key = name.to_lowercase();
        if let Some(d) = self.decoded.borrow().get(&key) {
            return d.clone();
        }
        let bytes = if resources::is_resource_url(name) { resources::read_bytes(name) } else { self.package.read_bytes(name).map(<[u8]>::to_vec) };
        let decoded = match bytes {
            Some(b) => {
                let d = decode_image(&b).map(Rc::new);
                if d.is_none() {
                    crate::log(&format!("Skin image {name} could not be decoded"));
                }
                d
            }
            None => {
                crate::log(&format!("Skin image not found: {name}"));
                None
            }
        };
        self.decoded.borrow_mut().insert(key, decoded.clone());
        decoded
    }

    /// The current track's album art, square, or WMP's placeholder when there is none.
    fn album_art(&self, name: &str) -> Rc<SkinBitmap> {
        let size: u32 = if name.to_ascii_lowercase().ends_with("small") { 75 } else { 200 };
        if let Some(b) = self.artwork.borrow().1.get(&size) {
            return b.clone();
        }
        let art = self.artwork.borrow().0.clone();
        let px = match art {
            Some(a) if a.width > 0 && a.height > 0 => square_downscale(&a, size),
            _ => placeholder_art(size),
        };
        let bmp = Rc::new(SkinBitmap::from_argb(size, size, px).expect("art size"));
        self.artwork.borrow_mut().1.insert(size, bmp.clone());
        bmp
    }
}

fn square_downscale(a: &Artwork, size: u32) -> Vec<u32> {
    let side = a.width.min(a.height);
    let (ox, oy) = ((a.width - side) / 2, (a.height - side) / 2);
    (0..size * size)
        .map(|i| {
            let (x, y) = (i % size, i / size);
            let sx0 = ox + x * side / size;
            let sx1 = (ox + (x + 1) * side / size).max(sx0 + 1);
            let sy0 = oy + y * side / size;
            let sy1 = (oy + (y + 1) * side / size).max(sy0 + 1);
            let (mut r, mut g, mut b, mut n) = (0u32, 0u32, 0u32, 0u32);
            for sy in sy0..sy1 {
                for sx in sx0..sx1 {
                    let c = a.argb[(sy * a.width + sx) as usize];
                    r += (c >> 16) & 255;
                    g += (c >> 8) & 255;
                    b += c & 255;
                    n += 1;
                }
            }
            0xFF00_0000 | ((r / n) << 16) | ((g / n) << 8) | (b / n)
        })
        .collect()
}

/// Dark blue gradient disc, like WMP 11's "no album art".
fn placeholder_art(size: u32) -> Vec<u32> {
    let c = size as f32 / 2.0;
    (0..size * size)
        .map(|i| {
            let (x, y) = ((i % size) as f32, (i / size) as f32);
            let d = ((x - c).powi(2) + (y - c).powi(2)).sqrt() / c;
            let v = (40.0 + 70.0 * (1.0 - y / size as f32)) as i32;
            let ring = if d < 0.8 && d > 0.12 { (25.0 * (d * 40.0).sin()) as i32 + 20 } else { 0 };
            let ch = |v: i32| v.clamp(0, 255) as u32;
            0xFF00_0000 | (ch(v / 3 + ring) << 16) | (ch(v / 2 + ring) << 8) | ch(v + ring)
        })
        .collect()
}

fn decode_image(bytes: &[u8]) -> Option<Decoded> {
    if bytes.starts_with(b"GIF8")
        && let Some(anim) = decode_gif(bytes)
    {
        return Some(anim);
    }
    let img = image::load_from_memory(bytes).ok()?;
    let has_alpha = img.color().has_alpha();
    let rgba = img.to_rgba8();
    // 32bpp BMPs often carry a garbage/zero alpha channel; WMP ignored it.
    let opaque = bytes.starts_with(b"BM") || !has_alpha;
    let px = rgba
        .pixels()
        .map(|p| {
            let a = if opaque { 255 } else { u32::from(p[3]) };
            (a << 24) | (u32::from(p[0]) << 16) | (u32::from(p[1]) << 8) | u32::from(p[2])
        })
        .collect();
    Some(Decoded { width: rgba.width(), height: rgba.height(), frames: vec![Rc::new(px)], delays: vec![], loops: 1 })
}

/// Composes an animated GIF into full-size frames (offsets and disposal), with per-frame delays and
/// the NETSCAPE loop count; WMP played these in skins. None for single-frame GIFs.
/// How long WMP showed a GIF frame, measured on Windows: its delay rounded up to the 1/64 s
/// system timer tick, and never under three ticks, so 0 to 40 ms frames all last 47 ms.
fn wmp_frame_ms(centiseconds: u16) -> u32 {
    let ticks = (u32::from(centiseconds) * 10 * 64).div_ceil(1000).max(3);
    (ticks * 1000 + 32) / 64
}

fn decode_gif(bytes: &[u8]) -> Option<Decoded> {
    let mut opts = gif::DecodeOptions::new();
    opts.set_color_output(gif::ColorOutput::RGBA);
    let mut dec = opts.read_info(bytes).ok()?;
    let (w, h) = (u32::from(dec.width()), u32::from(dec.height()));
    let mut raw = Vec::new();
    while let Ok(Some(frame)) = dec.read_next_frame() {
        raw.push(frame.clone());
    }
    if raw.len() < 2 || w == 0 || h == 0 {
        return None;
    }
    // Keep memory sane for huge animations: drop frames beyond ~40M pixels.
    let max_frames = (40_000_000 / (w * h).max(1)).clamp(2, 400) as usize;
    let step = if raw.len() > max_frames { raw.len().div_ceil(max_frames) } else { 1 };
    let loops = match dec.repeat() {
        gif::Repeat::Infinite => 0,
        gif::Repeat::Finite(0) => 1,
        gif::Repeat::Finite(n) => u32::from(n),
    };

    let mut canvas = vec![0u32; (w * h) as usize];
    let mut frames = Vec::new();
    let mut delays = Vec::new();
    let mut pending_delay = 0;
    let count = raw.len();
    for (i, f) in raw.iter().enumerate() {
        let (left, top, fw, fh) = (u32::from(f.left), u32::from(f.top), u32::from(f.width), u32::from(f.height));
        let delay = wmp_frame_ms(f.delay);
        let previous = (f.dispose == gif::DisposalMethod::Previous).then(|| canvas.clone());
        for y in 0..fh {
            let cy = top + y;
            if cy >= h {
                continue;
            }
            for x in 0..fw {
                let cx = left + x;
                if cx >= w {
                    continue;
                }
                let s = ((y * fw + x) * 4) as usize;
                let Some(p) = f.buffer.get(s..s + 4) else { continue };
                if p[3] != 0 {
                    canvas[(cy * w + cx) as usize] = (u32::from(p[3]) << 24) | (u32::from(p[0]) << 16) | (u32::from(p[1]) << 8) | u32::from(p[2]);
                }
            }
        }
        pending_delay += delay;
        if i % step == 0 || i == count - 1 {
            frames.push(Rc::new(canvas.clone()));
            delays.push(pending_delay);
            pending_delay = 0;
        }
        if f.dispose == gif::DisposalMethod::Background {
            for y in top.min(h)..(top + fh).min(h) {
                for x in left.min(w)..(left + fw).min(w) {
                    canvas[(y * w + x) as usize] = 0;
                }
            }
        } else if let Some(p) = previous {
            canvas = p;
        }
    }
    // A finite animation whose last frame alone asks to be cleared ends cleared: QuickSilver's
    // shutter opens onto the display under it. When every frame asks, that was only how the GIF
    // was saved, and the last frame stays (Xbox Live's buttons, in its preview).
    let last = raw[count - 1].dispose;
    let clears = matches!(last, gif::DisposalMethod::Background | gif::DisposalMethod::Previous);
    if loops > 0 && clears && raw.iter().any(|f| f.dispose != last) {
        frames.push(Rc::new(canvas));
        delays.push(0);
    }
    Some(Decoded { width: w, height: h, frames, delays, loops })
}

fn tint_rgb(rgb: u32, hue_shift: f64, saturation: f64) -> u32 {
    let ch = |s: u32| f64::from((rgb >> s) & 255) / 255.0;
    let (r, g, b) = (ch(16), ch(8), ch(0));
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let l = (max + min) / 2.0;
    let (mut h, mut s) = (0.0, 0.0);
    if max != min {
        let d = max - min;
        s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
        h = if max == r {
            (g - b) / d + if g < b { 6.0 } else { 0.0 }
        } else if max == g {
            (b - r) / d + 2.0
        } else {
            (r - g) / d + 4.0
        };
        h /= 6.0;
    }
    h = ((h + hue_shift) % 1.0 + 1.0) % 1.0;
    s = (s * saturation).clamp(0.0, 1.0);
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    let hue = |t: f64| {
        let t = (t % 1.0 + 1.0) % 1.0;
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    let to = |v: f64| (v * 255.0).round().clamp(0.0, 255.0) as u32;
    if s == 0.0 {
        let v = to(l);
        return (v << 16) | (v << 8) | v;
    }
    (to(hue(h + 1.0 / 3.0)) << 16) | (to(hue(h)) << 8) | to(hue(h - 1.0 / 3.0))
}

/// "#RRGGBB", "RRGGBB", "#RGB", "#AARRGGBB" or a named colour; "none"/"transparent" are no colour.
/// Whether `rgb` is the key colour `key`. The key's 16-bit forms (5-5-5 and 5-6-5) match too:
/// bitmaps saved from a 16-bit desktop hold those (YIL Online Music Awards' speakers are
/// 0x639CFF for the key 0x6699FF).
pub fn matches_key(rgb: u32, key: u32) -> bool {
    rgb == key || rgb == sixteen_bit(key, 5) || rgb == sixteen_bit(key, 6)
}

/// `rgb` with each channel cut to 5 bits (green to `green_bits`) and widened back to 8.
fn sixteen_bit(rgb: u32, green_bits: u32) -> u32 {
    let channel = |shift: u32, bits: u32| {
        let v = ((rgb >> shift) & 255) >> (8 - bits);
        ((v << (8 - bits)) | (v >> (2 * bits - 8))) << shift
    };
    channel(16, 5) | channel(8, green_bits) | channel(0, 5)
}

pub fn parse_color(s: &str) -> Option<u32> {
    let s = s.trim();
    if s.is_empty() || s.eq_ignore_ascii_case("none") || s.eq_ignore_ascii_case("transparent") {
        return None;
    }
    let hex = |h: &str| u32::from_str_radix(h, 16).ok();
    match s.strip_prefix('#') {
        Some(h) if h.len() == 6 => hex(h),
        Some(h) if h.len() == 8 => hex(h).map(|v| v & 0xFF_FFFF),
        Some(h) if h.len() == 3 || h.len() == 4 => {
            let v = hex(h)?;
            let (r, g, b) = ((v >> 8) & 15, (v >> 4) & 15, v & 15);
            Some((r * 17) << 16 | (g * 17) << 8 | (b * 17))
        }
        Some(_) => None,
        None if s.len() == 6 => hex(s).or_else(|| named_color(s)),
        None => named_color(s),
    }
}

fn named_color(s: &str) -> Option<u32> {
    const NAMES: &[(&str, u32)] = &[
        ("aliceblue", 0xF0F8FF),
        ("antiquewhite", 0xFAEBD7),
        ("aqua", 0x00FFFF),
        ("aquamarine", 0x7FFFD4),
        ("azure", 0xF0FFFF),
        ("beige", 0xF5F5DC),
        ("bisque", 0xFFE4C4),
        ("black", 0x000000),
        ("blanchedalmond", 0xFFEBCD),
        ("blue", 0x0000FF),
        ("blueviolet", 0x8A2BE2),
        ("brown", 0xA52A2A),
        ("burlywood", 0xDEB887),
        ("cadetblue", 0x5F9EA0),
        ("chartreuse", 0x7FFF00),
        ("chocolate", 0xD2691E),
        ("coral", 0xFF7F50),
        ("cornflowerblue", 0x6495ED),
        ("cornsilk", 0xFFF8DC),
        ("crimson", 0xDC143C),
        ("cyan", 0x00FFFF),
        ("darkblue", 0x00008B),
        ("darkcyan", 0x008B8B),
        ("darkgoldenrod", 0xB8860B),
        ("darkgray", 0xA9A9A9),
        ("darkgreen", 0x006400),
        ("darkkhaki", 0xBDB76B),
        ("darkmagenta", 0x8B008B),
        ("darkolivegreen", 0x556B2F),
        ("darkorange", 0xFF8C00),
        ("darkorchid", 0x9932CC),
        ("darkred", 0x8B0000),
        ("darksalmon", 0xE9967A),
        ("darkseagreen", 0x8FBC8F),
        ("darkslateblue", 0x483D8B),
        ("darkslategray", 0x2F4F4F),
        ("darkturquoise", 0x00CED1),
        ("darkviolet", 0x9400D3),
        ("deeppink", 0xFF1493),
        ("deepskyblue", 0x00BFFF),
        ("dimgray", 0x696969),
        ("dodgerblue", 0x1E90FF),
        ("firebrick", 0xB22222),
        ("floralwhite", 0xFFFAF0),
        ("forestgreen", 0x228B22),
        ("fuchsia", 0xFF00FF),
        ("gainsboro", 0xDCDCDC),
        ("ghostwhite", 0xF8F8FF),
        ("gold", 0xFFD700),
        ("goldenrod", 0xDAA520),
        ("gray", 0x808080),
        ("grey", 0x808080),
        ("green", 0x008000),
        ("greenyellow", 0xADFF2F),
        ("honeydew", 0xF0FFF0),
        ("hotpink", 0xFF69B4),
        ("indianred", 0xCD5C5C),
        ("indigo", 0x4B0082),
        ("ivory", 0xFFFFF0),
        ("khaki", 0xF0E68C),
        ("lavender", 0xE6E6FA),
        ("lavenderblush", 0xFFF0F5),
        ("lawngreen", 0x7CFC00),
        ("lemonchiffon", 0xFFFACD),
        ("lightblue", 0xADD8E6),
        ("lightcoral", 0xF08080),
        ("lightcyan", 0xE0FFFF),
        ("lightgoldenrodyellow", 0xFAFAD2),
        ("lightgray", 0xD3D3D3),
        ("lightgreen", 0x90EE90),
        ("lightpink", 0xFFB6C1),
        ("lightsalmon", 0xFFA07A),
        ("lightseagreen", 0x20B2AA),
        ("lightskyblue", 0x87CEFA),
        ("lightslategray", 0x778899),
        ("lightsteelblue", 0xB0C4DE),
        ("lightyellow", 0xFFFFE0),
        ("lime", 0x00FF00),
        ("limegreen", 0x32CD32),
        ("linen", 0xFAF0E6),
        ("magenta", 0xFF00FF),
        ("maroon", 0x800000),
        ("mediumaquamarine", 0x66CDAA),
        ("mediumblue", 0x0000CD),
        ("mediumorchid", 0xBA55D3),
        ("mediumpurple", 0x9370DB),
        ("mediumseagreen", 0x3CB371),
        ("mediumslateblue", 0x7B68EE),
        ("mediumspringgreen", 0x00FA9A),
        ("mediumturquoise", 0x48D1CC),
        ("mediumvioletred", 0xC71585),
        ("midnightblue", 0x191970),
        ("mintcream", 0xF5FFFA),
        ("mistyrose", 0xFFE4E1),
        ("moccasin", 0xFFE4B5),
        ("navajowhite", 0xFFDEAD),
        ("navy", 0x000080),
        ("oldlace", 0xFDF5E6),
        ("olive", 0x808000),
        ("olivedrab", 0x6B8E23),
        ("orange", 0xFFA500),
        ("orangered", 0xFF4500),
        ("orchid", 0xDA70D6),
        ("palegoldenrod", 0xEEE8AA),
        ("palegreen", 0x98FB98),
        ("paleturquoise", 0xAFEEEE),
        ("palevioletred", 0xDB7093),
        ("papayawhip", 0xFFEFD5),
        ("peachpuff", 0xFFDAB9),
        ("peru", 0xCD853F),
        ("pink", 0xFFC0CB),
        ("plum", 0xDDA0DD),
        ("powderblue", 0xB0E0E6),
        ("purple", 0x800080),
        ("red", 0xFF0000),
        ("rosybrown", 0xBC8F8F),
        ("royalblue", 0x4169E1),
        ("saddlebrown", 0x8B4513),
        ("salmon", 0xFA8072),
        ("sandybrown", 0xF4A460),
        ("seagreen", 0x2E8B57),
        ("seashell", 0xFFF5EE),
        ("sienna", 0xA0522D),
        ("silver", 0xC0C0C0),
        ("skyblue", 0x87CEEB),
        ("slateblue", 0x6A5ACD),
        ("slategray", 0x708090),
        ("snow", 0xFFFAFA),
        ("springgreen", 0x00FF7F),
        ("steelblue", 0x4682B4),
        ("tan", 0xD2B48C),
        ("teal", 0x008080),
        ("thistle", 0xD8BFD8),
        ("tomato", 0xFF6347),
        ("turquoise", 0x40E0D0),
        ("violet", 0xEE82EE),
        ("wheat", 0xF5DEB3),
        ("white", 0xFFFFFF),
        ("whitesmoke", 0xF5F5F5),
        ("yellow", 0xFFFF00),
        ("yellowgreen", 0x9ACD32),
    ];
    NAMES.iter().find(|(n, _)| n.eq_ignore_ascii_case(s)).map(|&(_, c)| c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_colours() {
        assert_eq!(parse_color("#FF00ff"), Some(0xFF00FF));
        assert_eq!(parse_color("00FF00"), Some(0x00FF00));
        assert_eq!(parse_color("#f0a"), Some(0xFF00AA));
        assert_eq!(parse_color("#80112233"), Some(0x112233));
        assert_eq!(parse_color("Black"), Some(0));
        assert_eq!(parse_color("none"), None);
        assert_eq!(parse_color("bogus"), None);
    }

    #[test]
    fn tints_keep_grey_and_rotate_hue() {
        assert_eq!(tint_rgb(0x808080, 0.25, 1.0), 0x808080);
        assert_eq!(tint_rgb(0xFF0000, 1.0 / 3.0, 1.0), 0x00FF00);
        assert_eq!(tint_rgb(0xFF0000, 0.0, 0.0), 0x808080);
    }

    #[test]
    fn keys_out_the_transparency_colour() {
        let mut data = Vec::new();
        let img = image::RgbImage::from_fn(2, 1, |x, _| if x == 0 { image::Rgb([255, 0, 255]) } else { image::Rgb([1, 2, 3]) });
        image::DynamicImage::ImageRgb8(img).write_to(&mut std::io::Cursor::new(&mut data), image::ImageFormat::Bmp).unwrap();
        let d = decode_image(&data).unwrap();
        assert_eq!(*d.frames[0], vec![0xFFFF00FF, 0xFF010203]);
    }

    #[test]
    fn frames_last_as_long_as_wmp_showed_them() {
        let mut data = Vec::new();
        let mut gif = gif::Encoder::new(&mut data, 1, 1, &[0, 0, 0, 90, 90, 90]).unwrap();
        gif.set_repeat(gif::Repeat::Infinite).unwrap();
        for (i, centiseconds) in [0, 5, 10, 15].into_iter().enumerate() {
            let mut frame = gif::Frame::from_indexed_pixels(1, 1, vec![(i % 2) as u8], None);
            frame.delay = centiseconds;
            gif.write_frame(&frame).unwrap();
        }
        drop(gif);
        assert_eq!(decode_image(&data).unwrap().delays, vec![47, 63, 109, 156]);
    }

    fn played_once(disposals: [gif::DisposalMethod; 2]) -> Decoded {
        let mut data = Vec::new();
        let mut gif = gif::Encoder::new(&mut data, 1, 1, &[0, 0, 0, 90, 90, 90]).unwrap();
        for dispose in disposals {
            let mut frame = gif::Frame::from_indexed_pixels(1, 1, vec![1], Some(0));
            frame.delay = 10;
            frame.dispose = dispose;
            gif.write_frame(&frame).unwrap();
        }
        drop(gif);
        decode_image(&data).unwrap()
    }

    #[test]
    fn a_finite_animation_clears_when_only_its_last_frame_asks() {
        use gif::DisposalMethod::{Background, Keep};
        let cleared = played_once([Keep, Background]);
        assert_eq!(cleared.loops, 1);
        assert_eq!(**cleared.frames.last().unwrap(), vec![0], "cleared once played");
        assert_eq!(*cleared.delays.last().unwrap(), 0);
        let kept = played_once([Background, Background]);
        assert_eq!(kept.frames.len(), 2);
        assert_ne!(**kept.frames.last().unwrap(), vec![0], "every frame asked: the last one stays");
    }

    #[test]
    fn frame_timing_loops_and_finishes() {
        let f = || Pixmap::new(1, 1).unwrap();
        let b = SkinBitmap::new(1, 1, Rc::new(vec![0]), vec![f(), f()], vec![100, 50], 2);
        assert_eq!(b.frame_at(120.0).1, Some(30.0));
        assert_eq!(b.frame_at(160.0).1, Some(90.0));
        assert_eq!(b.frame_at(400.0).1, None);
    }
}
