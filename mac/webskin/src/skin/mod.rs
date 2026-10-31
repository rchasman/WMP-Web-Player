//! A loaded skin: its views, their elements and scripts, and WMP's object model over the player.
//!
//! All state lives in [`Skin`] behind one `RefCell` in [`Shared`]. Scripts run with that borrow
//! released: Rust code mutates the skin and queues [`Effect`]s (handlers to run), and the entry
//! point runs them once the borrow is dropped, so a handler may freely touch the skin again.

pub mod audit;
mod element;
mod objects;
mod view;
mod visual;

pub use element::Kind;
pub use view::{MouseButton, MouseResult, View};

use crate::images::SkinImages;
use crate::package::SkinPackage;
use crate::player::{MediaSource, PlayerChange, PlayerModel};
use crate::render::Rect;
use crate::script::{JsRef, ScriptHost};
use crate::settings::Settings;
use crate::value::{ObjRef, Value};
use crate::vis::{VisContext, VisFrame};
use crate::wms::{self, WmsNode};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;
use std::time::Instant;

pub type ViewId = usize;

/// One entry of a context or POPUP menu.
#[derive(Clone, Debug, Default)]
pub struct MenuItem {
    pub label: String,
    pub enabled: bool,
    pub checked: bool,
    pub separator: bool,
    pub submenu: Vec<MenuItem>,
}

impl MenuItem {
    pub fn item(label: &str, enabled: bool, checked: bool) -> Self {
        MenuItem { label: label.into(), enabled, checked, ..Default::default() }
    }

    pub fn separator() -> Self {
        MenuItem { separator: true, ..Default::default() }
    }

    pub fn submenu(label: &str, items: Vec<MenuItem>) -> Self {
        MenuItem { label: label.into(), enabled: true, submenu: items, ..Default::default() }
    }
}

/// What the app around the skin does: windows, menus, URLs. All methods are called with the skin
/// unborrowed, so they may call back into [`Shared`].
pub trait Host {
    fn open_url(&self, _url: &str) {}
    /// A view was opened: show its window at its `left`/`top`.
    fn view_opened(&self, _view: ViewId) {}
    fn view_closed(&self, _view: ViewId) {}
    /// The view's size or title changed.
    fn view_changed(&self, _view: ViewId) {}
    fn view_moved(&self, _view: ViewId) {}
    fn minimize(&self, _view: ViewId) {}
    fn begin_move(&self, _view: ViewId) {}
    fn begin_resize(&self, _view: ViewId, _edge: &str) {}
    fn redraw(&self, _view: ViewId) {}
    /// Only `area` of the view changed since the last redraw.
    fn redraw_region(&self, view: ViewId, _area: Rect) {
        self.redraw(view);
    }
    fn set_tooltip(&self, _view: ViewId, _text: Option<&str>) {}
    fn set_cursor(&self, _view: ViewId, _cursor: &str) {}
    /// Shows a menu at view coordinates; returns the chosen item's index path.
    fn show_menu(&self, _view: ViewId, _items: &[MenuItem], _at: (f64, f64)) -> Option<Vec<usize>> {
        None
    }
    fn message_box(&self, _title: &str, _text: &str) {}
    fn play_sound(&self, _bytes: Vec<u8>) {}
    /// The usable desktop area (x, y, width, height) and the full screen size.
    fn work_area(&self) -> Rect {
        Rect::new(0.0, 0.0, 1440.0, 900.0)
    }
    fn screen_size(&self) -> (f64, f64) {
        (1440.0, 900.0)
    }
    /// The skin's last view closed (or a script closed it).
    fn skin_closed(&self) {}
    /// "Open" and the skin chooser: pick another skin.
    fn choose_skin(&self) {}
}

/// A host that does nothing (skin previews and tests).
pub struct NullHost;
impl Host for NullHost {}

#[derive(Clone, Debug)]
pub struct TrackInfo {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration: f64,
}

type HostCall = Box<dyn FnOnce(&dyn Host)>;

/// Script work queued while the skin was borrowed.
enum Effect {
    /// An element's handler (compiled or a function a script stored), with `this` = the element.
    Fire { view: ViewId, element: usize, event: String, args: Vec<Value> },
    /// A handler compiled with scope objects (`<PLAYER playStateChange=...>`).
    Scoped { view: ViewId, handler: JsRef, scopes: Vec<ObjRef>, args: Vec<Value> },
    /// A host call that must happen with the skin unborrowed.
    Host(HostCall),
}

/// Work done on the next tick, as WMP posted it to the message loop.
enum Deferred {
    SwitchView(String),
    OpenView { id: String, at: Option<(f64, f64)> },
    CloseView(ViewId),
    Close,
}

pub struct Skin {
    /// The skin's settings key (its file name).
    pub key: String,
    pub package: Rc<SkinPackage>,
    pub images: SkinImages,
    pub title: String,
    pub author: String,
    pub copyright: String,
    pub has_effects: bool,
    view_nodes: Vec<WmsNode>,
    pub views: Vec<Option<View>>,
    main_view: Option<ViewId>,
    pub active_view: Option<ViewId>,
    pub player: PlayerModel,
    pub settings: Settings,
    /// WMP's Skin Chooser ran previews without a player object; some skins rely on that.
    pub preview: bool,
    closed: bool,
    pub history: Vec<TrackInfo>,
    /// Expando properties scripts set on WMP's shared objects (mediacenter's are persisted).
    expandos: HashMap<ObjRef, HashMap<String, Value>>,
    event: HashMap<String, Value>,
    effects: Vec<Effect>,
    deferred: Vec<Deferred>,
    work_area: Rect,
    screen: (f64, f64),
    /// The track as the Album Art visualization shows it.
    vis_context: VisContext,
    /// The time of the last tick: timers, animations and animated GIFs all run on it, so a
    /// drawing and a hit test made between ticks agree.
    now: Instant,
}

impl Skin {
    pub fn view(&self, v: ViewId) -> &View {
        self.views[v].as_ref().expect("open view")
    }

    pub fn view_mut(&mut self, v: ViewId) -> &mut View {
        self.views[v].as_mut().expect("open view")
    }

    pub fn open_views(&self) -> impl Iterator<Item = ViewId> + '_ {
        self.views.iter().enumerate().filter(|(_, v)| v.as_ref().is_some_and(|v| v.open)).map(|(i, _)| i)
    }

    fn host_call(&mut self, f: impl FnOnce(&dyn Host) + 'static) {
        self.effects.push(Effect::Host(Box::new(f)));
    }

    fn fire(&mut self, view: ViewId, element: usize, event: &str, args: Vec<Value>) {
        if self.has_handler(view, element, event) {
            self.effects.push(Effect::Fire { view, element, event: event.to_string(), args });
        }
    }

    /// Stores a (coerced) element property and raises change notifications if it differs.
    pub fn set_value(&mut self, v: ViewId, e: usize, name: &str, value: Value, notify: bool) {
        let name = name.to_lowercase();
        let kind = self.el(v, e).kind;
        let coerced = value.coerce(element::type_of(kind, &name));
        let had = self.el(v, e).values.get(&name).cloned();
        let old = had.clone().unwrap_or_else(|| self.el_get(v, e, &name));
        if had.is_some() && old == coerced {
            return;
        }
        self.el_mut(v, e).values.insert(name.clone(), coerced.clone());
        if had.is_none() && old == coerced {
            return;
        }
        self.on_value_changed(v, e, &name);
        if notify {
            self.notify(ObjRef::Element { view: v, element: e }, &name);
        }
    }

    /// Something about `obj.prop` changed: run *_onchange handlers and update bindings.
    pub fn notify(&mut self, obj: ObjRef, prop: &str) {
        let prop = prop.to_lowercase();
        if let ObjRef::Element { view, element } = obj
            && self.views.get(view).is_some_and(Option::is_some)
        {
            self.on_element_changed(view, element, &prop);
        }
        let open: Vec<ViewId> = self.views.iter().enumerate().filter(|(_, v)| v.is_some()).map(|(i, _)| i).collect();
        for v in open {
            self.apply_bindings(v, obj, &prop);
            let view = self.view(v);
            if view.loading || !view.open {
                continue;
            }
            let handlers: Vec<(JsRef, Vec<ObjRef>)> =
                view.object_handlers.iter().filter(|h| h.0 == obj && h.1 == prop).map(|h| (h.2.clone(), h.3.clone())).collect();
            for (handler, scopes) in handlers {
                self.effects.push(Effect::Scoped { view: v, handler, scopes, args: vec![] });
            }
        }
    }

    /// Applies what the player reported.
    fn apply_player_changes(&mut self, changes: Vec<PlayerChange>) {
        for change in changes {
            match change {
                PlayerChange::Prop(obj, prop) => {
                    let o = match obj {
                        "player" => ObjRef::Player,
                        "controls" => ObjRef::Controls,
                        "settings" => ObjRef::Settings,
                        _ => ObjRef::CurrentMedia,
                    };
                    self.notify(o, prop);
                }
                PlayerChange::Event(name, mut args) => {
                    if name == "MediaChange" {
                        self.record_history();
                        self.images.set_artwork(self.player.source.artwork());
                        self.refresh_vis_context();
                        self.invalidate_all();
                    }
                    if matches!(name, "MediaChange" | "CurrentItemChange") {
                        args = vec![ObjRef::CurrentMedia.into()];
                    }
                    self.player_event(name, args);
                }
            }
        }
    }

    fn player_event(&mut self, name: &str, args: Vec<Value>) {
        let open: Vec<ViewId> = self.open_views().collect();
        for v in open {
            let view = self.view(v);
            if view.loading {
                continue;
            }
            let handlers: Vec<(JsRef, Vec<ObjRef>)> =
                view.player_handlers.iter().filter(|h| h.0.eq_ignore_ascii_case(name)).map(|h| (h.1.clone(), h.2.clone())).collect();
            for (handler, scopes) in handlers {
                self.effects.push(Effect::Scoped { view: v, handler, scopes, args: args.clone() });
            }
        }
    }

    fn record_history(&mut self) {
        let m = &self.player.media;
        if m.title.is_empty() {
            return;
        }
        let duration = self.player.duration();
        if let Some(last) = self.history.last_mut().filter(|l| l.title == m.title && l.artist == m.artist) {
            last.duration = duration;
            return;
        }
        self.history.push(TrackInfo { title: m.title.clone(), artist: m.artist.clone(), album: m.album.clone(), duration });
        if self.history.len() > 200 {
            self.history.remove(0);
        }
        self.notify(ObjRef::CurrentPlaylist, "count");
        self.player_event("CurrentPlaylistChange", vec![6.into()]);
    }

    pub fn invalidate_all(&mut self) {
        for v in self.views.iter_mut().flatten() {
            v.invalidate();
        }
    }

    // ---- theme services ------------------------------------------------------------------

    pub fn load_preference(&self, name: &str) -> String {
        self.settings.skin_preferences.get(&self.key).and_then(|p| p.get(&name.to_lowercase())).cloned().unwrap_or_else(|| "--".into())
    }

    pub fn save_preference(&mut self, name: &str, value: String) {
        if !self.preview {
            self.settings.skin_preferences.entry(self.key.clone()).or_default().insert(name.to_lowercase(), value);
        }
    }

    pub fn load_string(&self, id: &str) -> String {
        if crate::resources::is_resource_url(id) {
            return crate::resources::read_text(id).unwrap_or_default();
        }
        let numeric = id.strip_prefix('#').unwrap_or(id);
        if numeric.parse::<u32>().is_ok() {
            return crate::resources::read_text(&format!("res://wmploc.dll/RT_STRING/#{numeric}")).unwrap_or_default();
        }
        id.to_string()
    }

    fn play_sound(&mut self, file: &str) {
        if self.preview {
            return;
        }
        let bytes = if crate::resources::is_resource_url(file) {
            crate::resources::read_bytes(file)
        } else {
            self.package.read_bytes(file).map(<[u8]>::to_vec)
        };
        if let Some(b) = bytes {
            self.host_call(move |h| h.play_sound(b));
        }
    }

    /// The right-click / AUTOMENU menu: transport and app commands.
    pub fn app_menu(&self, which: &str) -> (Vec<MenuItem>, Vec<AppAction>) {
        let pm = &self.player;
        let playing = pm.play_state == crate::player::state::PS_PLAYING;
        let play = vec![
            (MenuItem::item(if playing { "Pause" } else { "Play" }, true, false), AppAction::PlayPause),
            (MenuItem::item("Stop", true, false), AppAction::Stop),
            (MenuItem::item("Previous", pm.is_available("previous"), false), AppAction::Previous),
            (MenuItem::item("Next", pm.is_available("next"), false), AppAction::Next),
            (MenuItem::separator(), AppAction::None),
            (MenuItem::item("Mute", true, pm.mute()), AppAction::Mute),
            (MenuItem::item("Volume Up", true, false), AppAction::VolumeUp),
            (MenuItem::item("Volume Down", true, false), AppAction::VolumeDown),
        ];
        let open = vec![
            (MenuItem::item("Open Skin…", true, false), AppAction::ChooseSkin),
            (MenuItem::item("Spotify", true, false), AppAction::OpenUrl("https://open.spotify.com/")),
            (MenuItem::item("SoundCloud", true, false), AppAction::OpenUrl("https://soundcloud.com/discover")),
            (MenuItem::item("YouTube Music", true, false), AppAction::OpenUrl("https://music.youtube.com/")),
        ];
        let (shown, shown_preset) = self.global_vis();
        let (vis_items, vis_actions): (Vec<_>, Vec<_>) = crate::vis::ALL
            .iter()
            .enumerate()
            .map(|(i, d)| {
                let (items, actions) = d
                    .presets
                    .iter()
                    .enumerate()
                    .map(|(p, name)| (MenuItem::item(name, true, i == shown && p == shown_preset), AppAction::Visualization(i, p)))
                    .unzip();
                (MenuItem::submenu(d.title, items), AppAction::Submenu(actions))
            })
            .unzip();
        let flat = |list: Vec<(MenuItem, AppAction)>| list.into_iter().unzip();
        match which.to_lowercase().as_str() {
            "open" => flat(open),
            "play" => flat(play),
            _ => {
                // Submenus report their choice as a path; the actions list is flattened in order.
                let (open_items, open_actions): (Vec<_>, Vec<_>) = open.into_iter().unzip();
                let (play_items, play_actions): (Vec<_>, Vec<_>) = play.into_iter().unzip();
                let items = vec![
                    MenuItem::submenu("Open", open_items),
                    MenuItem::submenu("Play", play_items),
                    MenuItem::submenu("Visualizations", vis_items),
                    MenuItem::separator(),
                    MenuItem::item("Skin Chooser…", true, false),
                    MenuItem::separator(),
                    MenuItem::item("Quit WebSkin Player", true, false),
                ];
                let mut actions = vec![
                    AppAction::Submenu(open_actions),
                    AppAction::Submenu(play_actions),
                    AppAction::Submenu(vis_actions),
                    AppAction::None,
                    AppAction::ChooseSkin,
                ];
                actions.extend([AppAction::None, AppAction::Quit]);
                (items, actions)
            }
        }
    }
}

/// What a menu entry of [`Skin::app_menu`] does.
#[derive(Clone, Debug, PartialEq)]
pub enum AppAction {
    None,
    Submenu(Vec<AppAction>),
    PlayPause,
    Stop,
    Previous,
    Next,
    Mute,
    VolumeUp,
    VolumeDown,
    ChooseSkin,
    OpenUrl(&'static str),
    /// A visualization and preset, by index into `vis::ALL`.
    Visualization(usize, usize),
    Quit,
}

/// The skin and everything that runs it. Hosts and scripts reach the skin only through here.
pub struct Shared {
    skin: RefCell<Skin>,
    runtime: rquickjs::Runtime,
    scripts: RefCell<HashMap<ViewId, Rc<ScriptHost>>>,
    pub host: Rc<dyn Host>,
    change_depth: Cell<u32>,
    pub errors: RefCell<Vec<String>>,
}

#[derive(Debug)]
pub struct LoadError(pub String);

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for LoadError {}

impl Shared {
    /// Loads a skin (.wmz or .wms). Views open with [`Shared::open_main_view`].
    pub fn load(path: &Path, source: Box<dyn MediaSource>, settings: Settings, host: Rc<dyn Host>, preview: bool) -> Result<Rc<Self>, LoadError> {
        let package = Rc::new(SkinPackage::open(path).map_err(|e| LoadError(e.0))?);
        let text = package.read_text(&package.definition).ok_or_else(|| LoadError("The skin definition can't be read.".into()))?;
        let root = wms::parse(&text);
        let theme = root.children.iter().find(|n| n.tag == "THEME").cloned().unwrap_or(root);
        let view_nodes: Vec<WmsNode> = theme.descendants().into_iter().filter(|n| n.tag == "VIEW").cloned().collect();
        if view_nodes.is_empty() {
            return Err(LoadError("The skin definition has no VIEW.".into()));
        }
        let has_effects = theme.descendants().iter().any(|n| matches!(n.tag.as_str(), "EFFECTS" | "WMPEFFECTS"));
        let key = path.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
        let mut expandos: HashMap<ObjRef, HashMap<String, Value>> = HashMap::new();
        expandos.insert(ObjRef::MediaCenter, settings.media_center.iter().map(|(k, v)| (k.to_lowercase(), objects::restore(v))).collect());
        let skin = Skin {
            key,
            images: SkinImages::new(package.clone()),
            title: theme.attr("title").map(str::to_string).unwrap_or_else(|| path.file_stem().unwrap_or_default().to_string_lossy().into_owned()),
            author: theme.attr("author").unwrap_or_default().to_string(),
            copyright: theme.attr("copyright").unwrap_or_default().to_string(),
            package,
            has_effects,
            view_nodes,
            views: vec![],
            main_view: None,
            active_view: None,
            player: PlayerModel::new(source),
            settings,
            preview,
            closed: false,
            history: vec![],
            expandos,
            event: objects::new_event(),
            effects: vec![],
            deferred: vec![],
            work_area: host.work_area(),
            screen: host.screen_size(),
            vis_context: VisContext::default(),
            now: Instant::now(),
        };
        let shared = Rc::new(Shared {
            skin: RefCell::new(skin),
            runtime: crate::script::new_runtime(),
            scripts: RefCell::default(),
            host,
            change_depth: Cell::new(0),
            errors: RefCell::default(),
        });
        shared.with(|s| {
            s.record_history();
            let art = s.player.source.artwork();
            s.images.set_artwork(art);
            s.refresh_vis_context();
        });
        Ok(shared)
    }

    /// Runs `f` on the skin, then the script work it queued (with the skin unborrowed).
    pub fn with<R>(&self, f: impl FnOnce(&mut Skin) -> R) -> R {
        let (r, effects) = {
            let mut skin = self.skin.borrow_mut();
            let r = f(&mut skin);
            (r, std::mem::take(&mut skin.effects))
        };
        for e in effects {
            self.run(e);
        }
        r
    }

    /// Read-only access (rendering, hit testing). Must not be called from inside [`Shared::with`].
    pub fn read<R>(&self, f: impl FnOnce(&Skin) -> R) -> R {
        f(&self.skin.borrow())
    }

    fn script(&self, view: ViewId) -> Option<Rc<ScriptHost>> {
        self.scripts.borrow().get(&view).cloned()
    }

    fn run(&self, effect: Effect) {
        match effect {
            Effect::Fire { view, element, event, args } => {
                // *_onchange handlers can set properties whose handlers set them back; WMP stopped at a depth.
                let is_change = event.ends_with("_onchange");
                if is_change && self.change_depth.get() >= 8 {
                    return;
                }
                let target = self.read(|s| {
                    let el = s.views.get(view)?.as_ref()?.elements.get(element)?;
                    el.handlers.get(&event).map(|h| (h.clone(), true)).or_else(|| match el.values.get(&event) {
                        Some(Value::Js(f)) if f.is_function => Some((f.clone(), false)),
                        _ => None,
                    })
                });
                let (Some((func, compiled)), Some(script)) = (target, self.script(view)) else { return };
                let this = ObjRef::Element { view, element };
                let args: Vec<Value> = if compiled { std::iter::once(this.into()).chain(args).collect() } else { args };
                self.change_depth.set(self.change_depth.get() + u32::from(is_change));
                let place = self.read(|s| format!("{}.{event}", view::describe(s, view, element)));
                let result = script.call(&func, Some(this), &args, &place);
                self.change_depth.set(self.change_depth.get() - u32::from(is_change));
                self.record_error(view, result.err());
            }
            Effect::Scoped { view, handler, scopes, args } => {
                let Some(script) = self.script(view) else { return };
                let all: Vec<Value> = scopes.into_iter().map(Value::Obj).chain(args).collect();
                let result = script.call(&handler, None, &all, "player event");
                self.record_error(view, result.err());
            }
            Effect::Host(f) => f(self.host.as_ref()),
        }
    }

    /// Keeps a script error unless the same one is already kept, and says whether it was new.
    pub(crate) fn keep_error(&self, e: String) -> bool {
        let mut errors = self.errors.borrow_mut();
        let new = !errors.contains(&e);
        if new {
            errors.push(e);
        }
        new
    }

    fn record_error(&self, view: ViewId, err: Option<String>) {
        if let Some(e) = err
            && self.keep_error(e)
            && let Ok(mut s) = self.skin.try_borrow_mut()
            && let Some(Some(v)) = s.views.get_mut(view)
        {
            v.error_count += 1;
        }
    }

    pub fn open_main_view(self: &Rc<Self>) -> Option<ViewId> {
        let node = self.read(|s| s.view_nodes[0].clone());
        let v = self.open_node(&node, None)?;
        self.with(|s| s.main_view = Some(v));
        Some(v)
    }

    fn open_node(self: &Rc<Self>, node: &WmsNode, at: Option<(f64, f64)>) -> Option<ViewId> {
        let v = view::build(self, node);
        self.with(|s| {
            s.active_view.get_or_insert(v);
        });
        self.open_window(v, at);
        let errors = self.read(|s| s.view(v).error_count);
        if errors > 0 {
            crate::log(&format!("{}: {errors} script error(s) while opening view {v}", self.read(|s| s.key.clone())));
        }
        Some(v)
    }

    /// Fires onload without showing a window (skin previews and the corpus runner).
    pub fn load_main_view_offscreen(self: &Rc<Self>) -> ViewId {
        let node = self.read(|s| s.view_nodes[0].clone());
        let v = view::build(self, &node);
        self.with(|s| {
            s.main_view = Some(v);
            s.active_view = Some(v);
            s.fire(v, 0, "onload", vec![]);
        });
        v
    }

    /// Close a view (the main one, or the last, ends the skin like WMP did).
    fn close_view(&self, v: ViewId) {
        let still_open = self.with(|s| s.views.get(v).is_some_and(Option::is_some));
        if !still_open {
            return;
        }
        self.with(|s| {
            s.fire(v, 0, "onclose", vec![]);
        });
        self.with(|s| {
            s.view_mut(v).open = false;
            s.views[v] = None;
            s.host_call(move |h| h.view_closed(v));
            if s.active_view == Some(v) {
                s.active_view = s.main_view.filter(|&m| m != v).or_else(|| s.open_views().next());
            }
        });
        self.scripts.borrow_mut().remove(&v);
        let ends = self.read(|s| s.open_views().next().is_none() || s.main_view == Some(v));
        if ends {
            self.close();
        }
    }

    pub fn close(&self) {
        let views: Vec<ViewId> = self.with(|s| {
            if s.closed {
                return vec![];
            }
            s.closed = true;
            s.open_views().collect()
        });
        for v in views {
            self.with(|s| s.fire(v, 0, "onclose", vec![]));
            self.with(|s| {
                s.views[v] = None;
                s.host_call(move |h| h.view_closed(v));
            });
        }
        self.scripts.borrow_mut().clear();
        self.read(|s| s.settings.save());
        self.host.skin_closed();
    }

    /// Advances timers, animations and the player; call about 60 times a second.
    pub fn tick(self: &Rc<Self>, now: Instant) {
        if self.read(|s| s.closed) {
            return;
        }
        self.with(|s| {
            s.now = now;
            let changes = s.player.tick(now);
            s.apply_player_changes(changes);
            let views: Vec<ViewId> = s.open_views().collect();
            for v in views {
                s.tick_view(v, now);
            }
        });
        let deferred = self.with(|s| std::mem::take(&mut s.deferred));
        for d in deferred {
            match d {
                Deferred::SwitchView(id) => self.switch_view(&id),
                Deferred::OpenView { id, at } => self.open_view(&id, at),
                Deferred::CloseView(v) => self.close_view(v),
                Deferred::Close => self.close(),
            }
        }
        let dirty: Vec<(ViewId, Option<Rect>)> = self.with(|s| {
            let views: Vec<ViewId> = s.open_views().collect();
            views
                .into_iter()
                .filter_map(|v| {
                    let view = s.view_mut(v);
                    let region = view.dirty_region.take();
                    if std::mem::take(&mut view.dirty) { Some((v, None)) } else { region.map(|r| (v, Some(r))) }
                })
                .collect()
        });
        for (v, region) in dirty {
            match region {
                Some(area) => self.host.redraw_region(v, area),
                None => self.host.redraw(v),
            }
        }
    }

    fn find_view_node(&self, id: &str) -> Option<WmsNode> {
        self.read(|s| s.view_nodes.iter().find(|n| n.attr("id").is_some_and(|i| i.eq_ignore_ascii_case(id))).cloned())
    }

    fn view_by_id(&self, id: &str) -> Option<ViewId> {
        self.read(|s| s.open_views().find(|&v| s.view(v).id.as_deref().is_some_and(|i| i.eq_ignore_ascii_case(id))))
    }

    /// theme.currentViewID = "x": replace the current view with another one.
    fn switch_view(self: &Rc<Self>, id: &str) {
        let from = self.read(|s| s.active_view);
        if from.is_some_and(|f| self.read(|s| s.view(f).id.as_deref().is_some_and(|i| i.eq_ignore_ascii_case(id)))) {
            return;
        }
        let next = match self.view_by_id(id) {
            Some(v) => v,
            None => match self.find_view_node(id).and_then(|n| self.open_node(&n, None)) {
                Some(v) => v,
                None => return,
            },
        };
        self.with(|s| {
            s.main_view = Some(next);
            s.active_view = Some(next);
        });
        if let Some(f) = from.filter(|&f| f != next) {
            self.with(|s| s.fire(f, 0, "onclose", vec![]));
            self.with(|s| {
                s.views[f] = None;
                s.host_call(move |h| h.view_closed(f));
            });
            self.scripts.borrow_mut().remove(&f);
        }
    }

    fn open_view(self: &Rc<Self>, id: &str, at: Option<(f64, f64)>) {
        if self.view_by_id(id).is_some() {
            return;
        }
        match self.find_view_node(id) {
            Some(node) => {
                self.open_node(&node, at);
            }
            None => crate::log(&format!("openView: no view '{id}'")),
        }
    }

    fn open_window(self: &Rc<Self>, v: ViewId, at: Option<(f64, f64)>) {
        self.with(|s| {
            s.place_view(v, at);
            s.view_mut(v).open = true;
            s.host_call(move |h| h.view_opened(v));
            s.restart_timer(v, s.now);
            s.update_all_scrolling(v);
        });
        self.with(|s| s.fire(v, 0, "onload", vec![]));
        self.with(|s| {
            if s.views.get(v).is_some_and(Option::is_some) {
                let focus = s.get_str(v, 0, "focusobjectid");
                if let Some(&f) = s.view(v).by_id.get(&focus.to_lowercase()) {
                    s.focus(v, Some(f));
                }
                s.view_mut(v).invalidate();
            }
        });
    }

    pub fn register_script(&self, v: ViewId, host: ScriptHost) -> Rc<ScriptHost> {
        let host = Rc::new(host);
        self.scripts.borrow_mut().insert(v, host.clone());
        host
    }

    pub fn runtime(&self) -> &rquickjs::Runtime {
        &self.runtime
    }

    /// Runs a chosen [`AppAction`]; returns false for the ones the host handles (Quit).
    pub fn run_action(&self, action: &AppAction) -> bool {
        match action {
            AppAction::PlayPause => self.player(|p| p.toggle_play_pause()),
            AppAction::Stop => self.player(|p| p.stop()),
            AppAction::Previous => self.player(|p| p.previous()),
            AppAction::Next => self.player(|p| p.next()),
            AppAction::Mute => self.player(|p| p.set_mute(!p.mute())),
            AppAction::VolumeUp => self.player(|p| p.set_volume(p.volume() + 5)),
            AppAction::VolumeDown => self.player(|p| p.set_volume(p.volume() - 5)),
            AppAction::ChooseSkin => self.host.choose_skin(),
            AppAction::OpenUrl(url) => self.host.open_url(url),
            AppAction::Visualization(effect, preset) => self.with(|s| s.choose_vis(*effect, *preset)),
            AppAction::Quit => return false,
            AppAction::None | AppAction::Submenu(_) => {}
        }
        true
    }

    /// Whether any visualizer can be seen, so the host knows to listen to the audio.
    pub fn wants_visualization(&self) -> bool {
        self.read(|s| s.wants_visualization())
    }

    /// Draws the next frame of every visible visualizer; the views redraw on the next tick.
    pub fn render_visualizations(&self, frame: &VisFrame, scale: f64) {
        self.with(|s| s.render_visualizers(frame, scale));
    }

    /// Calls the player and delivers the notifications its setters raise.
    pub fn player(&self, f: impl FnOnce(&mut PlayerModel)) {
        self.with(|s| {
            f(&mut s.player);
            let changes = s.player.take_changes();
            s.apply_player_changes(changes);
        });
    }

    /// Shows the app menu for `view` at view coordinates and runs the choice.
    pub fn show_app_menu(&self, view: ViewId, which: &str, at: (f64, f64)) -> Option<AppAction> {
        let (items, actions) = self.read(|s| s.app_menu(which));
        let path = self.host.show_menu(view, &items, at)?;
        let mut list = &actions;
        let mut chosen = None;
        for i in path {
            match list.get(i)? {
                AppAction::Submenu(sub) => list = sub,
                a => chosen = Some(a.clone()),
            }
        }
        let chosen = chosen?;
        if self.run_action(&chosen) { None } else { Some(chosen) }
    }
}

// ---- the script bridge -------------------------------------------------------------------

/// 0: no such member, 1: a property, 2: a method.
pub fn bridge_kind(shared: &Shared, o: ObjRef, key: &str) -> i32 {
    let key = key.to_lowercase();
    let Ok(s) = shared.skin.try_borrow() else { return 0 };
    if s.has_method(o, &key) {
        2
    } else if s.has_member(o, &key) {
        1
    } else {
        0
    }
}

pub fn bridge_get(shared: &Shared, o: ObjRef, key: &str) -> Value {
    shared.with(|s| s.get(o, key))
}

pub fn bridge_set(shared: &Shared, o: ObjRef, key: &str, value: Value) {
    shared.with(|s| s.set(o, key, value));
}

pub fn bridge_keys(shared: &Shared, o: ObjRef) -> Vec<String> {
    shared.read(|s| s.member_names(o))
}

pub fn bridge_call(shared: &Shared, o: ObjRef, key: &str, args: Vec<Value>) -> Value {
    let key = key.to_lowercase();
    // Methods that show something modal run with the skin unborrowed.
    if let ObjRef::Element { view, element } = o {
        let kind = shared.read(|s| s.views.get(view).and_then(Option::as_ref).and_then(|v| v.elements.get(element)).map(|e| e.kind));
        match (kind, key.as_str()) {
            (Some(Kind::Popup), "show") => return view::show_popup(shared, view, element, &args),
            (Some(Kind::AutoMenu), "show") => {
                let which = args.first().map(Value::to_str).unwrap_or_default();
                let at = shared.read(|s| s.absolute(view, element));
                if shared.show_app_menu(view, &which, at) == Some(AppAction::Quit) {
                    shared.with(|s| s.deferred.push(Deferred::Close));
                }
                return Value::Null;
            }
            _ => {}
        }
    }
    if o == ObjRef::Theme && key == "messagebox" {
        let (title, text) = shared.read(|s| (s.title.clone(), args.first().map(Value::to_str).unwrap_or_default()));
        if !shared.read(|s| s.preview) {
            shared.host.message_box(&title, &text);
        }
        return Value::Null;
    }
    shared.with(|s| s.call(o, &key, &args))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::images::Artwork;
    use crate::player::{Command, MediaSnapshot, MediaStatus};
    use std::path::PathBuf;

    struct Recorder(Rc<RefCell<Vec<Command>>>);

    impl MediaSource for Recorder {
        fn snapshot(&self) -> MediaSnapshot {
            MediaSnapshot { status: MediaStatus::Paused, ..crate::runner::Playing::default().snapshot() }
        }
        fn position(&self) -> f64 {
            crate::runner::Playing::default().position()
        }
        fn artwork(&self) -> Option<Rc<Artwork>> {
            None
        }
        fn command(&self, c: Command) {
            self.0.borrow_mut().push(c);
        }
        fn seek(&self, _: f64) {}
        fn set_shuffle(&self, _: bool) {}
        fn set_repeat(&self, _: bool) {}
        fn volume(&self) -> f64 {
            0.5
        }
        fn set_volume(&self, _: f64) {}
        fn mute(&self) -> bool {
            false
        }
        fn set_mute(&self, _: bool) {}
    }

    fn load(dir: &Path) -> (Rc<Shared>, ViewId, Rc<RefCell<Vec<Command>>>) {
        std::fs::write(
            dir.join("t.wms"),
            r#"<THEME title="T"><VIEW id="Main" width="100" height="60" scriptFile="t.js">
                 <SLIDER id="vol" width="80" height="10" value_onchange="label.value = 'v' + value;"/>
                 <TEXT id="label" top="20" value="start"/>
                 <PLAYBUTTON id="play" top="40" width="20" height="20"/>
                 <METADATATEXT id="title" top="40" left="30"/>
               </VIEW></THEME>"#,
        )
        .unwrap();
        std::fs::write(dir.join("t.js"), "function bump() { vol.value = 40; return LABEL.value; }").unwrap();
        let commands = Rc::new(RefCell::new(vec![]));
        let shared = Shared::load(&dir.join("t.wms"), Box::new(Recorder(commands.clone())), Settings::scratch(), Rc::new(NullHost), false).unwrap();
        let v = shared.load_main_view_offscreen();
        (shared, v, commands)
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("webskin-test-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_script_sees_onchange_handlers_run_before_it_continues() {
        let (shared, v, _) = load(&temp_dir("onchange"));
        let script = shared.script(v).unwrap();
        script.execute("var result = bump();", "test").unwrap();
        assert_eq!(script.get_global("result").to_str(), "v40");
        assert!(shared.errors.borrow().is_empty(), "{:?}", shared.errors.borrow());
    }

    #[test]
    fn bindings_follow_the_player_and_buttons_drive_it() {
        let (shared, v, commands) = load(&temp_dir("buttons"));
        let (title, enabled) = shared.read(|s| {
            let by_id = |id: &str| s.view(v).by_id[id];
            (s.get_str(v, by_id("title"), "value"), s.enabled(v, by_id("play")))
        });
        assert_eq!(title, "Glue");
        assert!(enabled, "play is available while paused");
        shared.with(|s| {
            assert_eq!(s.mouse_down(v, 5.0, 45.0, MouseButton::Left, 1, (false, false, false)), MouseResult::Taken);
            s.mouse_up(v, 5.0, 45.0, MouseButton::Left, (false, false, false));
        });
        assert_eq!(*commands.borrow(), vec![Command::Play]);
    }

    #[test]
    fn visualizers_follow_scripts_the_menu_and_mediacenter_and_draw() {
        let dir = temp_dir("vis");
        std::fs::write(
            dir.join("t.wms"),
            r##"<THEME><VIEW id="Main" width="60" height="40" backgroundColor="#102030">
                 <EFFECTS id="vis" width="60" height="40" currentEffectType="wmpprop:mediacenter.effectType" currentEffectType_onchange="mediacenter.effectType = currentEffectType;"/>
               </VIEW></THEME>"##,
        )
        .unwrap();
        let shared = Shared::load(&dir.join("t.wms"), Box::new(crate::player::NoMedia), Settings::scratch(), Rc::new(NullHost), false).unwrap();
        let v = shared.load_main_view_offscreen();
        // As if its window were showing.
        shared.with(|s| s.view_mut(v).open = true);
        let script = shared.script(v).unwrap();
        let eval = |code: &str| {
            script.execute(&format!("var out = {code};"), "test").unwrap();
            script.get_global("out").to_str()
        };
        assert_eq!(eval("vis.currentEffectTitle"), "Bars and Waves");
        assert_eq!(eval("vis.effectCount"), "3");
        script.execute("vis.nextEffect();", "test").unwrap();
        assert_eq!(eval("mediacenter.effectType"), "{8D2A317B-D98B-4efa-89D6-E49F0ACF98A1}", "the skin's choice becomes the player's");
        assert_eq!(eval("vis.currentPresetTitle"), "Whirlpool");
        script.execute("vis.previous();", "test").unwrap();
        assert_eq!(eval("vis.currentPresetTitle"), "Scope", "previous() rolls back into the last preset of Bars and Waves");
        assert!(shared.run_action(&AppAction::Visualization(2, 1)));
        assert_eq!(eval("vis.currentEffectTitle + '/' + vis.currentPresetTitle"), "Album Art/Album Art");
        assert_eq!(eval("mediacenter.effectPresetTitle"), "Album Art");
        assert!(shared.wants_visualization());
        let black = shared.read(|s| s.render_view(v, 1.0)).unwrap();
        shared.render_visualizations(&VisFrame::default(), 1.0);
        let drawn = shared.read(|s| s.render_view(v, 1.0)).unwrap();
        assert_eq!(black.pixmap.pixel(30, 20).map(|p| (p.red(), p.green(), p.blue())), Some((0, 0, 0)));
        assert_ne!(drawn.pixmap.pixel(30, 20).map(|p| (p.red(), p.green(), p.blue())), Some((0, 0, 0)), "Album Art's disc is drawn");
        assert!(shared.errors.borrow().is_empty(), "{:?}", shared.errors.borrow());
    }

    /// A VIEW's colour lies under the children behind it, which show through its image's holes and
    /// its clippingColor (Radio, Yahoo); clippingColor with nothing behind still cuts the window.
    #[test]
    fn children_behind_a_view_show_through_its_holes_and_clipping_colour() {
        let dir = temp_dir("viewbehind");
        // Columns: 0-4 frame (red), 5-9 transparent hole, 10-14 clipping colour (yellow), 15-19 yellow.
        image::RgbaImage::from_fn(20, 10, |x, _| match x {
            0..=4 => image::Rgba([255, 0, 0, 255]),
            5..=9 => image::Rgba([0, 0, 0, 0]),
            _ => image::Rgba([255, 255, 0, 255]),
        })
        .save(dir.join("frame.png"))
        .unwrap();
        std::fs::write(
            dir.join("t.wms"),
            r##"<THEME><VIEW id="Main" width="20" height="10" titleBar="false" backgroundImage="frame.png" backgroundColor="#000080" clippingColor="#FFFF00">
                 <SUBVIEW id="screen" zIndex="-1" width="15" height="10" backgroundColor="#00FF00"/>
               </VIEW></THEME>"##,
        )
        .unwrap();
        let shared = Shared::load(&dir.join("t.wms"), Box::new(crate::player::NoMedia), Settings::scratch(), Rc::new(NullHost), false).unwrap();
        let v = shared.load_main_view_offscreen();
        let canvas = shared.read(|s| s.render_view(v, 1.0)).unwrap();
        let rgba = |x, y| canvas.pixmap.pixel(x, y).map(|p| (p.red(), p.green(), p.blue(), p.alpha()));
        assert_eq!(rgba(2, 5), Some((255, 0, 0, 255)), "the frame covers the child");
        assert_eq!(rgba(7, 5), Some((0, 255, 0, 255)), "the child shows through the hole, over the view's colour");
        assert_eq!(rgba(12, 5), Some((0, 255, 0, 255)), "and through the clipping colour");
        assert_eq!(rgba(17, 5), Some((0, 0, 0, 0)), "clipping colour with nothing behind cuts the window");
        let (frame_hit, hole_hit, screen) =
            shared.read(|s| (s.hit_test_point(v, 2.0, 5.0), s.hit_test_point(v, 7.0, 5.0), s.view(v).by_id["screen"]));
        assert_eq!(frame_hit, Some(0));
        assert_eq!(hole_hit, Some(screen));
    }

    /// WMP shaped the window from the backgrounds: a control drawn where a keyed background is
    /// open was cut, whatever its colour (World of Warcraft's magenta slider ends, xXx's lime
    /// button corners, both checked against real WMP).
    #[test]
    fn a_control_painted_over_a_subview_hole_is_cut() {
        let dir = temp_dir("holes");
        let (key, grey, blue) = (image::Rgba([0, 255, 0, 255]), image::Rgba([90, 90, 90, 255]), image::Rgba([0, 0, 255, 255]));
        // Columns 0-9 the body's key (open), 10-19 the body.
        image::RgbaImage::from_fn(20, 10, |x, _| if x < 10 { key } else { grey }).save(dir.join("body.png")).unwrap();
        // Over the hole: the key, then blue; over the body: the key, then nothing.
        image::RgbaImage::from_fn(20, 5, |x, _| match x {
            0..=4 | 10..=14 => key,
            5..=9 => blue,
            _ => image::Rgba([0, 0, 0, 0]),
        })
        .save(dir.join("button.png"))
        .unwrap();
        std::fs::write(
            dir.join("t.wms"),
            r##"<THEME><VIEW id="Main" width="20" height="10" backgroundColor="none" titleBar="false">
                 <SUBVIEW id="body" backgroundImage="body.png" transparencyColor="#00FF00"/>
                 <SUBVIEW id="drawer" zIndex="1" top="5" width="5" height="5" backgroundColor="#00FF00"/>
                 <BUTTON id="b" zIndex="2" image="button.png"/>
               </VIEW></THEME>"##,
        )
        .unwrap();
        let shared = Shared::load(&dir.join("t.wms"), Box::new(crate::player::NoMedia), Settings::scratch(), Rc::new(NullHost), false).unwrap();
        let v = shared.load_main_view_offscreen();
        let canvas = shared.read(|s| s.render_view(v, 2.0)).unwrap();
        let rgba = |x: u32, y: u32| canvas.pixmap.pixel(x * 2, y * 2).map(|p| (p.red(), p.green(), p.blue(), p.alpha()));
        assert_eq!(rgba(2, 2), Some((0, 0, 0, 0)), "the key painted over the hole is cut");
        assert_eq!(rgba(7, 2), Some((0, 0, 0, 0)), "other colours over the hole are cut too");
        assert_eq!(rgba(12, 2), Some((0, 255, 0, 255)), "the key painted over the body stays");
        assert_eq!(rgba(7, 7), Some((0, 0, 0, 0)), "the hole itself");
        assert_eq!(rgba(2, 7), Some((0, 255, 0, 255)), "a subview filling the hole covers it");
        let part = shared.read(|s| s.render_region(v, 2.0, Rect::new(0.0, 0.0, 10.0, 5.0))).unwrap();
        assert_eq!(part.pixmap.pixel(4, 4).map(|p| p.alpha()), Some(0), "a redrawn region cuts it too");
    }

    /// Need for Speed Underground fades keyed pieces in over its body; drawn in a layer, a
    /// piece's key over the solid body is no hole.
    #[test]
    fn a_fading_subview_leaves_no_hole_over_the_body_below() {
        let dir = temp_dir("fade_holes");
        let (key, grey, blue) = (image::Rgba([255, 0, 255, 255]), image::Rgba([90, 90, 90, 255]), image::Rgba([0, 0, 255, 255]));
        image::RgbaImage::from_pixel(10, 10, grey).save(dir.join("body.png")).unwrap();
        image::RgbaImage::from_pixel(10, 10, key).save(dir.join("piece.png")).unwrap();
        image::RgbaImage::from_pixel(10, 10, blue).save(dir.join("button.png")).unwrap();
        std::fs::write(
            dir.join("t.wms"),
            r##"<THEME><VIEW id="Main" width="10" height="10" backgroundColor="none" titleBar="false">
                 <SUBVIEW id="body" backgroundImage="body.png" transparencyColor="#FF00FF"/>
                 <SUBVIEW id="piece" zIndex="1" backgroundImage="piece.png" transparencyColor="#FF00FF" alphaBlend="128"/>
                 <BUTTON id="b" zIndex="2" image="button.png"/>
               </VIEW></THEME>"##,
        )
        .unwrap();
        let shared = Shared::load(&dir.join("t.wms"), Box::new(crate::player::NoMedia), Settings::scratch(), Rc::new(NullHost), false).unwrap();
        let v = shared.load_main_view_offscreen();
        let canvas = shared.read(|s| s.render_view(v, 1.0)).unwrap();
        assert_eq!(canvas.pixmap.pixel(5, 5).map(|p| (p.blue(), p.alpha())), Some((255, 255)));
    }

    /// Radio keeps its drawer behind the view's frame; the frame covers the drawer's key.
    #[test]
    fn a_view_picture_covers_the_holes_of_children_behind_it() {
        let dir = temp_dir("view_covers");
        let (key, grey, blue) = (image::Rgba([255, 0, 255, 255]), image::Rgba([90, 90, 90, 255]), image::Rgba([0, 0, 255, 255]));
        image::RgbaImage::from_pixel(10, 10, grey).save(dir.join("frame.png")).unwrap();
        image::RgbaImage::from_pixel(10, 10, key).save(dir.join("tray.png")).unwrap();
        image::RgbaImage::from_pixel(10, 10, blue).save(dir.join("button.png")).unwrap();
        std::fs::write(
            dir.join("t.wms"),
            r##"<THEME><VIEW id="Main" width="10" height="10" backgroundImage="frame.png" titleBar="false">
                 <SUBVIEW id="tray" zIndex="-1" backgroundImage="tray.png" transparencyColor="#FF00FF"/>
                 <BUTTON id="b" zIndex="1" image="button.png"/>
               </VIEW></THEME>"##,
        )
        .unwrap();
        let shared = Shared::load(&dir.join("t.wms"), Box::new(crate::player::NoMedia), Settings::scratch(), Rc::new(NullHost), false).unwrap();
        let v = shared.load_main_view_offscreen();
        let canvas = shared.read(|s| s.render_view(v, 1.0)).unwrap();
        assert_eq!(canvas.pixmap.pixel(5, 5).map(|p| (p.blue(), p.alpha())), Some((255, 255)));
    }

    /// YIL Online Music Awards' speakers were saved from a 16-bit desktop: their key colour
    /// #6699FF reads back as #639CFF.
    #[test]
    fn a_key_colour_saved_in_16_bits_is_keyed() {
        let dir = temp_dir("key16");
        let colours = [[0x66, 0x99, 0xFF], [0x63, 0x9C, 0xFF], [0x63, 0x9A, 0xFF], [0x64, 0x9C, 0xFF]];
        image::RgbImage::from_fn(4, 1, |x, _| image::Rgb(colours[x as usize])).save(dir.join("speaker.bmp")).unwrap();
        std::fs::write(
            dir.join("t.wms"),
            r##"<THEME><VIEW id="Main" width="4" height="1" backgroundColor="none" titleBar="false">
                 <BUTTON id="speaker" image="speaker.bmp" transparencyColor="#6699FF"/>
               </VIEW></THEME>"##,
        )
        .unwrap();
        let shared = Shared::load(&dir.join("t.wms"), Box::new(crate::player::NoMedia), Settings::scratch(), Rc::new(NullHost), false).unwrap();
        let v = shared.load_main_view_offscreen();
        let canvas = shared.read(|s| s.render_view(v, 1.0)).unwrap();
        let alpha: Vec<u8> = (0..4).map(|x| canvas.pixmap.pixel(x, 0).unwrap().alpha()).collect();
        assert_eq!(alpha, [0, 0, 0, 255], "the key, its 5-5-5 and 5-6-5 forms are keyed; a near colour is not");
    }

    /// A film strip one frame wide steps top to bottom (Windows XP Media Center Edition's seek bar).
    #[test]
    fn a_custom_slider_steps_through_a_vertical_strip() {
        let dir = temp_dir("strip");
        let colours = [[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255]];
        image::RgbaImage::from_fn(4, 6, |_, y| image::Rgba(colours[(y / 2) as usize])).save(dir.join("strip.png")).unwrap();
        image::RgbaImage::from_pixel(4, 2, image::Rgba([0, 0, 0, 255])).save(dir.join("map.png")).unwrap();
        std::fs::write(
            dir.join("t.wms"),
            r##"<THEME><VIEW id="Main" width="10" height="10" backgroundColor="none" titleBar="false">
                 <CUSTOMSLIDER id="s" image="strip.png" positionImage="map.png" min="0" max="2" value="1"/>
               </VIEW></THEME>"##,
        )
        .unwrap();
        let shared = Shared::load(&dir.join("t.wms"), Box::new(crate::player::NoMedia), Settings::scratch(), Rc::new(NullHost), false).unwrap();
        let v = shared.load_main_view_offscreen();
        let canvas = shared.read(|s| s.render_view(v, 1.0)).unwrap();
        let rgba = |x, y| canvas.pixmap.pixel(x, y).map(|p| (p.red(), p.green(), p.blue(), p.alpha()));
        assert_eq!(rgba(1, 1), Some((0, 255, 0, 255)), "value 1 of 0..2 shows the middle frame");
        assert_eq!(rgba(1, 2), Some((0, 0, 0, 0)), "only one frame tall");
    }

    #[test]
    fn a_region_draws_the_same_pixels_as_the_whole_view() {
        let dir = temp_dir("region");
        let magenta = image::Rgba([255, 0, 255, 255]);
        image::RgbaImage::from_fn(30, 20, |x, y| if (x + y) % 7 == 0 { magenta } else { image::Rgba([(x * 8) as u8, (y * 12) as u8, 90, 255]) })
            .save(dir.join("bg.png"))
            .unwrap();
        std::fs::write(
            dir.join("t.wms"),
            r##"<THEME><VIEW id="Main" width="40" height="30" backgroundColor="none" titleBar="false" transparencyColor="#000001">
                 <SUBVIEW left="3" top="2" backgroundImage="bg.png" clippingColor="#FF00FF" alphaBlend="200">
                   <TEXT left="1" top="3" value="Glue" foregroundColor="#FFFFFF"/>
                   <EFFECTS zIndex="-1" left="4" top="4" width="20" height="10"/>
                 </SUBVIEW>
                 <SUBVIEW left="15" top="12" width="20" height="15" backgroundColor="#204060" alphaBlend="128"/>
               </VIEW></THEME>"##,
        )
        .unwrap();
        let shared = Shared::load(&dir.join("t.wms"), Box::new(crate::player::NoMedia), Settings::scratch(), Rc::new(NullHost), false).unwrap();
        let v = shared.load_main_view_offscreen();
        for scale in [1.0, 2.0] {
            let full = shared.read(|s| s.render_view(v, scale)).unwrap();
            for area in [Rect::new(7.0, 6.0, 20.0, 10.0), Rect::new(0.0, 0.0, 9.5, 31.0), Rect::new(14.2, 11.7, 3.1, 9.0)] {
                let part = shared.read(|s| s.render_region(v, scale, area)).unwrap();
                let (ox, oy) = ((part.origin.0 * f64::from(scale)) as u32, (part.origin.1 * f64::from(scale)) as u32);
                for y in 0..part.pixmap.height() {
                    for x in 0..part.pixmap.width() {
                        assert_eq!(part.pixmap.pixel(x, y), full.pixmap.pixel(ox + x, oy + y), "scale {scale}, area {area:?}, pixel {x},{y}");
                    }
                }
            }
        }
    }

    /// Left: a dome subview (magenta left strip, clear elsewhere) over a visualization, as in
    /// Plus! Bionic Dot. Right: a red frame with a magenta hole over a green negative-zIndex
    /// subview, as in Creed and Goo.
    #[test]
    fn visualizations_stay_in_their_subview_and_negative_zindex_sits_behind() {
        let dir = temp_dir("zindex");
        let magenta = image::Rgba([255, 0, 255, 255]);
        image::RgbaImage::from_fn(20, 20, |x, _| if x < 5 { magenta } else { image::Rgba([0, 0, 0, 0]) }).save(dir.join("dome.png")).unwrap();
        image::RgbaImage::from_fn(20, 20, |x, _| if (5..15).contains(&x) { magenta } else { image::Rgba([255, 0, 0, 255]) })
            .save(dir.join("frame.png"))
            .unwrap();
        std::fs::write(
            dir.join("t.wms"),
            r##"<THEME><VIEW id="Main" width="40" height="20" backgroundColor="none" titleBar="false">
                 <SUBVIEW backgroundImage="dome.png" transparencyColor="#FF00FF">
                   <EFFECTS zIndex="-15" width="20" height="20"/>
                 </SUBVIEW>
                 <SUBVIEW id="frame" left="20" backgroundImage="frame.png" transparencyColor="#FF00FF">
                   <SUBVIEW id="behind" zIndex="-1" width="20" height="20" backgroundColor="#00FF00"/>
                 </SUBVIEW>
               </VIEW></THEME>"##,
        )
        .unwrap();
        let shared = Shared::load(&dir.join("t.wms"), Box::new(crate::player::NoMedia), Settings::scratch(), Rc::new(NullHost), false).unwrap();
        let v = shared.load_main_view_offscreen();
        let canvas = shared.read(|s| s.render_view(v, 1.0)).unwrap();
        let rgba = |x, y| {
            let p = canvas.pixmap.pixel(x, y).unwrap();
            (p.red(), p.green(), p.blue(), p.alpha())
        };
        assert_eq!(rgba(2, 10).3, 0, "the visualization is cut out of the dome's magenta");
        assert_eq!(rgba(12, 10), (0, 0, 0, 255), "the visualization shows inside the dome");
        assert_eq!(rgba(21, 10), (255, 0, 0, 255), "the frame covers its negative-zIndex child");
        assert_eq!(rgba(30, 10), (0, 255, 0, 255), "the child shows through the frame's hole");
        let (hit_frame, hit_hole, frame, behind) = shared.read(|s| {
            let id = |name: &str| s.view(v).by_id[name];
            (s.hit_test_point(v, 21.0, 10.0), s.hit_test_point(v, 30.0, 10.0), id("frame"), id("behind"))
        });
        assert_eq!(hit_frame, Some(frame), "clicks on the frame reach the frame, not the child behind it");
        assert_eq!(hit_hole, Some(behind), "clicks through the hole reach the child");
    }

    /// Constantine and Media Center group their controls in window-sized subviews with nothing
    /// to draw, placed above other controls.
    #[test]
    fn an_empty_subview_lets_clicks_reach_the_controls_under_it() {
        let dir = temp_dir("empty-subview");
        std::fs::write(
            dir.join("t.wms"),
            r##"<THEME><VIEW id="Main" width="40" height="20">
                 <BUTTON id="play" width="20" height="20" onClick="x = 1;"/>
                 <SUBVIEW id="group" zIndex="5" width="40" height="20">
                   <BUTTON id="stop" left="20" width="20" height="20" onClick="x = 2;"/>
                 </SUBVIEW>
                 <SUBVIEW id="handle" zIndex="6" top="15" width="5" height="5" onmousedown="x = 1;"/>
               </VIEW></THEME>"##,
        )
        .unwrap();
        let shared = Shared::load(&dir.join("t.wms"), Box::new(crate::player::NoMedia), Settings::scratch(), Rc::new(NullHost), false).unwrap();
        let v = shared.load_main_view_offscreen();
        let (play, stop, handle) = shared.read(|s| {
            let id = |name: &str| Some(s.view(v).by_id[name]);
            assert_eq!(s.hit_test_point(v, 5.0, 5.0), id("play"), "the empty subview above lets the click through");
            assert_eq!(s.hit_test_point(v, 25.0, 5.0), id("stop"), "its own children still take clicks");
            (id("play"), id("stop"), (s.hit_test_point(v, 2.0, 17.0), id("handle")))
        });
        assert!(play.is_some() && stop.is_some());
        assert_eq!(handle.0, handle.1, "an empty subview with a mouse handler still takes clicks");
    }

    /// QuickSilver lays disabled buttons over its real ones as artwork.
    #[test]
    fn a_disabled_button_lets_clicks_reach_the_button_under_it() {
        let dir = temp_dir("disabled-cover");
        std::fs::write(
            dir.join("t.wms"),
            r##"<THEME><VIEW id="Main" width="20" height="20">
                 <BUTTON id="real" width="20" height="20" onClick="x = 1;"/>
                 <BUTTON id="cover" zIndex="1" width="20" height="20" enabled="false"/>
               </VIEW></THEME>"##,
        )
        .unwrap();
        let shared = Shared::load(&dir.join("t.wms"), Box::new(crate::player::NoMedia), Settings::scratch(), Rc::new(NullHost), false).unwrap();
        let v = shared.load_main_view_offscreen();
        let (hit, real) = shared.read(|s| (s.hit_test_point(v, 5.0, 5.0), s.view(v).by_id["real"]));
        assert_eq!(hit, Some(real));
    }

    /// Blue Crush's title box overlaps its close button.
    #[test]
    fn text_without_mouse_handlers_lets_clicks_through() {
        let dir = temp_dir("text-over");
        std::fs::write(
            dir.join("t.wms"),
            r##"<THEME><VIEW id="Main" width="40" height="20">
                 <BUTTON id="close" width="40" height="20" onClick="x = 1;"/>
                 <TEXT id="title" zIndex="1" width="20" height="20" value="Glue"/>
                 <TEXT id="link" zIndex="1" left="20" width="20" height="20" value="Bicep" onClick="x = 2;"/>
               </VIEW></THEME>"##,
        )
        .unwrap();
        let shared = Shared::load(&dir.join("t.wms"), Box::new(crate::player::NoMedia), Settings::scratch(), Rc::new(NullHost), false).unwrap();
        let v = shared.load_main_view_offscreen();
        shared.read(|s| {
            let id = |name: &str| Some(s.view(v).by_id[name]);
            assert_eq!(s.hit_test_point(v, 5.0, 5.0), id("close"));
            assert_eq!(s.hit_test_point(v, 25.0, 5.0), id("link"), "text with a click handler takes clicks");
        });
    }

    /// 9 Series tiles its left edge, whose see-through pixels show the playlist grip under it.
    #[test]
    fn a_tiled_background_lets_clicks_through_its_holes() {
        let dir = temp_dir("tiled-holes");
        image::RgbaImage::from_fn(2, 1, |x, _| if x == 0 { image::Rgba([255, 0, 255, 255]) } else { image::Rgba([90, 90, 90, 255]) })
            .save(dir.join("edge.png"))
            .unwrap();
        std::fs::write(
            dir.join("t.wms"),
            r##"<THEME><VIEW id="Main" width="20" height="20">
                 <BUTTON id="grip" width="20" height="20" onClick="x = 1;"/>
                 <SUBVIEW id="edge" zIndex="1" width="20" height="20" backgroundImage="edge.png" transparencyColor="#FF00FF" backgroundTiled="true"/>
               </VIEW></THEME>"##,
        )
        .unwrap();
        let shared = Shared::load(&dir.join("t.wms"), Box::new(crate::player::NoMedia), Settings::scratch(), Rc::new(NullHost), false).unwrap();
        let v = shared.load_main_view_offscreen();
        shared.read(|s| {
            let id = |name: &str| Some(s.view(v).by_id[name]);
            assert_eq!(s.hit_test_point(v, 4.5, 9.5), id("grip"), "a see-through pixel of the tiles");
            assert_eq!(s.hit_test_point(v, 5.5, 9.5), id("edge"), "a solid one");
        });
    }

    /// anime's animated background covers its sliders only in some frames.
    #[test]
    fn an_animated_background_takes_clicks_where_its_shown_frame_is_solid() {
        let dir = temp_dir("gif-hit");
        let mut file = std::fs::File::create(dir.join("anim.gif")).unwrap();
        let mut gif = gif::Encoder::new(&mut file, 4, 4, &[0, 0, 0, 90, 90, 90]).unwrap();
        gif.set_repeat(gif::Repeat::Infinite).unwrap();
        for (solid, delay) in [(true, 2), (false, 1000)] {
            let mut frame = gif::Frame::from_indexed_pixels(4, 4, vec![if solid { 1 } else { 0 }; 16], Some(0));
            frame.delay = delay;
            frame.dispose = gif::DisposalMethod::Background;
            gif.write_frame(&frame).unwrap();
        }
        drop(gif);
        std::fs::write(
            dir.join("t.wms"),
            r##"<THEME><VIEW id="Main" width="4" height="4">
                 <BUTTON id="under" width="4" height="4" onClick="x = 1;"/>
                 <SUBVIEW id="anim" zIndex="1" backgroundImage="anim.gif"/>
               </VIEW></THEME>"##,
        )
        .unwrap();
        let shared = Shared::load(&dir.join("t.wms"), Box::new(crate::player::NoMedia), Settings::scratch(), Rc::new(NullHost), false).unwrap();
        let v = shared.load_main_view_offscreen();
        let id = |name: &str| shared.read(|s| Some(s.view(v).by_id[name]));
        assert_eq!(shared.read(|s| s.hit_test_point(v, 1.0, 1.0)), id("anim"), "the first frame is solid");
        shared.read(|s| s.render_view(v, 1.0));
        shared.tick(Instant::now() + std::time::Duration::from_millis(60));
        assert_eq!(shared.read(|s| s.hit_test_point(v, 1.0, 1.0)), id("under"), "the shown frame is clear");
    }

    /// Secura's display subview is far larger than its picture, over the next button.
    #[test]
    fn a_subview_takes_no_clicks_past_its_picture() {
        let dir = temp_dir("past-picture");
        image::RgbaImage::from_pixel(4, 4, image::Rgba([90, 90, 90, 255])).save(dir.join("small.png")).unwrap();
        std::fs::write(
            dir.join("t.wms"),
            r##"<THEME><VIEW id="Main" width="20" height="20">
                 <BUTTON id="next" width="20" height="20" onClick="x = 1;"/>
                 <SUBVIEW id="display" zIndex="1" width="20" height="20" backgroundImage="small.png"/>
               </VIEW></THEME>"##,
        )
        .unwrap();
        let shared = Shared::load(&dir.join("t.wms"), Box::new(crate::player::NoMedia), Settings::scratch(), Rc::new(NullHost), false).unwrap();
        let v = shared.load_main_view_offscreen();
        shared.read(|s| {
            let id = |name: &str| Some(s.view(v).by_id[name]);
            assert_eq!(s.hit_test_point(v, 1.5, 1.5), id("display"));
            assert_eq!(s.hit_test_point(v, 6.5, 0.5), id("next"), "right of the picture, not its next row");
        });
    }

    /// WMP filled a frameless window's empty background with magenta and keyed it out, so
    /// magenta vanished wherever it was drawn: over a hole with something under it (WWC's intro
    /// over its pipes) or over solid parts.
    #[test]
    fn a_frameless_window_loses_magenta_everywhere() {
        let dir = temp_dir("magenta");
        let (pink, grey) = (image::Rgba([255, 0, 255, 255]), image::Rgba([90, 90, 90, 255]));
        image::RgbaImage::from_fn(4, 1, |x, _| if x < 2 { pink } else { grey }).save(dir.join("art.png")).unwrap();
        let wms = |title_bar: &str| {
            format!(
                r##"<THEME><VIEW id="Main" width="4" height="1" titleBar="{title_bar}">
                     <SUBVIEW backgroundColor="#5A5A5A" width="4" height="1"/>
                     <BUTTON zIndex="1" image="art.png"/>
                   </VIEW></THEME>"##
            )
        };
        for (title_bar, alpha) in [("false", 0), ("true", 255)] {
            std::fs::write(dir.join("t.wms"), wms(title_bar)).unwrap();
            let shared = Shared::load(&dir.join("t.wms"), Box::new(crate::player::NoMedia), Settings::scratch(), Rc::new(NullHost), false).unwrap();
            let v = shared.load_main_view_offscreen();
            let canvas = shared.read(|s| s.render_view(v, 1.0)).unwrap();
            assert_eq!(canvas.pixmap.pixel(0, 0).map(|p| p.alpha()), Some(alpha), "titleBar={title_bar}");
            assert_eq!(canvas.pixmap.pixel(3, 0).map(|p| p.alpha()), Some(255), "grey stays");
        }
    }

    /// Catwoman's time digits sit in a subview given only a position.
    #[test]
    fn a_handler_failing_on_every_tick_is_kept_once() {
        let dir = temp_dir("repeat-error");
        std::fs::write(
            dir.join("t.wms"),
            r##"<THEME><VIEW id="Main" width="4" height="4" timerInterval="100" onTimer="missing.value = 1;"/></THEME>"##,
        )
        .unwrap();
        let shared = Shared::load(&dir.join("t.wms"), Box::new(crate::player::NoMedia), Settings::scratch(), Rc::new(NullHost), false).unwrap();
        shared.open_main_view();
        let start = Instant::now();
        for step in 1..=5 {
            shared.tick(start + std::time::Duration::from_millis(step * 150));
        }
        assert_eq!(shared.errors.borrow().len(), 1, "{:?}", shared.errors.borrow());
    }

    #[test]
    fn a_subview_without_size_or_picture_holds_its_children() {
        let dir = temp_dir("auto-size");
        image::RgbaImage::from_pixel(4, 4, image::Rgba([90, 90, 90, 255])).save(dir.join("digit.png")).unwrap();
        std::fs::write(
            dir.join("t.wms"),
            r##"<THEME><VIEW id="Main" width="20" height="20">
                 <SUBVIEW id="time" left="2" top="2">
                   <BUTTON left="5" top="5" image="digit.png"/>
                 </SUBVIEW>
                 <SUBVIEW id="strings">
                   <TEXT value="Insert an audio CD" foregroundColor="#FFFFFF"/>
                 </SUBVIEW>
               </VIEW></THEME>"##,
        )
        .unwrap();
        let shared = Shared::load(&dir.join("t.wms"), Box::new(crate::player::NoMedia), Settings::scratch(), Rc::new(NullHost), false).unwrap();
        let v = shared.load_main_view_offscreen();
        let (size, strings, canvas) = shared.read(|s| {
            let (time, strings) = (s.view(v).by_id["time"], s.view(v).by_id["strings"]);
            ((s.width(v, time), s.height(v, time)), (s.width(v, strings), s.height(v, strings)), s.render_view(v, 1.0).unwrap())
        });
        assert_eq!(size, (9, 9));
        assert_eq!(canvas.pixmap.pixel(8, 8).map(|p| p.alpha()), Some(255), "the digit shows");
        assert_eq!(strings, (0, 0), "a string table stays hidden");
    }
}
