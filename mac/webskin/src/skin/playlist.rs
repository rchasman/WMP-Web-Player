//! PLAYLIST: WMP drew it as Windows controls, a combo box naming the playlist over a details
//! list in Segoe UI 9 pt, whatever the skin's font. The sizes and colours here were measured on
//! real WMP on a GitHub runner (tools/wmp/probes/playlists and playlist-columns).

use super::{Skin, TrackInfo, ViewId};
use crate::images::parse_color;
use crate::render::{Canvas, Rect};
use crate::text::{self, TextStyle};
use crate::value::ObjRef;

const COMBO: f64 = 24.0;
const TOOLBAR: f64 = 29.0;
const HEADER: f64 = 24.0;
const ROW: f64 = 19.0;
const SCROLLBAR: f64 = 17.0;
/// The space a list cell keeps around its text, a header label's wider margin, and the playing
/// band's left gap.
const PAD: f64 = 6.0;
const HEADER_PAD: f64 = 8.0;
const BAND_INSET: f64 = 4.0;
/// WMP's columns when a skin names none.
const DEFAULT_COLUMNS: &str = "name=Title;duration=Length;artist=Artist;genre=Genre";

fn list_style() -> TextStyle {
    TextStyle::new("Segoe UI", 9.0, "normal")
}

pub struct Column {
    pub field: String,
    pub label: String,
    pub width: f64,
}

/// Where a PLAYLIST's parts sit inside its box, top to bottom.
pub struct PlaylistLayout {
    pub combo: Option<Rect>,
    pub toolbar: Option<Rect>,
    pub header: Option<Rect>,
    pub rows: Rect,
    pub columns: Vec<Column>,
    pub first: usize,
    pub last_first: usize,
    pub vscroll: Option<Rect>,
    pub hscroll: Option<Rect>,
}

/// A track's text in a column; WMP leaves fields it doesn't know about empty.
fn cell(track: &TrackInfo, field: &str) -> String {
    match field {
        "name" | "title" => track.title.clone(),
        "artist" | "author" => track.artist.clone(),
        "album" | "wm/albumtitle" => track.album.clone(),
        "duration" if track.duration > 0.0 => list_time(track.duration),
        _ => String::new(),
    }
}

/// "0:40", "12:05", "1:02:05": a list's lengths have no leading zero.
fn list_time(seconds: f64) -> String {
    let s = seconds.round() as u64;
    match (s / 3600, s / 60 % 60, s % 60) {
        (0, m, s) => format!("{m}:{s:02}"),
        (h, m, s) => format!("{h}:{m:02}:{s:02}"),
    }
}

fn right_aligned(field: &str) -> bool {
    field == "duration"
}

impl Skin {
    /// With its items hidden and no height given, WMP sized a PLAYLIST to its combo and toolbar.
    pub fn playlist_bar_height(&self, v: ViewId, e: usize) -> i32 {
        if self.get_bool(v, e, "playlistitemsvisible") {
            return 0;
        }
        let part = |attr: &str, h: f64| if self.get_bool(v, e, attr) { h as i32 } else { 0 };
        part("dropdownvisible", COMBO) + part("toolbarvisible", TOOLBAR)
    }

    pub fn playlist_layout(&self, v: ViewId, e: usize, b: Rect) -> PlaylistLayout {
        let mut y = b.y;
        let mut take = |on: bool, h: f64| {
            let part = on.then(|| Rect::new(b.x, y, b.w, h));
            y += if on { h } else { 0.0 };
            part
        };
        let combo = take(self.get_bool(v, e, "dropdownvisible"), COMBO);
        let toolbar = take(self.get_bool(v, e, "toolbarvisible"), TOOLBAR);
        let items_shown = self.get_bool(v, e, "playlistitemsvisible");
        let header = take(items_shown && self.get_bool(v, e, "columnsvisible"), HEADER);
        let list_top = header.map_or(y, |h| h.y);
        let mut rows = Rect::new(b.x, y, b.w, if items_shown { (b.bottom() - y).max(0.0) } else { 0.0 });

        let items = &self.history;
        let scrolled = self.el(v, e).state.playlist_top;
        // The first row shown, and the first row that keeps the newest track in view.
        let window = |h: f64| {
            let last_first = items.len().saturating_sub((h / ROW).floor() as usize);
            (scrolled.map_or(last_first, |top| top.min(last_first)), last_first)
        };
        let (first, _) = window(rows.h);
        let fits = (rows.h / ROW).floor() as usize;
        let style = list_style();
        let measure = |s: &str| f64::from(text::measure(s, &style, None).0);
        let spec = self.get_str(v, e, "columns");
        let spec = if spec.trim().is_empty() { DEFAULT_COLUMNS.to_string() } else { spec };
        // A Windows list sizes each column to its header and the rows in view.
        let columns: Vec<Column> = spec
            .split(';')
            .filter_map(|part| part.split_once('='))
            .map(|(field, label)| {
                let field = field.trim().to_lowercase();
                let label = label.trim().to_string();
                let cells = items.iter().skip(first).take(fits).map(|t| measure(&cell(t, &field)) + 2.0 * PAD);
                let width = cells.fold(measure(&label) + 2.0 * HEADER_PAD, f64::max).round();
                Column { field, label, width }
            })
            .collect();

        let total: f64 = columns.iter().map(|c| c.width).sum();
        let vertical = items_shown && items.len() as f64 * ROW > rows.h;
        let room = b.w - if vertical { SCROLLBAR } else { 0.0 };
        let hscroll = (items_shown && total > room).then(|| Rect::new(b.x, b.bottom() - SCROLLBAR, room, SCROLLBAR));
        if hscroll.is_some() {
            rows.h = (rows.h - SCROLLBAR).max(0.0);
        }
        let (first, last_first) = window(rows.h);
        let vscroll = vertical.then(|| Rect::new(b.right() - SCROLLBAR, list_top, SCROLLBAR, rows.bottom() - list_top));
        PlaylistLayout { combo, toolbar, header, rows, columns, first, last_first, vscroll, hscroll }
    }

    /// One row per wheel step. Scrolling back to the end follows the newest track again.
    pub fn scroll_playlist(&mut self, v: ViewId, e: usize, delta: f64) {
        let b = Rect::new(0.0, 0.0, f64::from(self.width(v, e)), f64::from(self.height(v, e)));
        let layout = self.playlist_layout(v, e, b);
        let first = if delta > 0.0 { layout.first.saturating_sub(1) } else { layout.first + 1 };
        self.el_mut(v, e).state.playlist_top = (first < layout.last_first).then_some(first);
        self.view_mut(v).invalidate();
    }

    pub fn render_playlist(&self, v: ViewId, e: usize, c: &mut Canvas, b: Rect, clip: Rect) {
        if b.w < 4.0 || b.h < 4.0 {
            return;
        }
        let clip = clip.intersect(&b);
        let color = |attr: &str, fallback: u32| parse_color(&self.get_str(v, e, attr)).unwrap_or(fallback);
        let layout = self.playlist_layout(v, e, b);
        let style = list_style();
        let draw = |c: &mut Canvas, s: &str, x: f64, y: f64, rgb: u32, clip: Rect| {
            c.draw_text(&text::rasterize(s, &style, c.scale, None), x, y, rgb, clip);
        };
        c.fill(b, color("backgroundcolor", 0xFF_FFFF), clip);
        if let Some(bg) = self.img(v, e, "backgroundimage") {
            c.draw_image(bg.source(), Rect::new(b.x, b.y, f64::from(bg.width), f64::from(bg.height)), None, clip, 1.0);
        }
        if let Some(r) = layout.combo {
            let name = self.get(ObjRef::CurrentPlaylist, "name").to_str();
            draw_combo(c, r, clip, &name, &draw);
        }
        if let Some(r) = layout.toolbar {
            let name = self.get(ObjRef::CurrentPlaylist, "name").to_str();
            draw_toolbar(c, r, clip, &name, &draw);
        }
        if let Some(h) = layout.header {
            let hclip = clip.intersect(&h);
            c.fill(h, 0xFF_FFFF, hclip);
            let mut x = h.x;
            for col in &layout.columns {
                let cellr = Rect::new(x, h.y, col.width, h.h).intersect(&hclip);
                let label = text::ellipsize_with(&col.label, &style, text_room(col.width), "...");
                draw(c, &label, text_x(&col.field, &label, x, col.width, &style), h.y + 4.0, 0, cellr);
                c.fill(Rect::new(x + col.width - 1.0, h.y, 1.0, h.h), 0xE5_E5E5, hclip);
                x += col.width;
            }
        }

        let rows = layout.rows;
        let rclip = clip.intersect(&rows);
        let total: f64 = layout.columns.iter().map(|c| c.width).sum();
        let (fg, playing) = (color("foregroundcolor", 0), color("itemplayingcolor", 0x00_FF00));
        let band = color("itemplayingbackgroundcolor", 0x22_2222);
        let items = &self.history;
        for (i, track) in items.iter().enumerate().skip(layout.first) {
            let y = rows.y + (i - layout.first) as f64 * ROW;
            if y >= rows.bottom() {
                break;
            }
            let current = i + 1 == items.len();
            if current {
                c.fill(Rect::new(rows.x + BAND_INSET, y, (total - BAND_INSET).max(0.0), ROW), band, rclip);
            }
            let mut x = rows.x;
            for col in &layout.columns {
                let s = cell(track, &col.field);
                if !s.is_empty() {
                    let s = text::ellipsize_with(&s, &style, text_room(col.width), "...");
                    let cellr = Rect::new(x, y, col.width, ROW).intersect(&rclip);
                    draw(c, &s, text_x(&col.field, &s, x, col.width, &style), y + 2.0, if current { playing } else { fg }, cellr);
                }
                x += col.width;
            }
        }
        if items.is_empty() && rows.h >= ROW {
            let hint = text::ellipsize("Play something in Spotify, Music or your browser", &style, (rows.w - 2.0 * PAD).max(1.0) as f32);
            draw(c, &hint, rows.x + PAD, rows.y + 2.0, fg, rclip);
        }
        let shown = rows.h / ROW;
        if let Some(r) = layout.vscroll {
            draw_scrollbar(c, r, clip, true, layout.first as f64, shown, items.len() as f64);
        }
        if let Some(r) = layout.hscroll {
            draw_scrollbar(c, r, clip, false, 0.0, r.w, total);
        }
        if let (Some(vs), Some(hs)) = (layout.vscroll, layout.hscroll) {
            c.fill(Rect::new(vs.x, hs.y, vs.w, hs.h), 0xF0_F0F0, clip);
        }
    }
}

/// The width a cell's text may take before it ends in "...". The column was rounded from the
/// same measure, so allow that rounding.
fn text_room(width: f64) -> f32 {
    (width - 2.0 * PAD + 0.5).max(1.0) as f32
}

fn text_x(field: &str, s: &str, x: f64, width: f64, style: &TextStyle) -> f64 {
    if right_aligned(field) { x + width - PAD - f64::from(text::measure(s, style, None).0) } else { x + PAD }
}

type DrawText<'a> = dyn Fn(&mut Canvas, &str, f64, f64, u32, Rect) + 'a;

/// A Windows 11 combo box: a one-pixel frame with a darker bottom edge, the playlist icon, the
/// name and a chevron.
fn draw_combo(c: &mut Canvas, r: Rect, clip: Rect, name: &str, draw: &DrawText) {
    let clip = clip.intersect(&r);
    c.fill(r, 0xD2_D2D2, clip);
    c.fill(Rect::new(r.x, r.bottom() - 1.0, r.w, 1.0), 0xBC_BCBC, clip);
    for (x, y) in [(r.x, r.y), (r.right() - 1.0, r.y), (r.x, r.bottom() - 1.0), (r.right() - 1.0, r.bottom() - 1.0)] {
        c.fill(Rect::new(x, y, 1.0, 1.0), 0xE6_E6E6, clip);
    }
    c.fill(Rect::new(r.x + 1.0, r.y + 1.0, r.w - 2.0, r.h - 2.0), 0xFD_FDFD, clip);
    draw_playlist_icon(c, r.x + 6.0, r.y + 4.0, clip);
    let text_clip = clip.intersect(&Rect::new(r.x + 24.0, r.y, (r.w - 24.0 - 17.0).max(0.0), r.h));
    draw(c, name, r.x + 24.0, r.y + 5.0, 0, text_clip);
    let (cx, cy) = (r.right() - 13.0, r.y + 10.0);
    for k in 0..4 {
        let k = f64::from(k);
        c.fill(Rect::new(cx + k, cy + k, 1.0, 1.0), 0x6E_6E6E, clip);
        c.fill(Rect::new(cx + 7.0 - k, cy + k, 1.0, 1.0), 0x6E_6E6E, clip);
    }
}

/// The page with a play triangle that WMP put before a playlist's name.
fn draw_playlist_icon(c: &mut Canvas, x: f64, y: f64, clip: Rect) {
    c.fill(Rect::new(x, y, 11.0, 13.0), 0x75_8899, clip);
    c.fill(Rect::new(x + 1.0, y + 1.0, 9.0, 11.0), 0xEE_F2F8, clip);
    for (row, len) in [(4.0, 2.0), (5.0, 3.0), (6.0, 4.0), (7.0, 3.0), (8.0, 2.0)] {
        c.fill(Rect::new(x + 3.0, y + row, len, 1.0), 0x6E_8BA5, clip);
    }
}

/// The light blue strip under the combo with the playlist's name and a menu arrow.
fn draw_toolbar(c: &mut Canvas, r: Rect, clip: Rect, name: &str, draw: &DrawText) {
    let clip = clip.intersect(&r);
    c.fill(Rect::new(r.x, r.y, r.w, r.h - 3.0), 0xDF_EAF7, clip);
    c.fill(Rect::new(r.x, r.bottom() - 3.0, r.w, 2.0), 0xDC_DFE0, clip);
    c.fill(Rect::new(r.x, r.bottom() - 1.0, r.w, 1.0), 0xA9_B7C9, clip);
    draw(c, name, r.x + 9.0, r.y + 6.0, 0, clip);
    let x = r.x + 9.0 + f64::from(text::measure(name, &list_style(), None).0) + 8.0;
    for k in 0..4 {
        let k = f64::from(k);
        c.fill(Rect::new(x + k, r.y + 12.0 + k, 7.0 - 2.0 * k, 1.0), 0, clip);
    }
}

/// Windows 11's thin scroll bar at rest: a light track with a two-pixel thumb line between the
/// arrow buttons.
fn draw_scrollbar(c: &mut Canvas, r: Rect, clip: Rect, vertical: bool, at: f64, shown: f64, total: f64) {
    let clip = clip.intersect(&r);
    c.fill(r, 0xF0_F0F0, clip);
    let span = if vertical { r.h } else { r.w } - 2.0 * SCROLLBAR;
    if span <= 0.0 || total <= 0.0 {
        return;
    }
    let len = (span * (shown / total).min(1.0)).max(8.0);
    let start = SCROLLBAR + (span - len) * (at / (total - shown).max(1.0)).min(1.0);
    let thumb = if vertical { Rect::new(r.x + 7.0, r.y + start, 2.0, len) } else { Rect::new(r.x + start, r.y + 7.0, len, 2.0) };
    c.fill(thumb, 0x85_8585, clip);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;
    use crate::skin::{NullHost, Shared};
    use std::rc::Rc;

    /// A view holding one PLAYLIST with `attrs`, its history set to `titles` (the last one playing).
    fn playlist(name: &str, attrs: &str, titles: &[&str]) -> (Rc<Shared>, ViewId, usize) {
        let dir = std::env::temp_dir().join(format!("webskin-test-playlist-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let wms = format!(r#"<THEME><VIEW id="Main" width="300" height="200"><PLAYLIST id="pl" width="300" height="200" {attrs}/></VIEW></THEME>"#);
        std::fs::write(dir.join("t.wms"), wms).unwrap();
        let shared = Shared::load(&dir.join("t.wms"), Box::new(crate::player::NoMedia), Settings::scratch(), Rc::new(NullHost), false).unwrap();
        let v = shared.load_main_view_offscreen();
        shared.with(|s| {
            s.history =
                titles.iter().map(|t| TrackInfo { title: t.to_string(), artist: String::new(), album: String::new(), duration: 40.0 }).collect();
        });
        let e = shared.read(|s| s.view(v).by_id["pl"]);
        (shared, v, e)
    }

    fn layout(shared: &Shared, v: ViewId, e: usize) -> PlaylistLayout {
        shared.read(|s| s.playlist_layout(v, e, Rect::new(0.0, 0.0, 300.0, 200.0)))
    }

    #[test]
    fn the_combo_header_and_rows_stack_at_windows_sizes() {
        let (shared, v, e) = playlist("stack", "", &["Glue"]);
        let l = layout(&shared, v, e);
        assert_eq!(l.combo.map(|r| (r.y, r.h)), Some((0.0, 24.0)), "the combo shows by default");
        assert_eq!(l.header.map(|r| (r.y, r.h)), Some((24.0, 24.0)), "so does the header");
        assert_eq!(l.rows.y, 48.0);
        let (shared, v, e) = playlist("bare", r#"dropDownVisible="false" columnsVisible="false""#, &["Glue"]);
        let l = layout(&shared, v, e);
        assert!(l.combo.is_none() && l.header.is_none());
        assert_eq!(l.rows.y, 0.0, "with both hidden the rows start at the top");
    }

    #[test]
    fn rows_stay_19_pixels_whatever_the_skins_font() {
        let (shared, v, e) =
            playlist("font", r#"dropDownVisible="false" columnsVisible="false" fontFace="Courier New" fontSize="20""#, &["Glue", "Opal"]);
        let pm = shared.read(|s| s.render_view(v, 1.0)).unwrap().pixmap;
        let band = |y: u32| pm.pixel(150, y).map(|p| (p.red(), p.green(), p.blue()));
        assert_eq!(band(18), Some((255, 255, 255)), "the first row ends at 19");
        assert_eq!(band(19), Some((0x22, 0x22, 0x22)), "the playing second row starts there");
        assert_eq!(band(37), Some((0x22, 0x22, 0x22)));
        assert_eq!(band(38), Some((255, 255, 255)));
        assert!(layout(&shared, v, e).columns.iter().all(|c| c.width < 60.0), "columns measured in Segoe UI 9 pt, not 20 pt Courier");
    }

    #[test]
    fn unset_colours_are_windows_white_black_and_green() {
        let (shared, v, _) = playlist("colours", r#"dropDownVisible="false" columnsVisible="false""#, &["Opal", "Glue"]);
        let pm = shared.read(|s| s.render_view(v, 1.0)).unwrap().pixmap;
        let rgb = |x: u32, y: u32| pm.pixel(x, y).map(|p| (p.red(), p.green(), p.blue())).unwrap();
        assert_eq!(rgb(250, 5), (255, 255, 255), "background");
        let row = |y0: u32| (0..19).flat_map(|y| (0..60).map(move |x| (x, y0 + y))).map(|(x, y)| rgb(x, y)).collect::<Vec<_>>();
        assert!(row(0).iter().any(|&(r, g, b)| r < 80 && g < 80 && b < 80), "dark text on the first row");
        assert!(row(19).iter().any(|&(r, g, b)| g > 200 && r < 80 && b < 80), "green text on the playing row");
        assert_eq!(rgb(2, 25), (255, 255, 255), "the band leaves a gap at the left");
        assert_eq!(rgb(5, 25), (0x22, 0x22, 0x22));
    }

    #[test]
    fn columns_fit_their_header_and_the_rows_in_view() {
        let (shared, v, e) = playlist("widths", r#"columns="name=Name;duration=Time""#, &["Glue", "Opal"]);
        let widths: Vec<f64> = layout(&shared, v, e).columns.iter().map(|c| c.width).collect();
        assert_eq!(widths, [48.0, 42.0], "as WMP sized Name and Time over short titles");
        let (shared, v, e) = playlist("wide", r#"columns="name=Name;duration=Time""#, &["Glue", "Late night"]);
        assert_eq!(layout(&shared, v, e).columns[0].width, 65.0, "as WMP sized Name over Late night");
    }

    #[test]
    fn default_columns_too_wide_for_the_box_get_a_horizontal_scroll_bar() {
        let (shared, v, e) = playlist("hscroll", "", &["A much longer song title for the list"]);
        let l = layout(&shared, v, e);
        let labels: Vec<&str> = l.columns.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, ["Title", "Length", "Artist", "Genre"]);
        assert_eq!(l.hscroll.map(|r| (r.y, r.h)), Some((183.0, 17.0)));
    }

    #[test]
    fn a_long_history_keeps_the_newest_track_in_view() {
        let titles: Vec<String> = (0..30).map(|i| format!("Song {i}")).collect();
        let titles: Vec<&str> = titles.iter().map(String::as_str).collect();
        let (shared, v, e) = playlist("follow", r#"dropDownVisible="false" columnsVisible="false""#, &titles);
        let l = layout(&shared, v, e);
        assert_eq!(l.first, 30 - 10, "ten rows fit in 200 px and the last is the newest");
        assert!(l.vscroll.is_some());
        shared.with(|s| s.scroll_playlist(v, e, 1.0));
        assert_eq!(layout(&shared, v, e).first, 19, "the wheel scrolls back one row");
        shared.with(|s| s.scroll_playlist(v, e, -1.0));
        shared.with(|s| s.history.push(TrackInfo { title: "New".into(), artist: String::new(), album: String::new(), duration: 40.0 }));
        assert_eq!(layout(&shared, v, e).first, 21, "back at the end it follows new tracks again");
    }

    #[test]
    fn hidden_items_with_no_height_leave_just_the_combo() {
        let dir = std::env::temp_dir().join(format!("webskin-test-playlist-bar-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("t.wms"),
            r#"<THEME><VIEW id="Main" width="300" height="200"><PLAYLIST id="pl" width="172" playlistItemsVisible="false"/></VIEW></THEME>"#,
        )
        .unwrap();
        let shared = Shared::load(&dir.join("t.wms"), Box::new(crate::player::NoMedia), Settings::scratch(), Rc::new(NullHost), false).unwrap();
        let v = shared.load_main_view_offscreen();
        let h = shared.read(|s| s.height(v, s.view(v).by_id["pl"]));
        assert_eq!(h, 24);
    }
}
