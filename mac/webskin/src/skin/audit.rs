//! Faults the corpus runner looks for in each window it saves, so a skin that draws wrong is found
//! before anyone applies it: a key colour left showing, a control nobody can click, and how far
//! the window is from the screenshot the skin's author took in WMP.

use super::element::preset;
use super::{Kind, Shared, Skin, ViewId};
use crate::images::{SkinBitmap, matches_key, parse_color};
use crate::render::{Canvas, Rect, pixel_at};
use crate::value::Value;
use std::collections::HashMap;
use tiny_skia::{Pixmap, PremultipliedColorU8};

fn rgb(p: PremultipliedColorU8) -> u32 {
    (u32::from(p.red()) << 16) | (u32::from(p.green()) << 8) | u32::from(p.blue())
}

/// The part of `e` its clipping ancestors and the window leave on screen.
fn on_screen(s: &Skin, v: ViewId, e: usize) -> Rect {
    let mut area = s.bounds(v, e).intersect(&s.bounds(v, 0));
    let mut cur = s.el(v, e).parent;
    while let Some(p) = cur {
        if s.clips_children(v, p) {
            area = area.intersect(&s.bounds(v, p));
        }
        cur = s.el(v, p).parent;
    }
    area
}

fn pixels(area: Rect) -> impl Iterator<Item = (i32, i32)> {
    let (x0, y0, x1, y1) = (area.x.ceil() as i32, area.y.ceil() as i32, area.right().floor() as i32, area.bottom().floor() as i32);
    (y0..y1).flat_map(move |y| (x0..x1).map(move |x| (x, y)))
}

/// Where effects and video draw: WMP put those in windows of their own, never keyed.
fn hosted(s: &Skin, v: ViewId) -> Vec<Rect> {
    (1..s.view(v).elements.len())
        .filter(|&e| matches!(s.el(v, e).kind, Kind::Effects | Kind::Video) && s.effectively_visible(v, e))
        .map(|e| on_screen(s, v, e))
        .collect()
}

/// Opaque pixels in an element's key colour where its own picture is that colour, no background
/// drawn later covers it, and nothing lies under it: WMP's window was see-through there, so the
/// colour came from this element or one drawn over it. (Over something solid WMP kept it: Hard
/// Boiled's white egg is drawn over a white-keyed mask.)
pub fn key_colour_left(shared: &Shared, v: ViewId, c: &Canvas) -> Vec<String> {
    let shows = |canvas: &Canvas, x: i32, y: i32, k: u32| {
        let (cx, cy) = (x - canvas.origin.0 as i32, y - canvas.origin.1 as i32);
        pixel_at(&canvas.pixmap, cx, cy).is_some_and(|p| p.alpha() == 255 && matches_key(rgb(p), k))
    };
    let suspects: Vec<Suspect> = shared.read(|s| {
        let hosted = hosted(s, v);
        let surfaces = surfaces(s, v);
        (1..s.view(v).elements.len())
            .filter(|&e| s.effectively_visible(v, e))
            .filter_map(|e| {
                let (k, own) = own_key(s, v, e)?;
                let after = surfaces.iter().position(|&(i, stage)| i == e && stage == Stage::Image).map_or(0, |i| i + 1);
                let hits: Vec<(i32, i32)> = pixels(on_screen(s, v, e))
                    .filter(|&(x, y)| {
                        let (px, py) = (f64::from(x) + 0.5, f64::from(y) + 0.5);
                        own(px, py)
                            && shows(c, x, y, k)
                            && !hosted.iter().any(|r| r.contains(px, py))
                            && !surfaces[after..].iter().any(|&(i, stage)| covers(s, v, i, stage, px, py))
                    })
                    .collect();
                (!hits.is_empty()).then_some(Suspect { element: e, key: k, pixels: hits })
            })
            .collect()
    });
    let mut found = vec![];
    for Suspect { element: e, key: k, pixels: hits } in suspects {
        let (order, area) = shared.read(|s| (paint_order(s, v), on_screen(s, v, e)));
        let ancestors: Vec<usize> = shared.read(|s| std::iter::successors(s.el(v, e).parent, |&p| s.el(v, p).parent).collect());
        let at = order.iter().position(|&i| i == e).unwrap_or(0);
        let hide: Vec<usize> = order[at..].iter().copied().filter(|i| !ancestors.contains(i)).collect();
        let Some(below) = render_hiding(shared, v, &hide, area) else { continue };
        let open = |x: i32, y: i32| pixel_at(&below.pixmap, x - below.origin.0 as i32, y - below.origin.1 as i32).is_none_or(|p| p.alpha() == 0);
        let hits: Vec<(i32, i32)> = hits.into_iter().filter(|&(x, y)| open(x, y)).collect();
        if let Some(r) = extent(&hits) {
            let name = shared.read(|s| describe(s, v, e));
            found.push(format!("key colour #{k:06X} of {name} shows in {} pixels at {r}", hits.len()));
        }
    }
    found
}

/// An element whose key colour shows where its own picture has that colour.
struct Suspect {
    element: usize,
    key: u32,
    pixels: Vec<(i32, i32)>,
}

/// An element's key colour and, for a point in the view, whether its own picture is that colour
/// there.
type OwnKey<'a> = (u32, Box<dyn Fn(f64, f64) -> bool + 'a>);

/// What a subview draws that can cover a hole under it.
#[derive(Clone, Copy, PartialEq)]
enum Stage {
    /// Its backgroundColor, under the children behind its image.
    Fill,
    Image,
}

/// Every element's fill and image in the order they draw (other elements draw no background).
fn surfaces(s: &Skin, v: ViewId) -> Vec<(usize, Stage)> {
    fn visit(s: &Skin, v: ViewId, e: usize, out: &mut Vec<(usize, Stage)>) {
        if e != 0 && !s.visible(v, e) {
            return;
        }
        out.push((e, Stage::Fill));
        let (behind, above) = s.children_behind_and_above(v, e);
        for &c in &behind {
            visit(s, v, c, out);
        }
        out.push((e, Stage::Image));
        for c in above {
            visit(s, v, c, out);
        }
    }
    let mut out = vec![];
    visit(s, v, 0, &mut out);
    out
}

/// Whether element `e`'s fill or image covers view point (x, y).
fn covers(s: &Skin, v: ViewId, e: usize, stage: Stage, x: f64, y: f64) -> bool {
    if !matches!(s.el(v, e).kind, Kind::View | Kind::Subview) || !on_screen(s, v, e).contains(x, y) {
        return false;
    }
    let b = s.bounds(v, e);
    match stage {
        Stage::Fill => parse_color(&s.get_str(v, e, "backgroundcolor")).is_some(),
        Stage::Image => s.background_covers(v, e, x - b.x, y - b.y),
    }
}

/// The view's elements in the order they draw: a container, the children behind its image, then
/// those above.
fn paint_order(s: &Skin, v: ViewId) -> Vec<usize> {
    fn visit(s: &Skin, v: ViewId, e: usize, out: &mut Vec<usize>) {
        out.push(e);
        let (behind, above) = s.children_behind_and_above(v, e);
        for c in behind.into_iter().chain(above) {
            visit(s, v, c, out);
        }
    }
    let mut out = vec![];
    visit(s, v, 0, &mut out);
    out
}

fn own_key<'a>(s: &'a Skin, v: ViewId, e: usize) -> Option<OwnKey<'a>> {
    let clipping = || {
        (s.el(v, e).kind == Kind::Subview && s.get_str(v, e, "clippingimage").is_empty())
            .then(|| parse_color(&s.get_str(v, e, "clippingcolor")))
            .flatten()
    };
    let k = parse_color(&s.get_str(v, e, "transparencycolor")).or_else(clipping)?;
    let b = s.bounds(v, e);
    let at = |raw: std::rc::Rc<SkinBitmap>, origin: (f64, f64)| -> Box<dyn Fn(f64, f64) -> bool + 'a> {
        Box::new(move |x, y| raw.rgb((x - origin.0).floor() as i32, (y - origin.1).floor() as i32).is_some_and(|p| matches_key(p, k)))
    };
    match s.el(v, e).kind {
        Kind::Subview => {
            let raw = s.images.get_raw(&s.get_str(v, e, "backgroundimage"))?;
            let at = s.background_pixel(v, e, &raw);
            Some((
                k,
                Box::new(move |x, y| {
                    let (ix, iy) = at(x - b.x, y - b.y);
                    raw.rgb(ix, iy).is_some_and(|p| matches_key(p, k))
                }),
            ))
        }
        Kind::Button | Kind::ButtonGroup => Some((k, at(s.images.get_raw(&s.get_str(v, e, "image"))?, (b.x, b.y)))),
        Kind::Slider => {
            let thumb = s.slider_thumb_rect(v, e)?;
            Some((k, at(s.images.get_raw(&s.get_str(v, e, "thumbimage"))?, (b.x + thumb.x, b.y + thumb.y))))
        }
        _ => None,
    }
}

/// Controls that show in the window but take no click at any pixel they show: something
/// transparent to the eye lies over them, or the window is see-through there. A control another
/// part covers (a closed drawer, an intro screen) shows nowhere and is left alone, and so is what
/// shows through a slider: a slider takes clicks across its whole rectangle, like a Windows
/// trackbar.
pub fn controls_out_of_reach(shared: &Shared, v: ViewId, c: &Canvas) -> Vec<String> {
    let controls: Vec<(usize, usize, Rect)> = shared.read(|s| {
        (1..s.view(v).elements.len())
            .filter(|&e| takes_clicks(s, v, e) && s.effectively_visible(v, e))
            .map(|e| {
                let owner = if s.el(v, e).kind == Kind::GroupButton { s.el(v, e).parent.unwrap_or(e) } else { e };
                (e, owner, on_screen(s, v, owner))
            })
            .filter(|(_, _, area)| !area.is_empty())
            .collect()
    });
    let mut found = vec![];
    for (e, owner, area) in controls {
        let Some(without) = shown_without(shared, v, e, area) else { continue };
        let report = shared.read(|s| {
            let b = s.bounds(v, owner);
            let own = |x: i32, y: i32| {
                let (lx, ly) = (f64::from(x) + 0.5 - b.x, f64::from(y) + 0.5 - b.y);
                if owner != e { s.group_element_at(v, owner, lx, ly) == Some(e) } else { s.hit_test(v, e, lx, ly) }
            };
            let differs = |x: i32, y: i32| pixel_at(&c.pixmap, x, y) != pixel_at(&without.pixmap, x - area.x as i32, y - area.y as i32);
            let mut takers: HashMap<Option<usize>, usize> = HashMap::new();
            for (x, y) in pixels(area).filter(|&(x, y)| own(x, y) && differs(x, y)) {
                let taker = s.hit_test_point(v, f64::from(x) + 0.5, f64::from(y) + 0.5);
                if taker.is_some_and(|t| t != e && s.el(v, t).kind == Kind::Slider) {
                    continue;
                }
                let opaque = pixel_at(&c.pixmap, x, y).is_some_and(|p| p.alpha() > 0);
                if taker == Some(e) && opaque {
                    return None;
                }
                *takers.entry(if opaque { taker } else { None }).or_default() += 1;
            }
            let shown: usize = takers.values().sum();
            let (taker, _) = takers.into_iter().max_by_key(|&(_, n)| n)?;
            let by = taker.map_or_else(|| "see-through window".to_string(), |t| describe(s, v, t));
            Some(format!("{} shows in {shown} pixels but can't be clicked: {by} takes them", describe(s, v, e)))
        });
        found.extend(report);
    }
    found
}

/// A control meant to be clicked now: enabled, not passing clicks through (Scooby-Doo's second
/// seek bar mirrors the first), and a button only when clicking it does something.
fn takes_clicks(s: &Skin, v: ViewId, e: usize) -> bool {
    let kind = s.el(v, e).kind;
    let acts = || preset(&s.el(v, e).tag).is_some() || ["onclick", "onmousedown", "onmouseup", "ondblclick"].iter().any(|h| s.has_handler(v, e, h));
    matches!(kind, Kind::Button | Kind::GroupButton | Kind::Slider | Kind::CustomSlider)
        && !s.passes_clicks(v, e)
        && s.enabled(v, e)
        && (matches!(kind, Kind::Slider | Kind::CustomSlider) || acts())
}

/// `area` of the window drawn with element `e` hidden.
fn shown_without(shared: &Shared, v: ViewId, e: usize, area: Rect) -> Option<Canvas> {
    render_hiding(shared, v, &[e], area)
}

/// `area` of the window drawn with `hide` hidden.
fn render_hiding(shared: &Shared, v: ViewId, hide: &[usize], area: Rect) -> Option<Canvas> {
    let key = "visible".to_string();
    let before: Vec<Option<Value>> = shared.with(|s| hide.iter().map(|&e| s.el_mut(v, e).values.insert(key.clone(), Value::Bool(false))).collect());
    let drawn = shared.read(|s| s.render_region(v, 1.0, area));
    shared.with(|s| {
        for (&e, b) in hide.iter().zip(before) {
            match b {
                Some(b) => s.el_mut(v, e).values.insert(key.clone(), b),
                None => s.el_mut(v, e).values.remove(&key),
            };
        }
    });
    drawn
}

fn describe(s: &Skin, v: ViewId, e: usize) -> String {
    let el = s.el(v, e);
    match el.id() {
        Some(id) => format!("<{} id={id}>", el.tag),
        None => format!("<{}> #{e}", el.tag),
    }
}

fn extent(points: &[(i32, i32)]) -> Option<String> {
    let (x0, y0) = points.iter().fold((i32::MAX, i32::MAX), |(a, b), &(x, y)| (a.min(x), b.min(y)));
    let (x1, y1) = points.iter().fold((i32::MIN, i32::MIN), |(a, b), &(x, y)| (a.max(x), b.max(y)));
    (!points.is_empty()).then(|| format!("({x0},{y0})-({x1},{y1})"))
}

/// How a saved window compares with the skin's own preview screenshot, placed where they fit best.
pub struct PreviewMatch {
    /// Of the pixels either one shows, the share that agree.
    pub score: f64,
    /// Pixels the window shows where the preview is clear (or the other way round): None when the
    /// preview has no transparency to compare.
    pub extra: Option<usize>,
    pub missing: Option<usize>,
    /// The preview, the window and a map of where they differ, side by side.
    pub picture: Pixmap,
}

const CLOSE: i32 = 72;

fn close(a: u32, b: u32) -> bool {
    let ch = |c: u32, s: u32| ((c >> s) & 255) as i32;
    (ch(a, 16) - ch(b, 16)).abs() + (ch(a, 8) - ch(b, 8)).abs() + (ch(a, 0) - ch(b, 0)).abs() <= CLOSE
}

pub fn compare_preview(preview: &SkinBitmap, window: &Pixmap) -> Option<PreviewMatch> {
    let (pw, ph) = (preview.width as i32, preview.height as i32);
    let (ww, wh) = (window.width() as i32, window.height() as i32);
    let has_alpha = preview.original.iter().any(|&p| p >> 24 < 255);
    let prev = |x: i32, y: i32| -> Option<u32> {
        (x >= 0 && y >= 0 && x < pw && y < ph).then(|| preview.original[(y * pw + x) as usize]).filter(|p| p >> 24 >= 128).map(|p| p & 0xFF_FFFF)
    };
    let ours = |x: i32, y: i32| -> Option<u32> { pixel_at(window, x, y).filter(|p| p.alpha() >= 128).map(rgb) };
    let agree = |dx: i32, dy: i32, step: i32| -> usize {
        (0..wh)
            .step_by(step as usize)
            .flat_map(|y| (0..ww).step_by(step as usize).map(move |x| (x, y)))
            .filter(|&(x, y)| ours(x, y).zip(prev(x + dx, y + dy)).is_some_and(|(a, b)| close(a, b)))
            .count()
    };
    let slack = (ww.min(wh) / 4).max(4);
    let best = |candidates: Vec<(i32, i32)>, step: i32| candidates.into_iter().max_by_key(|&(dx, dy)| agree(dx, dy, step));
    let coarse: Vec<(i32, i32)> =
        (-slack..=(pw - ww + slack)).step_by(3).flat_map(|dx| (-slack..=(ph - wh + slack)).step_by(3).map(move |dy| (dx, dy))).collect();
    let (cx, cy) = best(coarse, 4)?;
    let (dx, dy) = best((cx - 3..=cx + 3).flat_map(|dx| (cy - 3..=cy + 3).map(move |dy| (dx, dy))).collect(), 1)?;

    let mut picture = Pixmap::new((ww * 3 + 8) as u32, wh as u32)?;
    let (mut agreeing, mut differing, mut extra, mut missing) = (0usize, 0usize, 0usize, 0usize);
    let put = |pm: &mut Pixmap, x: i32, y: i32, c: u32| {
        let w = pm.width() as i32;
        pm.pixels_mut()[(y * w + x) as usize] = PremultipliedColorU8::from_rgba((c >> 16) as u8, (c >> 8) as u8, c as u8, 255).expect("opaque");
    };
    for y in 0..wh {
        for x in 0..ww {
            let (a, b) = (ours(x, y), prev(x + dx, y + dy));
            if let Some(b) = b {
                put(&mut picture, x, y, b);
            }
            if let Some(a) = a {
                put(&mut picture, ww + 4 + x, y, a);
            }
            let mark = match (a, b) {
                (Some(a), Some(b)) if close(a, b) => {
                    agreeing += 1;
                    Some(0x30_6030)
                }
                (Some(_), Some(_)) => {
                    differing += 1;
                    Some(0xFF_D000)
                }
                (Some(_), None) if has_alpha => {
                    extra += 1;
                    Some(0xFF_2020)
                }
                (None, Some(_)) if has_alpha => {
                    missing += 1;
                    Some(0x30_60FF)
                }
                _ => None,
            };
            if let Some(m) = mark {
                put(&mut picture, 2 * (ww + 4) + x, y, m);
            }
        }
    }
    let total = agreeing + differing + extra + missing;
    Some(PreviewMatch {
        score: if total == 0 { 0.0 } else { agreeing as f64 / total as f64 },
        extra: has_alpha.then_some(extra),
        missing: has_alpha.then_some(missing),
        picture,
    })
}
