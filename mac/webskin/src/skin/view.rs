//! One VIEW of a skin: building its element tree and script context, bindings, timers,
//! animations, input and drawing.

use super::element::{self, Element, Kind};
use super::{MenuItem, Shared, Skin, ViewId, objects};
use crate::images::{matches_key, parse_color};
use crate::render::{Canvas, Rect};
use crate::resources;
use crate::script::{JsRef, ScriptHost};
use crate::value::{ObjRef, Value};
use crate::wms::WmsNode;
use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq)]
enum AnimKind {
    Move { x0: f64, y0: f64, x: f64, y: f64 },
    Alpha { a0: f64, a: f64 },
    Resize { w0: f64, h0: f64, w: f64, h: f64 },
}

struct Anim {
    element: usize,
    kind: AnimKind,
    ms: f64,
    start: Instant,
}

/// A wmpprop:/wmpenabled:/wmpdisabled: attribute following another object's property.
struct Binding {
    source: ObjRef,
    /// The lower-case property whose change re-applies it.
    watch: String,
    target: usize,
    attr: String,
    prop: String,
    kind: BindKind,
}

#[derive(PartialEq, Clone, Copy)]
enum BindKind {
    Prop,
    Enabled,
    Disabled,
}

pub struct View {
    pub id: Option<String>,
    pub elements: Vec<Element>,
    /// Elements by lower-case id.
    pub by_id: HashMap<String, usize>,
    bindings: Vec<Binding>,
    pub(super) object_handlers: Vec<(ObjRef, String, JsRef, Vec<ObjRef>)>,
    pub(super) player_handlers: Vec<(String, JsRef, Vec<ObjRef>)>,
    pub loading: bool,
    pub open: bool,
    /// Window position on screen.
    pub left: f64,
    pub top: f64,
    hover: Option<usize>,
    pressed: Option<usize>,
    focus: Option<usize>,
    anims: Vec<Anim>,
    timer_due: Option<Instant>,
    scrollers: HashMap<usize, Instant>,
    frame_due: Cell<Option<Instant>>,
    pub dirty: bool,
    /// Where it changed since the last redraw, when only part of it did (visualizers).
    pub dirty_region: Option<Rect>,
    pub error_count: usize,
    tooltip: Option<String>,
}

impl View {
    fn new(id: Option<String>) -> Self {
        View {
            id,
            elements: vec![],
            by_id: HashMap::new(),
            bindings: vec![],
            object_handlers: vec![],
            player_handlers: vec![],
            loading: true,
            open: false,
            left: 0.0,
            top: 0.0,
            hover: None,
            pressed: None,
            focus: None,
            anims: vec![],
            timer_due: None,
            scrollers: HashMap::new(),
            frame_due: Cell::new(None),
            dirty: true,
            dirty_region: None,
            error_count: 0,
            tooltip: None,
        }
    }

    pub fn invalidate(&mut self) {
        self.dirty = true;
    }

    /// Only `area` changed: the host may redraw just that.
    pub fn invalidate_region(&mut self, area: Rect) {
        self.dirty_region = Some(match self.dirty_region {
            Some(r) => {
                let (x, y) = (r.x.min(area.x), r.y.min(area.y));
                Rect::new(x, y, r.right().max(area.right()) - x, r.bottom().max(area.bottom()) - y)
            }
            None => area,
        });
    }

    /// An animated GIF needs a redraw `ms` milliseconds after `now`.
    pub fn request_frame(&self, now: Instant, ms: f64) {
        let due = now + Duration::from_secs_f64(ms.max(15.0) / 1000.0);
        if self.frame_due.get().is_none_or(|d| due < d) {
            self.frame_due.set(Some(due));
        }
    }
}

pub fn describe(s: &Skin, v: ViewId, e: usize) -> String {
    let el = s.el(v, e);
    match el.id() {
        Some(id) => format!("<{} id={id}>", el.tag),
        None => format!("<{}>", el.tag),
    }
}

// ---- building ----------------------------------------------------------------------------

const PSEUDO_TAGS: &[(&str, ObjRef)] = &[
    ("PLAYER", ObjRef::Player),
    ("CONTROLS", ObjRef::Controls),
    ("SETTINGS", ObjRef::Settings),
    ("NETWORK", ObjRef::Network),
    ("CURRENTMEDIA", ObjRef::CurrentMedia),
    ("MEDIACENTER", ObjRef::MediaCenter),
    ("CURRENTPLAYLIST", ObjRef::CurrentPlaylist),
    ("DVD", ObjRef::Dvd),
    ("ERROR", ObjRef::Error),
    ("CDROMCOLLECTION", ObjRef::Cdroms),
];

/// What a view needs once its tree exists and its script context is up.
#[derive(Default)]
struct Plan {
    /// (element, handler name, code)
    handlers: Vec<(usize, String, String)>,
    /// (element, attribute, "jscript:..." / "wmpprop:..." value)
    deferred: Vec<(usize, String, String)>,
    /// Element ids as written, for the script globals.
    ids: Vec<String>,
    pseudo: Vec<Pseudo>,
}

/// <PLAYER>, <CONTROLS>, ...: handlers on WMP's objects, scoped to the target and its parents.
struct Pseudo {
    tag: String,
    /// Ends with the target.
    scopes: Vec<ObjRef>,
    attributes: Vec<(String, String)>,
}

fn is_handler_name(name: &str) -> bool {
    let l = name.to_lowercase();
    l.ends_with("_onchange") || (l.len() > 2 && l.starts_with("on") && l.as_bytes()[2].is_ascii_alphabetic())
}

fn is_image_attribute(name: &str) -> bool {
    let l = name.to_lowercase();
    l.ends_with("image") || l == "cursor"
}

fn has_prefix(v: &str, p: &str) -> bool {
    v.len() >= p.len() && v[..p.len()].eq_ignore_ascii_case(p)
}

/// The colour WMP keyed out of a frameless window's background.
const MAGENTA: u32 = 0xFF_00FF;

impl Skin {
    fn apply_attributes(&mut self, v: ViewId, e: usize, node: &WmsNode, plan: &mut Plan) {
        for (name, raw) in &node.attributes {
            if is_handler_name(name) {
                plan.handlers.push((e, name.to_lowercase(), raw.clone()));
                continue;
            }
            if name.eq_ignore_ascii_case("scriptfile") && e != 0 {
                continue;
            }
            let t = raw.trim();
            if ["jscript:", "wmpprop:", "wmpenabled:", "wmpdisabled:"].iter().any(|p| has_prefix(t, p)) {
                plan.deferred.push((e, name.to_lowercase(), t.to_string()));
            } else if resources::is_resource_url(t) && !is_image_attribute(name) && !name.eq_ignore_ascii_case("scriptfile") {
                // A string resource stands for its text.
                let text = resources::read_text(t).unwrap_or_default();
                self.set_value(v, e, name, text.into(), false);
            } else {
                self.set_value(v, e, name, raw.clone().into(), false);
            }
        }
    }

    fn build_node(&mut self, v: ViewId, node: &WmsNode, parent: usize, order: &mut usize, plan: &mut Plan) {
        if let Some(&(_, target)) = PSEUDO_TAGS.iter().find(|(t, _)| *t == node.tag) {
            self.collect_pseudo(node, target, vec![], plan);
            return;
        }
        if matches!(node.tag.as_str(), "VIEW" | "THEME") {
            return;
        }
        let parent_kind = self.el(v, parent).kind;
        let mut kind = element::kind_for(&node.tag, parent_kind);
        if kind == Kind::NonVisual && !node.children.is_empty() {
            crate::log(&format!("[{}] unknown element <{}> line {}; treating as container", self.key, node.tag, node.line));
            kind = Kind::Subview;
        }
        let e = self.view(v).elements.len();
        let mut el = Element::new(&node.tag, kind);
        el.parent = Some(parent);
        el.order = *order;
        *order += 1;
        self.view_mut(v).elements.push(el);
        self.el_mut(v, parent).children.push(e);
        self.apply_presets(v, e, node, plan);
        self.apply_attributes(v, e, node, plan);
        // Every id names the element, so one given two ids answers to both.
        for (k, id) in &node.attributes {
            if k.eq_ignore_ascii_case("id") && !id.is_empty() {
                self.view_mut(v).by_id.insert(id.to_lowercase(), e);
                plan.ids.push(id.clone());
            }
        }
        for c in &node.children {
            self.build_node(v, c, e, order, plan);
        }
        if matches!(kind, Kind::Popup | Kind::ListBox) {
            let lines: Vec<String> =
                self.el(v, e).children.iter().filter(|&&c| self.el(v, c).tag == "ITEM").map(|&c| self.get_str(v, c, "value")).collect();
            self.el_mut(v, e).state.items = lines;
        }
    }

    fn apply_presets(&mut self, v: ViewId, e: usize, node: &WmsNode, plan: &mut Plan) {
        let has = |a: &str| node.attr(a).is_some();
        let tag = node.tag.clone();
        if let Some(element::Preset { action, enabled, tooltip, sticky, down }) = element::preset(&tag) {
            if !has("onclick") {
                plan.handlers.push((e, "onclick".into(), action.into()));
            }
            if let Some(en) = enabled.filter(|_| !has("enabled")) {
                plan.deferred.push((e, "enabled".into(), en.into()));
            }
            if !has("uptooltip") && !has("tooltip") {
                self.set_value(v, e, "uptooltip", tooltip.into(), false);
            }
            if sticky && !has("sticky") {
                self.set_value(v, e, "sticky", true.into(), false);
            }
            if let Some(d) = down.filter(|_| !has("down")) {
                plan.deferred.push((e, "down".into(), d.into()));
            }
        }
        let mut defer = |attr: &str, spec: &str| {
            if !has(attr) {
                plan.deferred.push((e, attr.into(), spec.into()));
            }
        };
        let mut set = Vec::new();
        let mut handler = Vec::new();
        match tag.as_str() {
            "SEEKSLIDER" => {
                set.push(("min", Value::Num(0.0)));
                defer("max", "wmpprop:player.currentMedia.duration");
                defer("value", "wmpprop:player.controls.currentPosition");
                handler.push(("ondragend", "player.controls.currentPosition = value;"));
                defer("enabled", "wmpenabled:player.controls.currentPosition");
                set.push(("tooltip", "Seek".into()));
            }
            "VOLUMESLIDER" => {
                set.push(("min", Value::Num(0.0)));
                set.push(("max", Value::Num(100.0)));
                defer("value", "wmpprop:player.settings.volume");
                handler.push(("value_onchange", "player.settings.volume = value;"));
                set.push(("tooltip", "Volume".into()));
            }
            "BALANCESLIDER" => {
                set.push(("min", Value::Num(-100.0)));
                set.push(("max", Value::Num(100.0)));
                defer("value", "wmpprop:player.settings.balance");
                handler.push(("value_onchange", "player.settings.balance = value;"));
                set.push(("tooltip", "Balance".into()));
            }
            "STATUSTEXT" => defer("value", "wmpprop:player.status"),
            "CURRENTPOSITIONTEXT" => defer("value", "wmpprop:player.controls.currentPositionString"),
            "DURATIONTEXT" => defer("value", "wmpprop:player.currentMedia.durationString"),
            "METADATATEXT" => defer("value", "wmpprop:player.currentMedia.name"),
            _ => {}
        }
        for (attr, value) in set {
            if !has(attr) {
                self.set_value(v, e, attr, value, false);
            }
        }
        for (name, code) in handler {
            if !has(name) {
                plan.handlers.push((e, name.into(), code.into()));
            }
        }
    }

    /// <PLAYER playStateChange="..." status_onchange="..."> and its nested CONTROLS/NETWORK/...
    fn collect_pseudo(&self, node: &WmsNode, target: ObjRef, outer: Vec<ObjRef>, plan: &mut Plan) {
        let mut scopes = outer;
        scopes.push(target);
        plan.pseudo.push(Pseudo { tag: node.tag.clone(), scopes: scopes.clone(), attributes: node.attributes.clone() });
        for c in &node.children {
            if let Some(&(_, t)) = PSEUDO_TAGS.iter().find(|(tag, _)| *tag == c.tag) {
                self.collect_pseudo(c, t, scopes.clone(), plan);
            }
        }
    }

    /// The object a binding path starts from.
    fn resolve_root(&self, v: ViewId, name: &str, script: &ScriptHost) -> Option<ObjRef> {
        let lower = name.to_lowercase();
        if let Some(&e) = self.view(v).by_id.get(&lower) {
            return Some(ObjRef::Element { view: v, element: e });
        }
        match lower.as_str() {
            "player" => return Some(ObjRef::Player),
            "mediacenter" => return Some(ObjRef::MediaCenter),
            "theme" => return Some(ObjRef::Theme),
            "view" => return Some(ObjRef::Element { view: v, element: 0 }),
            "event" => return Some(ObjRef::Event),
            _ => {}
        }
        if self.view(v).id.as_deref().is_some_and(|i| i.eq_ignore_ascii_case(name)) {
            return Some(ObjRef::Element { view: v, element: 0 });
        }
        match script.get_global(name) {
            Value::Obj(o) => Some(o),
            _ => None,
        }
    }

    fn apply_binding(&mut self, v: ViewId, i: usize) {
        let b = &self.view(v).bindings[i];
        let (source, target, attr, prop, kind) = (b.source, b.target, b.attr.clone(), b.prop.clone(), b.kind);
        let notify = !self.view(v).loading;
        let value = match kind {
            BindKind::Prop => {
                let el = self.el(v, target);
                if attr == "value" && el.state.dragging && el.kind == Kind::Slider {
                    return;
                }
                self.get(source, &prop)
            }
            BindKind::Enabled | BindKind::Disabled => {
                // wmpenabled:player.controls.play → controls.isAvailable("play")
                let on = if self.has_method(source, "isavailable") {
                    self.call(source, "isavailable", &[prop.clone().into()]).to_bool()
                } else {
                    self.get(source, &prop).to_bool()
                };
                Value::Bool(if kind == BindKind::Disabled { !on } else { on })
            }
        };
        self.set_value(v, target, &attr, value, notify);
    }

    pub(super) fn apply_bindings(&mut self, v: ViewId, source: ObjRef, prop: &str) {
        let hits: Vec<usize> =
            self.view(v).bindings.iter().enumerate().filter(|(_, b)| b.source == source && b.watch == prop).map(|(i, _)| i).collect();
        for i in hits {
            if self.views.get(v).is_some_and(Option::is_some) {
                self.apply_binding(v, i);
            }
        }
    }

    // ---- changes -----------------------------------------------------------------------

    pub(super) fn on_value_changed(&mut self, v: ViewId, e: usize, name: &str) {
        if matches!(name, "width" | "height") {
            self.apply_resize_to_children(v, e);
        }
        if name == "visible" && self.visible(v, e) {
            self.restart_animations(v, e);
        }
        match self.el(v, e).kind {
            Kind::Text => {
                if name == "value" {
                    self.el_mut(v, e).state.scroll = 0.0;
                }
                if matches!(name, "scrolling" | "value" | "visible" | "width" | "scrollingdelay") {
                    self.update_scrolling(v, e);
                }
            }
            Kind::EqualizerSettings => self.equalizer_changed(v, e, name),
            Kind::Effects | Kind::Video if matches!(name, "currenteffecttype" | "effecttype" | "currentpreset" | "preset") => {
                self.effects_changed(v, e, name)
            }
            _ => {}
        }
        self.view_mut(v).invalidate();
    }

    fn equalizer_changed(&mut self, v: ViewId, e: usize, name: &str) {
        if self.el(v, e).state.applying {
            return;
        }
        let custom = (element::EQ_PRESETS.len() - 1) as f64;
        if name == "currentpreset" {
            let p = (self.get_int(v, e, "currentpreset").max(0) as usize).min(element::EQ_PRESETS.len() - 1);
            if element::EQ_PRESETS[p].0 == "Custom" {
                return;
            }
            self.el_mut(v, e).state.applying = true;
            for (i, g) in element::EQ_PRESETS[p].1.iter().enumerate() {
                self.set_value(v, e, &format!("gainlevel{}", i + 1), (*g).into(), true);
            }
            self.el_mut(v, e).state.applying = false;
            self.notify(ObjRef::Element { view: v, element: e }, "currentpresettitle");
        } else if name.starts_with("gainlevel") && self.get_num(v, e, "currentpreset") != custom {
            self.el_mut(v, e).state.applying = true;
            self.set_value(v, e, "currentpreset", custom.into(), true);
            self.el_mut(v, e).state.applying = false;
            self.notify(ObjRef::Element { view: v, element: e }, "currentpresettitle");
        } else if name == "enable" {
            let on = self.get_bool(v, e, "enable");
            self.set_value(v, e, "enabled", on.into(), true);
        }
    }

    pub(super) fn on_element_changed(&mut self, v: ViewId, e: usize, prop: &str) {
        if self.view(v).loading {
            return;
        }
        self.fire(v, e, &format!("{prop}_onchange"), vec![]);
        if e == 0 && matches!(prop, "width" | "height" | "title") {
            self.host_call(move |h| h.view_changed(v));
        }
        if e == 0 && prop == "timerinterval" {
            self.restart_timer(v, self.now);
        }
        if matches!(prop, "visible" | "width") {
            self.update_all_scrolling(v);
        }
    }

    fn restart_animations(&mut self, v: ViewId, e: usize) {
        self.el(v, e).restart_animations();
        for c in self.el(v, e).children.clone() {
            self.restart_animations(v, c);
        }
    }

    fn capture_layout_baseline(&mut self, v: ViewId, e: usize) {
        let size = (self.width(v, e), self.height(v, e));
        self.el_mut(v, e).baseline = Some(size);
        for c in self.el(v, e).children.clone() {
            self.capture_layout_baseline(v, c);
        }
    }

    /// WMP alignment: left/top stay put, right/bottom follow the far edge, center keeps centred,
    /// stretch grows.
    fn apply_resize_to_children(&mut self, v: ViewId, e: usize) {
        let Some((lw, lh)) = self.el(v, e).baseline else { return };
        let (w, h) = (self.width(v, e), self.height(v, e));
        let (dw, dh) = (w - lw, h - lh);
        self.el_mut(v, e).baseline = Some((w, h));
        if dw == 0 && dh == 0 {
            return;
        }
        for c in self.el(v, e).children.clone() {
            if dw != 0 {
                match self.get_str(v, c, "horizontalalignment").to_lowercase().as_str() {
                    "right" => self.set_value(v, c, "left", f64::from(self.get_int(v, c, "left") + dw).into(), true),
                    "center" => self.set_value(v, c, "left", (f64::from(self.get_int(v, c, "left")) + f64::from(dw) / 2.0).into(), true),
                    "stretch" => self.set_value(v, c, "width", f64::from((self.width(v, c) + dw).max(0)).into(), true),
                    _ => {}
                }
            }
            if dh != 0 {
                match self.get_str(v, c, "verticalalignment").to_lowercase().as_str() {
                    "bottom" => self.set_value(v, c, "top", f64::from(self.get_int(v, c, "top") + dh).into(), true),
                    "center" => self.set_value(v, c, "top", (f64::from(self.get_int(v, c, "top")) + f64::from(dh) / 2.0).into(), true),
                    "stretch" => self.set_value(v, c, "height", f64::from((self.height(v, c) + dh).max(0)).into(), true),
                    _ => {}
                }
            }
        }
    }

    // ---- timers and animation ----------------------------------------------------------

    pub(super) fn restart_timer(&mut self, v: ViewId, now: Instant) {
        let ms = self.get_int(v, 0, "timerinterval");
        let due = (ms > 0 && self.has_handler(v, 0, "ontimer") && self.view(v).open).then(|| now + Duration::from_millis(ms.max(15) as u64));
        self.view_mut(v).timer_due = due;
    }

    pub(super) fn update_all_scrolling(&mut self, v: ViewId) {
        let texts: Vec<usize> = (0..self.view(v).elements.len()).filter(|&e| self.el(v, e).kind == Kind::Text).collect();
        for t in texts {
            self.update_scrolling(v, t);
        }
    }

    fn update_scrolling(&mut self, v: ViewId, e: usize) {
        if !self.view(v).open {
            return;
        }
        let want = self.text_scrolling(v, e);
        let has = self.view(v).scrollers.contains_key(&e);
        if want && !has {
            let due = self.now + Duration::from_millis(self.get_int(v, e, "scrollingdelay").max(15) as u64);
            self.view_mut(v).scrollers.insert(e, due);
        } else if !want && has {
            self.view_mut(v).scrollers.remove(&e);
        }
    }

    fn animate(&mut self, v: ViewId, e: usize, kind: AnimKind, ms: f64) {
        let start = self.now;
        let view = self.view_mut(v);
        view.anims.retain(|a| !(a.element == e && std::mem::discriminant(&a.kind) == std::mem::discriminant(&kind)));
        view.anims.push(Anim { element: e, kind, ms, start });
    }

    pub fn move_to(&mut self, v: ViewId, e: usize, x: f64, y: f64, ms: f64) {
        if ms <= 0.0 {
            self.set_value(v, e, "left", x.into(), true);
            self.set_value(v, e, "top", y.into(), true);
            self.fire(v, e, "onendmove", vec![]);
            return;
        }
        let (x0, y0) = (f64::from(self.get_int(v, e, "left")), f64::from(self.get_int(v, e, "top")));
        self.animate(v, e, AnimKind::Move { x0, y0, x, y }, ms);
    }

    pub fn alpha_blend_to(&mut self, v: ViewId, e: usize, a: f64, ms: f64) {
        if ms <= 0.0 {
            self.set_value(v, e, "alphablend", a.into(), true);
            self.fire(v, e, "onendalphablend", vec![]);
            return;
        }
        let a0 = self.get_num(v, e, "alphablend");
        self.animate(v, e, AnimKind::Alpha { a0, a }, ms);
    }

    pub fn resize_to(&mut self, v: ViewId, e: usize, w: f64, h: f64, ms: f64) {
        if ms <= 0.0 {
            self.set_value(v, e, "width", w.into(), true);
            self.set_value(v, e, "height", h.into(), true);
            self.fire(v, e, "onendresize", vec![]);
            return;
        }
        let (w0, h0) = (f64::from(self.width(v, e)), f64::from(self.height(v, e)));
        self.animate(v, e, AnimKind::Resize { w0, h0, w, h }, ms);
    }

    pub(super) fn tick_view(&mut self, v: ViewId, now: Instant) {
        if let Some(due) = self.view(v).timer_due.filter(|d| *d <= now) {
            let ms = self.get_int(v, 0, "timerinterval").max(15) as u64;
            self.view_mut(v).timer_due = Some(due.max(now - Duration::from_millis(ms)) + Duration::from_millis(ms));
            self.fire(v, 0, "ontimer", vec![]);
        }
        let anims = std::mem::take(&mut self.view_mut(v).anims);
        let mut keep = vec![];
        for a in anims {
            if self.views.get(v).is_none_or(Option::is_none) {
                return;
            }
            let t = (now.duration_since(a.start).as_secs_f64() * 1000.0 / a.ms).clamp(0.0, 1.0);
            let e = a.element;
            match a.kind {
                AnimKind::Move { x0, y0, x, y } => {
                    self.set_value(v, e, "left", (x0 + (x - x0) * t).into(), true);
                    self.set_value(v, e, "top", (y0 + (y - y0) * t).into(), true);
                }
                AnimKind::Alpha { a0, a: to } => self.set_value(v, e, "alphablend", (a0 + (to - a0) * t).into(), true),
                AnimKind::Resize { w0, h0, w, h } => {
                    self.set_value(v, e, "width", (w0 + (w - w0) * t).into(), true);
                    self.set_value(v, e, "height", (h0 + (h - h0) * t).into(), true);
                }
            }
            if t >= 1.0 {
                let done = match a.kind {
                    AnimKind::Move { .. } => "onendmove",
                    AnimKind::Alpha { .. } => "onendalphablend",
                    AnimKind::Resize { .. } => "onendresize",
                };
                self.fire(v, e, done, vec![]);
            } else {
                keep.push(a);
            }
        }
        if self.views.get(v).is_none_or(Option::is_none) {
            return;
        }
        // Animations started by handlers during this step were added meanwhile.
        let view = self.view_mut(v);
        keep.append(&mut view.anims);
        view.anims = keep;

        let due: Vec<usize> = self.view(v).scrollers.iter().filter(|(_, d)| **d <= now).map(|(e, _)| *e).collect();
        for e in due {
            let width = f64::from(self.text_measure(v, e).0) + 24.0;
            let amount = f64::from(self.get_int(v, e, "scrollingamount").max(1));
            let delay = self.get_int(v, e, "scrollingdelay").max(15) as u64;
            let el = self.el_mut(v, e);
            el.state.scroll = (el.state.scroll + amount) % width.max(1.0);
            let view = self.view_mut(v);
            view.scrollers.insert(e, now + Duration::from_millis(delay));
            view.invalidate();
        }
        if self.view(v).frame_due.get().is_some_and(|d| d <= now) {
            let view = self.view_mut(v);
            view.frame_due.set(None);
            view.invalidate();
        }
    }

    pub(super) fn focus(&mut self, v: ViewId, e: Option<usize>) {
        let old = self.view(v).focus;
        if old == e {
            return;
        }
        if let Some(o) = old {
            self.fire(v, o, "onblur", vec![]);
        }
        self.view_mut(v).focus = e;
        if let Some(n) = e {
            self.fire(v, n, "onfocus", vec![]);
        }
    }

    fn placement_key(&self, v: ViewId) -> String {
        format!("{}|{}", self.key, self.view(v).id.clone().unwrap_or_else(|| "main".into())).to_lowercase()
    }

    pub(super) fn place_view(&mut self, v: ViewId, at: Option<(f64, f64)>) {
        let area = self.work_area;
        let (w, h) = (f64::from(self.width(v, 0)), f64::from(self.height(v, 0)));
        let opener = self.active_view.filter(|&a| a != v && self.views.get(a).is_some_and(Option::is_some));
        let (left, top) = match (at, opener) {
            (Some((x, y)), Some(o)) => (self.view(o).left + x, self.view(o).top + y),
            _ => match self.settings.skin_windows.get(&self.placement_key(v)) {
                Some(&(l, t)) => (l.clamp(area.x - w + 40.0, area.right() - 40.0), t.clamp(area.y, area.bottom() - 40.0)),
                None => (area.x + ((area.w - w) / 2.0).max(0.0), area.y + ((area.h - h) / 2.0).max(0.0)),
            },
        };
        let view = self.view_mut(v);
        view.left = left;
        view.top = top;
    }

    /// The window was moved (by the user or the system).
    pub fn window_moved(&mut self, v: ViewId, left: f64, top: f64) {
        if self.views.get(v).is_none_or(Option::is_none) {
            return;
        }
        let view = self.view_mut(v);
        view.left = left;
        view.top = top;
        let key = self.placement_key(v);
        self.settings.skin_windows.insert(key, (left, top));
    }

    /// The user resized the window (view.size()): the view follows, within its limits.
    pub fn window_resized(&mut self, v: ViewId, w: f64, h: f64) {
        let (min_w, min_h) = (self.get_int(v, 0, "minwidth").max(1), self.get_int(v, 0, "minheight").max(1));
        let (max_w, max_h) = (self.get_int(v, 0, "maxwidth"), self.get_int(v, 0, "maxheight"));
        let clamp = |x: i32, min: i32, max: i32| if max > 0 { x.min(max).max(min) } else { x.max(min) };
        self.set_value(v, 0, "width", f64::from(clamp(w.round() as i32, min_w, max_w)).into(), true);
        self.set_value(v, 0, "height", f64::from(clamp(h.round() as i32, min_h, max_h)).into(), true);
    }

    // ---- drawing -----------------------------------------------------------------------

    /// A VIEW's clippingColor / transparencyColor key the composed window (like a layered window
    /// colour key): any pixel of that colour disappears, whichever element drew it.
    fn view_keys(&self, v: ViewId) -> Vec<u32> {
        let clip = self.get_str(v, 0, "clippingimage").is_empty().then(|| parse_color(&self.get_str(v, 0, "clippingcolor"))).flatten();
        let trans = parse_color(&self.get_str(v, 0, "transparencycolor"));
        if self.get_bool(v, 0, "titlebar") || self.width(v, 0) <= 0 || self.height(v, 0) <= 0 {
            return vec![];
        }
        clip.into_iter().chain(trans).collect()
    }

    /// Draws the whole view at `scale` device pixels per skin pixel.
    pub fn render_view(&self, v: ViewId, scale: f32) -> Option<Canvas> {
        let (w, h) = (self.width(v, 0).clamp(1, 4000) as u32, self.height(v, 0).clamp(1, 4000) as u32);
        let mut canvas = Canvas::new(w, h, scale)?;
        self.draw_window(v, &mut canvas, Rect::new(0.0, 0.0, f64::from(w), f64::from(h)));
        Some(canvas)
    }

    /// Draws just `area` of the view (widened to whole skin pixels), for pasting over the last
    /// full drawing. Gives the same pixels there as [`Skin::render_view`].
    pub fn render_region(&self, v: ViewId, scale: f32, area: Rect) -> Option<Canvas> {
        let (w, h) = (f64::from(self.width(v, 0).clamp(1, 4000)), f64::from(self.height(v, 0).clamp(1, 4000)));
        let (x0, y0) = (area.x.floor().max(0.0), area.y.floor().max(0.0));
        let (x1, y1) = (area.right().ceil().min(w), area.bottom().ceil().min(h));
        let area = Rect::new(x0, y0, x1 - x0, y1 - y0);
        if area.is_empty() {
            return None;
        }
        let mut canvas = Canvas::region(area, scale)?;
        self.draw_window(v, &mut canvas, area);
        Some(canvas)
    }

    /// Draws `area` of the view and cuts what WMP's window left see-through. A window without a
    /// title bar also loses magenta wherever it shows: WMP filled the window's empty background
    /// with it and keyed it out, so magenta drawn over a hole vanished too (WWC's intro over its
    /// pipes; World of Warcraft's sliders, as its preview screenshot shows).
    fn draw_window(&self, v: ViewId, canvas: &mut Canvas, area: Rect) {
        let keys = self.view_keys(v);
        self.render_tree(v, 0, canvas, area, keys.is_empty());
        canvas.cut_holes();
        let frameless = !self.get_bool(v, 0, "titlebar");
        canvas.key_out(&keys.into_iter().chain(frameless.then_some(MAGENTA)).collect::<Vec<_>>());
    }

    /// A VIEW with children behind it: its colour, them, then its image. They show through the
    /// image's holes and its clippingColor (Yahoo's screen); the window is cut only where that
    /// colour has nothing behind it.
    fn render_view_over_behind(&self, v: ViewId, c: &mut Canvas, b: Rect, clip: Rect, behind: &[usize]) {
        self.render_subview_fill(v, 0, c, b, clip);
        let Some(mut layer) = c.layer() else { return };
        for &child in behind {
            self.render_tree(v, child, &mut layer, clip, true);
        }
        c.composite(&layer, 1.0, None);
        self.render_subview_image(v, 0, c, b, clip, true);
        let (Some(cut), Some(raw)) = (self.view_cut_color(v), self.images.get_raw(&self.get_str(v, 0, "backgroundimage"))) else { return };
        let Some(mask) = c.mask(b.x, b.y, |x, y| if raw.rgb(x, y).is_some_and(|rgb| matches_key(rgb, cut)) { 255 } else { 0 }) else { return };
        for ((px, &m), under) in c.pixmap.pixels_mut().iter_mut().zip(mask.data()).zip(layer.pixmap.pixels()) {
            if m > 0 && under.alpha() == 0 {
                *px = tiny_skia::PremultipliedColorU8::TRANSPARENT;
            }
        }
    }

    fn render_tree(&self, v: ViewId, e: usize, canvas: &mut Canvas, clip: Rect, view_mask: bool) {
        // A VIEW is the window itself; WMP ignored visible="false" on it.
        if e != 0 && !self.visible(v, e) {
            return;
        }
        let b = self.bounds(v, e);
        // Nothing of it (or of what it clips) is inside the area being drawn.
        if e != 0 && b.intersect(&clip).is_empty() && (self.clips_children(v, e) || self.el(v, e).children.is_empty()) {
            return;
        }
        let opacity = self.opacity(v, e);
        let mask = if e == 0 && !view_mask { None } else { self.clip_mask(v, e) };
        let host = self.host_mask(v, e);
        let own_clip = if self.clips_children(v, e) { clip.intersect(&b) } else { clip };
        let draw = |target: &mut Canvas| {
            let (behind, above) = self.children_behind_and_above(v, e);
            if behind.is_empty() {
                self.render_element(v, e, target, b, own_clip);
            } else if e == 0 {
                self.render_view_over_behind(v, target, b, own_clip, &behind);
            } else {
                // Its colour, then the children behind, then its image.
                self.render_subview_fill(v, e, target, b, own_clip);
                for c in behind {
                    self.render_tree(v, c, target, own_clip, true);
                }
                self.render_subview_image(v, e, target, b, own_clip, false);
            }
            for c in above {
                self.render_tree(v, c, target, own_clip, true);
            }
        };
        if opacity < 0.999 || mask.is_some() || host.is_some() {
            let Some(mut layer) = canvas.layer() else { return };
            draw(&mut layer);
            let tiled = self.mask_tiled(v, e);
            let own_alpha = |x: i32, y: i32| {
                let Some(m) = &mask else { return 255 };
                let (mw, mh) = (m.width as i32, m.height as i32);
                if tiled && mw > 0 && mh > 0 { m.alpha(x.rem_euclid(mw), y.rem_euclid(mh)) } else { m.alpha(x, y) }
            };
            let device_mask = match &host {
                Some((hm, at)) => {
                    let (dx, dy) = ((b.x - at.x) as i32, (b.y - at.y) as i32);
                    canvas.mask(b.x, b.y, |x, y| own_alpha(x, y).min(hm.alpha(x + dx, y + dy)))
                }
                None if mask.is_some() => canvas.mask(b.x, b.y, own_alpha),
                None => None,
            };
            canvas.composite(&layer, opacity, device_mask.as_ref());
        } else {
            draw(canvas);
        }
    }

    // ---- input -------------------------------------------------------------------------

    pub fn hit_test_point(&self, v: ViewId, x: f64, y: f64) -> Option<usize> {
        let (behind, above) = self.children_behind_and_above(v, 0);
        let own = self.hit_test(v, 0, x, y);
        self.hit_in(v, &above, x, y)
            .or_else(|| (own && (behind.is_empty() || self.background_covers(v, 0, x, y))).then_some(0))
            .or_else(|| self.hit_in(v, &behind, x, y))
            .or_else(|| own.then_some(0))
    }

    /// The topmost of `children` (in draw order) under the point, searching into containers in
    /// the order they draw: children above, the container itself, then children behind it.
    fn hit_in(&self, v: ViewId, children: &[usize], x: f64, y: f64) -> Option<usize> {
        for &c in children.iter().rev() {
            if !self.visible(v, c) {
                continue;
            }
            let (ax, ay) = self.absolute(v, c);
            let (lx, ly) = (x - ax, y - ay);
            if self.el(v, c).kind == Kind::ButtonGroup {
                if !self.passes_clicks(v, c) && self.hit_test(v, c, lx, ly) && self.mask_allows(v, c, lx, ly) {
                    return self.group_element_at(v, c, lx, ly);
                }
                continue;
            }
            if self.clips_children(v, c)
                && (lx < 0.0
                    || ly < 0.0
                    || lx >= f64::from(self.width(v, c))
                    || ly >= f64::from(self.height(v, c))
                    || !self.mask_allows(v, c, lx, ly))
            {
                continue;
            }
            let (behind, above) = self.children_behind_and_above(v, c);
            if let Some(inner) = self.hit_in(v, &above, x, y) {
                return Some(inner);
            }
            // A disabled button lets clicks through: QuickSilver covers its buttons with disabled
            // ones as artwork.
            let disabled_button = self.el(v, c).kind == Kind::Button && !self.enabled(v, c);
            let own = !self.passes_clicks(v, c) && !disabled_button && self.hit_test(v, c, lx, ly) && self.mask_allows(v, c, lx, ly);
            // Its image covers the children behind it; its colour lies under them.
            if own && (behind.is_empty() || self.background_covers(v, c, lx, ly)) {
                return Some(c);
            }
            if let Some(inner) = self.hit_in(v, &behind, x, y) {
                return Some(inner);
            }
            if own {
                return Some(c);
            }
        }
        None
    }

    fn local(&self, v: ViewId, e: usize, x: f64, y: f64) -> (f64, f64) {
        if e == 0 {
            return (x, y);
        }
        let (ax, ay) = self.absolute(v, e);
        (x - ax, y - ay)
    }

    fn set_event_mouse(&mut self, v: ViewId, e: usize, x: f64, y: f64, button: i32, mods: (bool, bool, bool)) {
        let (lx, ly) = self.local(v, e, x, y);
        let (left, top) = (self.view(v).left, self.view(v).top);
        let ev = &mut self.event;
        for (k, val) in [("offsetx", lx), ("offsety", ly), ("x", x), ("y", y), ("screenx", left + x), ("screeny", top + y)] {
            ev.insert(k.into(), val.into());
        }
        ev.insert("button".into(), f64::from(button).into());
        objects::set_modifiers(ev, mods);
    }

    fn ancestry(&self, v: ViewId, e: Option<usize>) -> Vec<usize> {
        let mut chain = vec![];
        let mut cur = e;
        while let Some(i) = cur {
            chain.push(i);
            cur = self.el(v, i).parent;
        }
        chain
    }

    fn mouse_enter(&mut self, v: ViewId, e: usize) {
        self.el_mut(v, e).hovered = true;
        self.fire(v, e, "onmouseover", vec![]);
        if self.el(v, e).kind == Kind::GroupButton
            && let Some(g) = self.el(v, e).parent
        {
            self.notify(ObjRef::Element { view: v, element: g }, "hover");
        }
        self.view_mut(v).invalidate();
    }

    fn mouse_leave_el(&mut self, v: ViewId, e: usize) {
        self.el_mut(v, e).hovered = false;
        self.fire(v, e, "onmouseout", vec![]);
        self.view_mut(v).invalidate();
    }

    /// onmouseout fires on the elements the pointer really left and onmouseover on the ones it
    /// entered, so a container isn't left when the pointer moves onto one of its children.
    fn move_hover(&mut self, v: ViewId, hit: Option<usize>, x: f64, y: f64, mods: (bool, bool, bool)) {
        let from = self.ancestry(v, self.view(v).hover);
        let to = self.ancestry(v, hit);
        self.view_mut(v).hover = hit;
        for &e in &from {
            if !to.contains(&e) {
                self.set_event_mouse(v, e, x, y, 0, mods);
                self.mouse_leave_el(v, e);
            }
        }
        for &e in to.iter().rev() {
            if !from.contains(&e) {
                self.set_event_mouse(v, e, x, y, 0, mods);
                self.mouse_enter(v, e);
            }
        }
        let tip = hit.and_then(|h| self.tooltip(v, h));
        if tip != self.view(v).tooltip {
            self.view_mut(v).tooltip = tip.clone();
            self.host_call(move |h| h.set_tooltip(v, tip.as_deref()));
        }
        let cursor = hit.map(|h| self.get_str(v, h, "cursor")).unwrap_or_default();
        self.host_call(move |h| h.set_cursor(v, &cursor));
    }

    pub fn mouse_move(&mut self, v: ViewId, x: f64, y: f64, mods: (bool, bool, bool)) {
        if let Some(p) = self.view(v).pressed {
            self.set_event_mouse(v, p, x, y, 1, mods);
            let inside = self.hit_test_point(v, x, y) == Some(p);
            if self.el(v, p).hovered != inside {
                self.el_mut(v, p).hovered = inside;
                self.view_mut(v).invalidate();
            }
            let (lx, ly) = self.local(v, p, x, y);
            self.element_mouse_move(v, p, lx, ly, true);
            return;
        }
        let hit = self.hit_test_point(v, x, y);
        if hit != self.view(v).hover {
            self.move_hover(v, hit, x, y, mods);
        } else if let Some(h) = hit {
            self.set_event_mouse(v, h, x, y, 0, mods);
            let (lx, ly) = self.local(v, h, x, y);
            self.element_mouse_move(v, h, lx, ly, false);
        }
    }

    pub fn mouse_leave(&mut self, v: ViewId) {
        if self.view(v).pressed.is_some() {
            return;
        }
        for e in self.ancestry(v, self.view(v).hover) {
            self.mouse_leave_el(v, e);
        }
        self.view_mut(v).hover = None;
        self.view_mut(v).tooltip = None;
        self.host_call(move |h| h.set_tooltip(v, None));
    }

    /// What the window should do with a press the skin didn't take.
    pub fn mouse_down(&mut self, v: ViewId, x: f64, y: f64, button: MouseButton, clicks: u32, mods: (bool, bool, bool)) -> MouseResult {
        let Some(hit) = self.hit_test_point(v, x, y) else { return MouseResult::Ignored };
        let code = match button {
            MouseButton::Left => 1,
            MouseButton::Right => 2,
            MouseButton::Middle => 4,
        };
        self.set_event_mouse(v, hit, x, y, code, mods);
        if clicks == 2 && self.has_handler(v, hit, "ondblclick") {
            self.fire(v, hit, "ondblclick", vec![]);
            return MouseResult::Taken;
        }
        if !self.interactive(v, hit) {
            if hit != 0 {
                self.fire(v, hit, "onmousedown", vec![]);
                if self.has_handler(v, 0, "onmousedown") {
                    self.fire(v, 0, "onmousedown", vec![]);
                }
            }
            return if button == MouseButton::Right { MouseResult::ContextMenu } else { MouseResult::Drag };
        }
        if button != MouseButton::Left {
            self.fire(v, hit, "onmousedown", vec![]);
            return if button == MouseButton::Right { MouseResult::ContextMenu } else { MouseResult::Taken };
        }
        self.view_mut(v).pressed = Some(hit);
        self.view_mut(v).hover = Some(hit);
        self.el_mut(v, hit).hovered = true;
        self.focus(v, Some(hit));
        let (lx, ly) = self.local(v, hit, x, y);
        self.element_mouse_down(v, hit, lx, ly);
        MouseResult::Taken
    }

    pub fn mouse_up(&mut self, v: ViewId, x: f64, y: f64, button: MouseButton, mods: (bool, bool, bool)) {
        let Some(p) = self.view_mut(v).pressed.take() else { return };
        self.set_event_mouse(v, p, x, y, 1, mods);
        let inside = self.hit_test_point(v, x, y) == Some(p);
        let (lx, ly) = self.local(v, p, x, y);
        self.element_mouse_up(v, p, lx, ly, button, inside);
        if self.views.get(v).is_none_or(Option::is_none) || self.view(v).elements.len() <= p {
            return;
        }
        self.el_mut(v, p).hovered = inside;
        self.mouse_move(v, x, y, mods);
    }

    pub fn mouse_wheel(&mut self, v: ViewId, x: f64, y: f64, delta: f64) {
        let hit = self.hit_test_point(v, x, y).or(self.view(v).focus);
        match hit {
            Some(h) if matches!(self.el(v, h).kind, Kind::Slider | Kind::CustomSlider | Kind::Playlist | Kind::ListBox) => {
                self.element_wheel(v, h, delta)
            }
            Some(h) if self.interactive(v, h) => self.element_wheel(v, h, delta),
            _ => {
                let vol = self.player.volume() + if delta > 0.0 { 5 } else { -5 };
                self.player.set_volume(vol);
                let changes = self.player.take_changes();
                self.apply_player_changes(changes);
            }
        }
    }

    /// A key press with a Windows virtual-key code; returns true when the skin handled it.
    pub fn key_down(&mut self, v: ViewId, vk: u32, mods: (bool, bool, bool)) -> bool {
        self.event.insert("keycode".into(), f64::from(vk).into());
        objects::set_modifiers(&mut self.event, mods);
        if let Some(f) = self.view(v).focus {
            self.fire(v, f, "onkeydown", vec![]);
            if self.element_key_down(v, f, vk) {
                return true;
            }
        }
        if self.has_handler(v, 0, "onkeydown") {
            // The handler runs after this returns; WMP's default keys still apply unless it handles them.
            self.fire(v, 0, "onkeydown", vec![]);
        }
        let ctrl = mods.1;
        let pm = &mut self.player;
        match (ctrl, vk) {
            (true, 0x50) => pm.toggle_play_pause(), // Ctrl+P
            (true, 0x53) => pm.stop(),              // Ctrl+S
            (true, 0x42) => pm.previous(),          // Ctrl+B
            (true, 0x46) => pm.next(),              // Ctrl+F
            (_, 0x77) => pm.set_mute(!pm.mute()),   // F8
            (_, 0x78) => pm.set_volume(pm.volume() - 5),
            (_, 0x79) => pm.set_volume(pm.volume() + 5),
            _ => return false,
        }
        let changes = self.player.take_changes();
        self.apply_player_changes(changes);
        true
    }

    pub fn key_up(&mut self, v: ViewId, vk: u32, mods: (bool, bool, bool)) {
        self.event.insert("keycode".into(), f64::from(vk).into());
        objects::set_modifiers(&mut self.event, mods);
        if let Some(f) = self.view(v).focus {
            self.fire(v, f, "onkeyup", vec![]);
        }
        self.fire(v, 0, "onkeyup", vec![]);
    }

    // ---- per-kind pointer behaviour ----------------------------------------------------

    fn element_mouse_down(&mut self, v: ViewId, e: usize, x: f64, y: f64) {
        self.el_mut(v, e).pressed = true;
        self.fire(v, e, "onmousedown", vec![]);
        self.view_mut(v).invalidate();
        let enabled = self.enabled(v, e);
        match self.el(v, e).kind {
            Kind::Slider if enabled => {
                let vertical = self.slider_vertical(v, e);
                let pos = if vertical { y } else { x };
                match self.slider_thumb_rect(v, e).filter(|t| t.contains(x, y)) {
                    Some(t) => self.el_mut(v, e).state.drag_offset = pos - if vertical { t.y + t.h / 2.0 } else { t.x + t.w / 2.0 },
                    None => {
                        self.el_mut(v, e).state.drag_offset = 0.0;
                        let target = self.slider_value_at(v, e, pos);
                        if self.get_bool(v, e, "slide") {
                            self.set_value(v, e, "value", target.into(), true);
                        } else {
                            // Page toward the click.
                            let (cur, step) = (self.get_num(v, e, "value"), (self.get_num(v, e, "max") - self.get_num(v, e, "min")) / 10.0);
                            let next = if target > cur { target.min(cur + step) } else { target.max(cur - step) };
                            self.set_value(v, e, "value", next.into(), true);
                        }
                    }
                }
                self.el_mut(v, e).state.dragging = true;
                self.fire(v, e, "ondragbegin", vec![]);
            }
            Kind::CustomSlider if enabled => {
                self.el_mut(v, e).state.dragging = true;
                self.fire(v, e, "ondragbegin", vec![]);
                if let Some(val) = self.custom_slider_value(v, e, x, y) {
                    self.set_value(v, e, "value", val.into(), true);
                }
            }
            Kind::Playlist => self.focus(v, Some(e)),
            Kind::ListBox => {
                if let Some(i) = self.listbox_item_at(v, e, y) {
                    self.set_value(v, e, "selecteditem", i.into(), true);
                }
            }
            _ => {}
        }
    }

    fn element_mouse_move(&mut self, v: ViewId, e: usize, x: f64, y: f64, captured: bool) {
        self.fire(v, e, "onmousemove", vec![]);
        if !captured || !self.el(v, e).state.dragging {
            return;
        }
        match self.el(v, e).kind {
            Kind::Slider => {
                let pos = if self.slider_vertical(v, e) { y } else { x } - self.el(v, e).state.drag_offset;
                let val = self.slider_value_at(v, e, pos);
                self.set_value(v, e, "value", val.into(), true);
            }
            Kind::CustomSlider => {
                if let Some(val) = self.custom_slider_value(v, e, x, y) {
                    self.set_value(v, e, "value", val.into(), true);
                }
            }
            _ => {}
        }
    }

    fn element_mouse_up(&mut self, v: ViewId, e: usize, _x: f64, _y: f64, button: MouseButton, inside: bool) {
        let was_dragging = std::mem::take(&mut self.el_mut(v, e).state.dragging);
        self.el_mut(v, e).pressed = false;
        self.fire(v, e, "onmouseup", vec![]);
        if inside && button == MouseButton::Left && self.enabled(v, e) {
            if matches!(self.el(v, e).kind, Kind::Button | Kind::GroupButton) && self.get_bool(v, e, "sticky") {
                let down = !self.get_bool(v, e, "down");
                self.set_value(v, e, "down", down.into(), true);
            }
            self.fire(v, e, "onclick", vec![]);
        }
        self.view_mut(v).invalidate();
        if was_dragging && matches!(self.el(v, e).kind, Kind::Slider | Kind::CustomSlider) {
            self.fire(v, e, "ondragend", vec![]);
        }
    }

    fn element_wheel(&mut self, v: ViewId, e: usize, delta: f64) {
        let (min, max) = (self.get_num(v, e, "min"), self.get_num(v, e, "max"));
        match self.el(v, e).kind {
            Kind::Slider if self.enabled(v, e) => {
                let mut step = self.get_num(v, e, "keyboardincrement");
                if step <= 0.0 {
                    step = (max - min) / 20.0;
                }
                let val = (self.get_num(v, e, "value") + delta.signum() * step).clamp(min.min(max), min.max(max));
                self.set_value(v, e, "value", val.into(), true);
                self.fire(v, e, "ondragend", vec![]);
            }
            Kind::CustomSlider => {
                let val = (self.get_num(v, e, "value") + delta.signum() * (max - min) / 20.0).clamp(min, max);
                self.set_value(v, e, "value", val.into(), true);
            }
            Kind::Playlist => self.scroll_playlist(v, e, delta),
            Kind::ListBox => {
                let (_, row, inset) = self.listbox_layout(v, e);
                let shown = ((f64::from(self.height(v, e)) - 2.0 * inset) / row).floor().max(1.0) as i32;
                let last = (self.el(v, e).state.items.len() as i32 - shown).max(0);
                let first = (self.get_int(v, e, "firstvisibleitem") - delta.signum() as i32).clamp(0, last);
                self.set_value(v, e, "firstvisibleitem", f64::from(first).into(), true);
                self.view_mut(v).invalidate();
            }
            _ => {}
        }
    }

    fn element_key_down(&mut self, v: ViewId, e: usize, vk: u32) -> bool {
        if self.el(v, e).kind != Kind::Slider {
            return false;
        }
        let (min, max) = (self.get_num(v, e, "min"), self.get_num(v, e, "max"));
        let mut step = self.get_num(v, e, "keyboardincrement");
        if step <= 0.0 {
            step = (max - min) / 20.0;
        }
        let cur = self.get_num(v, e, "value");
        let next = match vk {
            0x25 | 0x28 => (cur - step).max(min), // left, down
            0x27 | 0x26 => (cur + step).min(max), // right, up
            0x24 => min,                          // home
            0x23 => max,                          // end
            _ => return false,
        };
        self.set_value(v, e, "value", next.into(), true);
        true
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

/// What a press means for the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseResult {
    /// An element took it (and gets the drag and release).
    Taken,
    /// Drag the window.
    Drag,
    /// Show the app menu.
    ContextMenu,
    /// Nothing there (a transparent pixel).
    Ignored,
}

/// Builds a view and its script context; returns its id. The window isn't shown yet.
pub fn build(shared: &Rc<Shared>, node: &WmsNode) -> ViewId {
    let (v, plan, label) = shared.with(|s| {
        let v = s.views.len();
        let mut view = View::new(node.attr("id").map(str::to_string));
        view.elements.push(Element::new("VIEW", Kind::View));
        s.views.push(Some(view));
        let mut plan = Plan::default();
        s.apply_attributes(v, 0, node, &mut plan);
        let mut order = 1;
        for c in &node.children {
            s.build_node(v, c, 0, &mut order, &mut plan);
        }
        let label = format!("{}:{}", s.key, node.attr("id").unwrap_or("view"));
        (v, plan, label)
    });
    let script = shared.register_script(v, ScriptHost::new(shared.runtime(), label, Rc::downgrade(shared)));
    let errors = |shared: &Shared, r: Result<(), String>| {
        if let Err(e) = r
            && shared.keep_error(e)
        {
            shared.with(|s| s.view_mut(v).error_count += 1);
        }
    };

    // Element handlers.
    let describe_all: Vec<String> = shared.read(|s| plan.handlers.iter().map(|(e, name, _)| format!("{}.{name}", describe(s, v, *e))).collect());
    for ((e, name, code), place) in plan.handlers.iter().zip(describe_all) {
        match script.compile(code, &place, 1, &[]) {
            Ok(Some(f)) => shared.with(|s| {
                s.el_mut(v, *e).handlers.insert(name.clone(), f);
            }),
            Ok(None) => {}
            Err(err) => errors(shared, Err(err)),
        }
    }

    // <PLAYER ...> handlers.
    for Pseudo { tag, scopes, attributes } in &plan.pseudo {
        for (name, code) in attributes {
            let lower = name.to_lowercase();
            let place = format!("{tag}.{name}");
            if let Some(prop) = lower.strip_suffix("_onchange") {
                match script.compile(code, &place, scopes.len(), &[]) {
                    Ok(Some(f)) => {
                        shared.with(|s| s.view_mut(v).object_handlers.push((*scopes.last().expect("target"), prop.to_string(), f, scopes.clone())))
                    }
                    Ok(None) => {}
                    Err(err) => errors(shared, Err(err)),
                }
            } else if tag == "PLAYER" || lower.starts_with("on") {
                let ev = if lower.starts_with("on") && objects::event_params(name).is_none() { &name[2..] } else { name.as_str() };
                let params = objects::event_params(ev).unwrap_or(&[]);
                match script.compile(code, &place, scopes.len(), params) {
                    Ok(Some(f)) => shared.with(|s| s.view_mut(v).player_handlers.push((ev.to_string(), f, scopes.clone()))),
                    Ok(None) => {}
                    Err(err) => errors(shared, Err(err)),
                }
            }
        }
    }

    // Globals: element ids, then the WMP objects.
    let (preview, view_id) = shared.read(|s| (s.preview, s.view(v).id.clone()));
    if !preview {
        script.set_global("player", &ObjRef::Player.into());
    }
    for (name, o) in [("mediacenter", ObjRef::MediaCenter), ("mediaCenter", ObjRef::MediaCenter), ("theme", ObjRef::Theme), ("event", ObjRef::Event)]
    {
        script.set_global(name, &o.into());
    }
    script.set_global("view", &ObjRef::Element { view: v, element: 0 }.into());
    for (name, value) in crate::player::state::ALL {
        script.set_global(name, &f64::from(*value).into());
    }
    for id in &plan.ids {
        if let Some(e) = shared.read(|s| s.view(v).by_id.get(&id.to_lowercase()).copied()) {
            script.set_global(id, &ObjRef::Element { view: v, element: e }.into());
        }
    }
    if let Some(id) = &view_id {
        script.set_global(id, &ObjRef::Element { view: v, element: 0 }.into());
    }

    // Scripts. Without scriptFile, WMP loaded the script named like the skin definition.
    let (files, package) = shared.read(|s| {
        let mut files = s.get_str(v, 0, "scriptfile");
        if !s.has_value(v, 0, "scriptfile") {
            let def = &s.package.definition;
            let same = format!("{}.js", def.rsplit_once('.').map_or(def.as_str(), |(stem, _)| stem));
            if s.package.exists(&same) {
                files = same;
            }
        }
        (files, s.package.clone())
    });
    let mut sources = vec![];
    for f in files.split(';').map(str::trim).filter(|f| !f.is_empty()) {
        let code = if resources::is_resource_url(f) { resources::read_text(f) } else { package.read_text(f) };
        match code {
            Some(code) => {
                errors(shared, script.execute(&code, f));
                sources.push(code);
            }
            None => crate::log(&format!("[{label}] script not found: {f}", label = shared.read(|s| s.key.clone()))),
        }
    }
    collect_sources(node, &mut sources);
    script.add_case_aliases(&sources);

    // jscript: attributes first (document order), then live bindings.
    for (e, attr, spec) in plan.deferred.iter().filter(|d| has_prefix(&d.2, "jscript:")) {
        let place = shared.read(|s| format!("{}.{attr}", describe(s, v, *e)));
        let value = match script.eval_in(ObjRef::Element { view: v, element: *e }, &spec[8..], &place) {
            Ok(Value::Js(f)) if !f.is_function => Value::Null,
            Ok(val) => val,
            Err(err) => {
                errors(shared, Err(err));
                Value::Null
            }
        };
        shared.with(|s| {
            if !value.is_null() || !s.has_value(v, *e, attr) {
                s.set_value(v, *e, attr, value, false);
            }
        });
    }
    for (e, attr, spec) in plan.deferred.iter().filter(|d| !has_prefix(&d.2, "jscript:")) {
        bind(shared, &script, v, *e, attr, spec);
    }

    shared.with(|s| {
        s.view_mut(v).loading = false;
        s.capture_layout_baseline(v, 0);
    });
    v
}

/// Handler and jscript: attribute code, for the case-insensitive global aliases.
fn collect_sources(node: &WmsNode, out: &mut Vec<String>) {
    for (k, val) in &node.attributes {
        if is_handler_name(k) || has_prefix(val, "jscript:") {
            out.push(val.clone());
        }
    }
    for c in &node.children {
        collect_sources(c, out);
    }
}

fn bind(shared: &Shared, script: &ScriptHost, v: ViewId, e: usize, attr: &str, spec: &str) {
    let Some((kind, path)) = spec.split_once(':') else { return };
    let parts: Vec<&str> = path.split('.').map(str::trim).filter(|p| !p.is_empty()).collect();
    if parts.len() < 2 {
        return;
    }
    let source = shared.with(|s| {
        let mut obj = s.resolve_root(v, parts[0], script);
        for p in &parts[1..parts.len() - 1] {
            obj = obj.and_then(|o| match s.get(o, p) {
                Value::Obj(n) => Some(n),
                _ => None,
            });
        }
        obj
    });
    let Some(source) = source else {
        crate::log(&format!("[{}] can't bind {attr} to {spec}", shared.read(|s| s.key.clone())));
        return;
    };
    let prop = parts[parts.len() - 1].to_string();
    let kind = match kind.to_lowercase().as_str() {
        "wmpprop" => BindKind::Prop,
        "wmpdisabled" => BindKind::Disabled,
        _ => BindKind::Enabled,
    };
    let watch = if kind == BindKind::Prop { prop.to_lowercase() } else { "isavailable".to_string() };
    shared.with(|s| {
        let view = s.view_mut(v);
        view.bindings.push(Binding { source, watch, target: e, attr: attr.to_string(), prop, kind });
        let i = view.bindings.len() - 1;
        s.apply_binding(v, i);
    });
}

/// POPUP.show(x, y): a menu of the script's items; selectedItem reports the choice.
pub fn show_popup(shared: &Shared, v: ViewId, e: usize, args: &[Value]) -> Value {
    let (items, at) = shared.read(|s| {
        let selected = s.get_int(v, e, "selecteditem");
        let items: Vec<MenuItem> = s
            .el(v, e)
            .state
            .items
            .iter()
            .enumerate()
            .map(|(i, label)| {
                if label == "-" || label.is_empty() {
                    MenuItem::separator()
                } else {
                    MenuItem::item(&label.replace('&', ""), true, i as i32 == selected)
                }
            })
            .collect();
        let at = if args.len() >= 2 { (args[0].to_number(), args[1].to_number()) } else { s.absolute(v, e) };
        (items, at)
    });
    if items.is_empty() {
        return Value::Null;
    }
    if let Some(&index) = shared.host.show_menu(v, &items, at).as_ref().and_then(|p| p.first()) {
        // Raise selectedItem_onchange even if the same item is chosen again.
        shared.with(|s| {
            s.set_value(v, e, "selecteditem", (-2.0).into(), false);
            s.set_value(v, e, "selecteditem", index.into(), true);
        });
    }
    Value::Null
}

impl Skin {
    /// Element methods (moveTo, POPUP.appendItem, ...); None when the element has no such method.
    pub(super) fn element_call(&mut self, v: ViewId, e: usize, name: &str, a: &[Value]) -> Option<Value> {
        let arg = |i: usize| a.get(i).cloned().unwrap_or_default();
        let num = |i: usize| arg(i).to_number();
        let kind = self.el(v, e).kind;
        let r: Value = match (kind, name) {
            (_, "moveto") => {
                self.move_to(v, e, num(0), num(1), num(2));
                Value::Null
            }
            (_, "alphablendto") => {
                self.alpha_blend_to(v, e, num(0), num(1));
                Value::Null
            }
            (_, "resizeto") => {
                self.resize_to(v, e, num(0), num(1), num(2));
                Value::Null
            }
            (_, "setfocus") => {
                self.focus(v, Some(e));
                Value::Null
            }
            (_, "removeelement") => Value::Null,
            (Kind::View, "close") => {
                self.deferred.push(super::Deferred::CloseView(v));
                Value::Null
            }
            (Kind::View, "minimize") => {
                self.host_call(move |h| h.minimize(v));
                Value::Null
            }
            (Kind::View, "maximize" | "restore" | "settimeout") => Value::Null,
            (Kind::View, "returntomediacenter" | "returntomedia") => {
                self.host_call(|h| h.choose_skin());
                Value::Null
            }
            (Kind::View, "size") => {
                let edge = arg(0).to_str();
                self.host_call(move |h| h.begin_resize(v, &edge));
                Value::Null
            }
            (Kind::View, "beginmove") => {
                self.host_call(move |h| h.begin_move(v));
                Value::Null
            }
            (Kind::Effects | Kind::Video, _) => {
                let index = (!a.is_empty()).then(|| num(0).max(0.0) as usize);
                self.effects_call(v, e, name, index)
            }
            (Kind::EqualizerSettings, "presettitle") => {
                element::EQ_PRESETS.get(num(0).max(0.0) as usize).map_or(Value::Str(String::new()), |p| p.0.into())
            }
            // gainLevels(band) reads a band (0-based); gainLevels(band, value) is the assignment form.
            (Kind::EqualizerSettings, "gainlevels") => {
                let band = format!("gainlevel{}", num(0).clamp(0.0, 9.0) as i32 + 1);
                if a.len() > 1 {
                    self.set_value(v, e, &band, arg(1), true);
                }
                self.get_num(v, e, &band).into()
            }
            (Kind::EqualizerSettings, "reset") => {
                for i in 1..=10 {
                    self.set_value(v, e, &format!("gainlevel{i}"), 0.0.into(), true);
                }
                Value::Null
            }
            (Kind::EqualizerSettings, "nextpreset" | "previouspreset") => {
                let n = element::EQ_PRESETS.len() as i32;
                let step = if name == "nextpreset" { 1 } else { n - 1 };
                let p = (self.get_int(v, e, "currentpreset") + step).rem_euclid(n);
                self.set_value(v, e, "currentpreset", f64::from(p).into(), true);
                Value::Null
            }
            (Kind::EqualizerSettings, "nextspeaker" | "previousspeaker") => {
                let step = if name == "nextspeaker" { 1 } else { 2 };
                let p = (self.get_int(v, e, "speakersize") + step).rem_euclid(3);
                self.set_value(v, e, "speakersize", f64::from(p).into(), true);
                Value::Null
            }
            (Kind::EqualizerSettings, "resetsettings") => Value::Null,
            (Kind::VideoSettings, "reset") => {
                for k in ["brightness", "contrast", "hue", "saturation"] {
                    self.set_value(v, e, k, 0.0.into(), true);
                }
                Value::Null
            }
            (Kind::ListBox, "appenditem") if self.get_bool(v, e, "sorted") => {
                let line = arg(0).to_str();
                let items = &mut self.el_mut(v, e).state.items;
                let at = items.partition_point(|i| i.to_lowercase() <= line.to_lowercase());
                items.insert(at, line);
                self.view_mut(v).invalidate();
                at.into()
            }
            (Kind::Popup | Kind::ListBox, "appenditem") => {
                let items = &mut self.el_mut(v, e).state.items;
                items.push(arg(0).to_str());
                let at = items.len() - 1;
                self.view_mut(v).invalidate();
                at.into()
            }
            (Kind::ListBox, "insertitem") => {
                let items = &mut self.el_mut(v, e).state.items;
                let at = (num(0).max(0.0) as usize).min(items.len());
                items.insert(at, arg(1).to_str());
                self.view_mut(v).invalidate();
                Value::Null
            }
            (Kind::Popup | Kind::ListBox, "deleteitem") => {
                let i = num(0);
                if i >= 0.0 && (i as usize) < self.el(v, e).state.items.len() {
                    self.el_mut(v, e).state.items.remove(i as usize);
                    let selected = self.get_int(v, e, "selecteditem");
                    if kind == Kind::ListBox && selected >= i as i32 {
                        let now = if selected == i as i32 { -1 } else { selected - 1 };
                        self.set_value(v, e, "selecteditem", f64::from(now).into(), false);
                    }
                    self.view_mut(v).invalidate();
                }
                Value::Null
            }
            (Kind::Popup | Kind::ListBox, "deleteall") => {
                self.el_mut(v, e).state.items.clear();
                if kind == Kind::ListBox {
                    self.set_value(v, e, "selecteditem", (-1.0).into(), false);
                    self.set_value(v, e, "firstvisibleitem", 0.0.into(), false);
                }
                self.view_mut(v).invalidate();
                Value::Null
            }
            (Kind::ListBox, "getitem") => self.el(v, e).state.items.get(num(0).max(0.0) as usize).cloned().unwrap_or_default().into(),
            (Kind::ListBox, "replaceitem") => {
                if let Some(line) = self.el_mut(v, e).state.items.get_mut(num(0).max(0.0) as usize) {
                    *line = arg(1).to_str();
                    self.view_mut(v).invalidate();
                }
                Value::Null
            }
            // A Windows list box's find: the first line after `start` that begins with the text,
            // ignoring case, wrapping round.
            (Kind::ListBox, "finditem") => {
                let (items, text) = (&self.el(v, e).state.items, arg(1).to_str().to_lowercase());
                let n = items.len();
                let start = (num(0) + 1.0).max(0.0) as usize;
                (0..n).map(|k| (start + k) % n).find(|&i| items[i].to_lowercase().starts_with(&text)).map_or(-1.0, |i| i as f64).into()
            }
            (Kind::ListBox, "getnextselecteditem") => {
                let selected = self.get_int(v, e, "selecteditem");
                if selected > num(0) as i32 { selected.into() } else { (-1.0).into() }
            }
            (Kind::ListBox, "setselectedstate") => {
                let (i, on) = (num(0) as i32, arg(1).to_bool());
                if on {
                    self.set_value(v, e, "selecteditem", f64::from(i).into(), true);
                } else if self.get_int(v, e, "selecteditem") == i {
                    self.set_value(v, e, "selecteditem", (-1.0).into(), true);
                }
                Value::Null
            }
            (Kind::ListBox, "show" | "dismiss") => {
                self.set_value(v, e, "visible", (name == "show").into(), true);
                Value::Null
            }
            (Kind::Popup, "dismiss") => Value::Null,
            (Kind::Playlist, "getcolumnwidth") => 0.0.into(),
            (Kind::Playlist, "getnextselecteditem" | "getnextcheckeditem") => (-1.0).into(),
            (Kind::Playlist, "getcheckedstate") => false.into(),
            (Kind::Playlist, _) => Value::Null,
            _ => return None,
        };
        Some(r)
    }
}
