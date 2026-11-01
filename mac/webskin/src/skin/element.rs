//! Skin elements: their scriptable properties, drawing and pointer behaviour.

use super::{Skin, ViewId};
use crate::images::{self, SkinBitmap, parse_color};
use crate::render::{Backdrop, Canvas, Rect, pixel_at};
use crate::script::JsRef;
use crate::text::{self, Align, TextStyle};
use crate::value::{PType, Value};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    View,
    Subview,
    Button,
    ButtonGroup,
    GroupButton,
    Slider,
    CustomSlider,
    Text,
    Effects,
    Video,
    Playlist,
    EqualizerSettings,
    VideoSettings,
    Popup,
    /// A POPUP shown in the window (WMP's docs: identical but for popUp's default).
    ListBox,
    AutoMenu,
    NonVisual,
}

/// Runtime state some kinds keep besides their properties.
#[derive(Default, Debug)]
pub struct State {
    pub dragging: bool,
    pub drag_offset: f64,
    /// Marquee offset of a scrolling TEXT.
    pub scroll: f64,
    /// A POPUP's or LISTBOX's lines.
    pub items: Vec<String>,
    pub playlist_top: usize,
    /// EQUALIZERSETTINGS applying a preset (its gain changes don't switch to Custom), or EFFECTS
    /// reporting the visualization it switched to.
    pub applying: bool,
    /// The visualization an EFFECTS (or stand-in VIDEO) element shows, once it has shown one.
    pub vis: Option<crate::vis::Renderer>,
}

pub struct Element {
    pub tag: String,
    pub kind: Kind,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    /// Document order, which breaks zIndex ties.
    pub order: usize,
    /// Property values by lower-case name.
    pub values: HashMap<String, Value>,
    /// Lower-case event name ("onclick", "value_onchange") → compiled handler.
    pub handlers: HashMap<String, JsRef>,
    pub hovered: bool,
    pub pressed: bool,
    /// Size recorded after load, the baseline for alignment when the parent resizes.
    pub baseline: Option<(i32, i32)>,
    measuring: Cell<bool>,
    /// When each animated GIF this element shows started.
    anim_start: RefCell<HashMap<String, Instant>>,
    pub state: State,
}

impl Element {
    pub fn new(tag: &str, kind: Kind) -> Self {
        Element {
            tag: tag.to_string(),
            kind,
            parent: None,
            children: vec![],
            order: 0,
            values: HashMap::new(),
            handlers: HashMap::new(),
            hovered: false,
            pressed: false,
            baseline: None,
            measuring: Cell::new(false),
            anim_start: RefCell::default(),
            state: State::default(),
        }
    }

    pub fn id(&self) -> Option<&str> {
        match self.values.get("id") {
            Some(Value::Str(s)) if !s.is_empty() => Some(s),
            _ => None,
        }
    }

    pub fn restart_animations(&self) {
        self.anim_start.borrow_mut().clear();
    }
}

pub fn kind_for(tag: &str, parent: Kind) -> Kind {
    match tag {
        "SUBVIEW" => Kind::Subview,
        "BUTTONGROUP" => Kind::ButtonGroup,
        "SLIDER" | "SEEKSLIDER" | "VOLUMESLIDER" | "BALANCESLIDER" | "PROGRESSBAR" => Kind::Slider,
        "CUSTOMSLIDER" => Kind::CustomSlider,
        "TEXT" | "STATUSTEXT" | "CURRENTPOSITIONTEXT" | "METADATATEXT" | "DURATIONTEXT" => Kind::Text,
        "EFFECTS" | "WMPEFFECTS" => Kind::Effects,
        "VIDEO" | "WMPVIDEO" => Kind::Video,
        "PLAYLIST" | "ITEMSPLAYLIST" | "DROPDOWNPLAYLIST" => Kind::Playlist,
        "LISTBOX" => Kind::ListBox,
        "EQUALIZERSETTINGS" => Kind::EqualizerSettings,
        "VIDEOSETTINGS" => Kind::VideoSettings,
        "POPUP" => Kind::Popup,
        "AUTOMENU" => Kind::AutoMenu,
        _ if parent == Kind::ButtonGroup && tag.ends_with("ELEMENT") => Kind::GroupButton,
        _ if tag.ends_with("BUTTON") => Kind::Button,
        _ if tag.ends_with("ELEMENT") => Kind::GroupButton,
        _ => Kind::NonVisual,
    }
}

// ---- schemas -----------------------------------------------------------------------------

use PType::{Bool as B, Int as I, Number as N, String as S};

const COMMON: &[(&str, PType)] = &[
    ("id", S),
    ("left", I),
    ("top", I),
    ("width", I),
    ("height", I),
    ("visible", B),
    ("enabled", B),
    ("zindex", I),
    ("alphablend", I),
    ("horizontalalignment", S),
    ("verticalalignment", S),
    ("tooltip", S),
    ("uptooltip", S),
    ("downtooltip", S),
    ("cursor", S),
    ("passthrough", B),
    ("tabstop", B),
    ("showfocus", B),
    ("clippingcolor", S),
    ("clippingimage", S),
    ("transparencycolor", S),
    ("hueshift", N),
    ("saturation", N),
    ("accname", S),
    ("accdescription", S),
    ("acckeyboardshortcut", S),
    ("elementtype", S),
];
const VIEW: &[(&str, PType)] = &[
    ("title", S),
    ("titlebar", B),
    ("resizable", B),
    ("minwidth", I),
    ("minheight", I),
    ("maxwidth", I),
    ("maxheight", I),
    ("timerinterval", I),
    ("scriptfile", S),
    ("focusobjectid", S),
    ("backgroundcolor", S),
    ("backgroundimage", S),
    ("backgroundtiled", B),
    ("category", S),
    ("stickyborderwidth", I),
];
const SUBVIEW: &[(&str, PType)] =
    &[("backgroundcolor", S), ("backgroundimage", S), ("backgroundtiled", B), ("resizebackgroundimage", B), ("resizeimages", B)];
const BUTTON: &[(&str, PType)] =
    &[("image", S), ("hoverimage", S), ("downimage", S), ("disabledimage", S), ("hoverdownimage", S), ("down", B), ("sticky", B)];
const BUTTON_GROUP: &[(&str, PType)] =
    &[("image", S), ("hoverimage", S), ("downimage", S), ("disabledimage", S), ("hoverdownimage", S), ("mappingimage", S), ("showbackground", B)];
const GROUP_BUTTON: &[(&str, PType)] = &[("mappingcolor", S), ("down", B), ("sticky", B)];
const SLIDER: &[(&str, PType)] = &[
    ("min", N),
    ("max", N),
    ("value", N),
    ("direction", S),
    ("backgroundimage", S),
    ("foregroundimage", S),
    ("thumbimage", S),
    ("thumbhoverimage", S),
    ("thumbdownimage", S),
    ("thumbdisabledimage", S),
    ("disabledimage", S),
    ("backgroundhoverimage", S),
    ("foregroundhoverimage", S),
    ("bordersize", I),
    ("tiled", B),
    ("slide", B),
    ("foregroundprogress", N),
    ("useforegroundprogress", B),
    ("keyboardincrement", N),
    ("invert", B),
];
const CUSTOM_SLIDER: &[(&str, PType)] = &[("min", N), ("max", N), ("value", N), ("image", S), ("positionimage", S)];
const TEXT: &[(&str, PType)] = &[
    ("value", S),
    ("foregroundcolor", S),
    ("backgroundcolor", S),
    ("hoverforegroundcolor", S),
    ("hoverbackgroundcolor", S),
    ("disabledforegroundcolor", S),
    ("fontface", S),
    ("fontsize", N),
    ("fontstyle", S),
    ("hoverfontstyle", S),
    ("disabledfontstyle", S),
    ("fontsmoothing", B),
    ("fonttype", S),
    ("justification", S),
    ("scrolling", B),
    ("scrollingdelay", I),
    ("scrollingamount", I),
    ("scrollingdirection", S),
    ("wordwrap", B),
    ("truncate", B),
];
const EFFECTS: &[(&str, PType)] = &[
    ("effecttype", S),
    ("preset", I),
    ("currenteffecttype", S),
    ("currentpreset", I),
    ("windowed", B),
    ("allowall", B),
    ("fullscreen", B),
    ("backgroundcolor", S),
];
const VIDEO: &[(&str, PType)] = &[
    ("backgroundcolor", S),
    ("stretchtofit", B),
    ("zoom", I),
    ("windowless", B),
    ("maintainaspectratio", B),
    ("fullscreen", B),
    ("cursor", S),
    ("shrinktofit", B),
];
const PLAYLIST: &[(&str, PType)] = &[
    ("backgroundcolor", S),
    ("foregroundcolor", S),
    ("itemplayingcolor", S),
    ("itemplayingbackgroundcolor", S),
    ("disableditemcolor", S),
    ("backgroundimage", S),
    ("columns", S),
    ("columnsvisible", B),
    ("fontface", S),
    ("fontsize", N),
    ("dropdownvisible", B),
    ("selectedindex", I),
];
const EQUALIZER: &[(&str, PType)] = &[
    ("gainlevel1", N),
    ("gainlevel2", N),
    ("gainlevel3", N),
    ("gainlevel4", N),
    ("gainlevel5", N),
    ("gainlevel6", N),
    ("gainlevel7", N),
    ("gainlevel8", N),
    ("gainlevel9", N),
    ("gainlevel10", N),
    ("enabled", B),
    ("enable", B),
    ("bypass", B),
    ("currentpreset", I),
    ("enhancedaudio", B),
    ("trubasslevel", I),
    ("wowlevel", I),
    ("splinetension", N),
    ("enablesplinetension", B),
    ("speakersize", I),
    ("normalization", B),
    ("truebass", I),
];
const VIDEO_SETTINGS: &[(&str, PType)] = &[("brightness", I), ("contrast", I), ("hue", I), ("saturation", I)];
const POPUP: &[(&str, PType)] = &[("selecteditem", I), ("backgroundcolor", S), ("foregroundcolor", S)];
const LISTBOX: &[(&str, PType)] = &[
    ("selecteditem", I),
    ("firstvisibleitem", I),
    ("backgroundcolor", S),
    ("foregroundcolor", S),
    ("fontface", S),
    ("fontsize", N),
    ("fontstyle", S),
    ("border", B),
    ("sorted", B),
];

fn schema(kind: Kind) -> &'static [(&'static str, PType)] {
    match kind {
        Kind::View => VIEW,
        Kind::Subview => SUBVIEW,
        Kind::Button => BUTTON,
        Kind::ButtonGroup => BUTTON_GROUP,
        Kind::GroupButton => GROUP_BUTTON,
        Kind::Slider => SLIDER,
        Kind::CustomSlider => CUSTOM_SLIDER,
        Kind::Text => TEXT,
        Kind::Effects => EFFECTS,
        Kind::Video => VIDEO,
        Kind::Playlist => PLAYLIST,
        Kind::EqualizerSettings => EQUALIZER,
        Kind::VideoSettings => VIDEO_SETTINGS,
        Kind::Popup => POPUP,
        Kind::ListBox => LISTBOX,
        Kind::AutoMenu | Kind::NonVisual => &[],
    }
}

/// A view also has its subview properties.
fn kind_schemas(kind: Kind) -> [&'static [(&'static str, PType)]; 2] {
    [schema(kind), if kind == Kind::View { SUBVIEW } else { &[] }]
}

pub fn type_of(kind: Kind, name: &str) -> PType {
    kind_schemas(kind).iter().flat_map(|s| s.iter()).chain(COMMON).find(|(n, _)| *n == name).map_or(PType::Any, |&(_, t)| t)
}

pub fn is_declared(kind: Kind, name: &str) -> bool {
    type_of(kind, name) != PType::Any
}

// WMP's stock equalizer presets: name and ten gains (-14..+14 dB).
pub const EQ_PRESETS: &[(&str, [f64; 10])] = &[
    ("Acoustic", [3., 3., 2., 1., 0., 0., 1., 2., 3., 3.]),
    ("Blues", [4., 3., 1., 0., -1., -1., 0., 1., 2., 3.]),
    ("Classical", [4., 3., 2., 1., -1., -1., 0., 2., 3., 4.]),
    ("Country", [2., 2., 1., 0., -1., 0., 1., 2., 2., 2.]),
    ("Dance", [6., 5., 2., 0., 0., -2., -2., -2., 0., 0.]),
    ("Grunge", [4., 2., 0., -2., -1., 0., 2., 3., 2., 1.]),
    ("Jazz", [3., 2., 1., 2., -1., -1., 0., 1., 2., 3.]),
    ("Metal", [5., 4., 3., 1., 0., 0., 1., 3., 4., 5.]),
    ("New Age", [3., 3., 1., 0., 0., 0., 1., 2., 3., 3.]),
    ("Oldies", [-2., 0., 2., 3., 3., 2., 1., 0., -1., -2.]),
    ("Opera", [0., 0., 1., 3., 4., 3., 2., 1., 0., 0.]),
    ("Rap", [6., 5., 3., 1., -1., -1., 0., 1., 2., 3.]),
    ("Reggae", [3., 2., 0., -2., 0., 2., 3., 3., 2., 1.]),
    ("Rock", [5., 4., 3., 1., -1., -1., 1., 3., 4., 5.]),
    ("Speech", [-3., -2., 0., 2., 4., 4., 3., 1., 0., -2.]),
    ("Swing", [2., 1., 0., 1., 2., 2., 1., 0., 1., 2.]),
    ("Techno", [5., 4., 1., -1., -1., 0., 1., 3., 4., 5.]),
    ("Custom", [0.; 10]),
];

/// Built-in behaviour of a predefined button/element (PLAYBUTTON, PAUSEELEMENT, ...).
pub struct Preset {
    pub action: &'static str,
    pub enabled: Option<&'static str>,
    pub tooltip: &'static str,
    pub sticky: bool,
    pub down: Option<&'static str>,
}

pub fn preset(tag: &str) -> Option<Preset> {
    let t = tag.strip_suffix("BUTTON").or_else(|| tag.strip_suffix("ELEMENT"))?;
    let (action, enabled, tooltip, sticky, down) = match t {
        "PLAY" => ("player.controls.play();", Some("wmpenabled:player.controls.play"), "Play", false, None),
        "PAUSE" => ("player.controls.pause();", Some("wmpenabled:player.controls.pause"), "Pause", false, None),
        "STOP" => ("player.controls.stop();", Some("wmpenabled:player.controls.stop"), "Stop", false, None),
        "PREV" => ("player.controls.previous();", Some("wmpenabled:player.controls.previous"), "Previous", false, None),
        "NEXT" => ("player.controls.next();", Some("wmpenabled:player.controls.next"), "Next", false, None),
        "FFWD" => ("player.controls.fastForward();", Some("wmpenabled:player.controls.fastForward"), "Fast Forward", false, None),
        "REW" => ("player.controls.fastReverse();", Some("wmpenabled:player.controls.fastReverse"), "Rewind", false, None),
        "MUTE" => ("player.settings.mute = down;", None, "Mute", true, Some("wmpprop:player.settings.mute")),
        "OPEN" => ("theme.openDialog('FILE_OPEN','');", None, "Open", false, None),
        "RETURN" => ("view.returnToMediaCenter();", None, "Return to Full Mode", false, None),
        "SHUFFLE" => ("player.settings.setMode('shuffle', down);", None, "Shuffle", true, Some("wmpprop:player.settings.shuffle")),
        "REPEAT" | "LOOP" => ("player.settings.setMode('loop', down);", None, "Repeat", true, Some("wmpprop:player.settings.loop")),
        _ => return None,
    };
    Some(Preset { action, enabled, tooltip, sticky, down })
}

// ---- properties --------------------------------------------------------------------------

impl Skin {
    pub fn el(&self, v: ViewId, e: usize) -> &Element {
        &self.view(v).elements[e]
    }

    pub fn el_mut(&mut self, v: ViewId, e: usize) -> &mut Element {
        &mut self.view_mut(v).elements[e]
    }

    pub fn get_str(&self, v: ViewId, e: usize, name: &str) -> String {
        self.el_get(v, e, name).to_str()
    }

    pub fn get_num(&self, v: ViewId, e: usize, name: &str) -> f64 {
        self.el_get(v, e, name).to_number()
    }

    pub fn get_int(&self, v: ViewId, e: usize, name: &str) -> i32 {
        self.el_get(v, e, name).to_int()
    }

    pub fn get_bool(&self, v: ViewId, e: usize, name: &str) -> bool {
        self.el_get(v, e, name).to_bool()
    }

    pub fn has_value(&self, v: ViewId, e: usize, name: &str) -> bool {
        self.el(v, e).values.contains_key(name)
    }

    /// A property: computed members first, then stored values, then WMP's defaults.
    pub fn el_get(&self, v: ViewId, e: usize, name: &str) -> Value {
        let name = name.to_lowercase();
        if let Some(c) = self.el_computed(v, e, &name) {
            return c;
        }
        if let Some(val) = self.el(v, e).values.get(&name) {
            return val.clone();
        }
        self.el_default(v, e, &name)
    }

    fn el_default(&self, v: ViewId, e: usize, name: &str) -> Value {
        let el = self.el(v, e);
        match name {
            "visible" | "enabled" => return true.into(),
            "alphablend" => return 255.0.into(),
            "saturation" if el.kind != Kind::VideoSettings => return 1.0.into(),
            "left" | "top" | "zindex" | "hueshift" => return 0.0.into(),
            "width" | "height" => {
                if el.measuring.get() {
                    return 0.0.into();
                }
                el.measuring.set(true);
                let n = self.natural_size(v, e);
                self.el(v, e).measuring.set(false);
                return f64::from(if name == "width" { n.0 } else { n.1 }).into();
            }
            "horizontalalignment" => return "left".into(),
            "verticalalignment" => return "top".into(),
            "tabstop" => return false.into(),
            _ => {}
        }
        match (el.kind, name) {
            (Kind::View, "titlebar" | "resizable") => true.into(),
            (Kind::View, "title") => self.title.clone().into(),
            (Kind::ButtonGroup, "showbackground") => false.into(),
            (Kind::Slider, "min") | (Kind::CustomSlider, "min") => 0.0.into(),
            (Kind::Slider, "max") | (Kind::CustomSlider, "max") => 100.0.into(),
            (Kind::Slider, "value") | (Kind::CustomSlider, "value") => 0.0.into(),
            (Kind::Slider, "direction") => "horizontal".into(),
            (Kind::Slider, "slide") => true.into(),
            (Kind::Slider, "bordersize") => 0.0.into(),
            (Kind::Text, "foregroundcolor") => "#FFFFFF".into(),
            (Kind::Text, "backgroundcolor") => "none".into(),
            (Kind::Text, "fontface") | (Kind::Playlist, "fontface") => "Tahoma".into(),
            (Kind::Text, "fontsize") | (Kind::Playlist, "fontsize") => 8.0.into(),
            (Kind::Text, "fontstyle") => "normal".into(),
            (Kind::Text, "fontsmoothing") => true.into(),
            (Kind::Text, "justification") => "left".into(),
            (Kind::Text, "scrollingdelay") => 50.0.into(),
            (Kind::Text, "scrollingamount") => 1.0.into(),
            (Kind::Text, "scrollingdirection") => "left".into(),
            (Kind::Video, "backgroundcolor") => "#000000".into(),
            (Kind::Playlist, "backgroundcolor") => "#000000".into(),
            (Kind::Playlist, "foregroundcolor") => "#C0C0C0".into(),
            (Kind::Playlist, "itemplayingcolor") => "#FFFFFF".into(),
            (Kind::EqualizerSettings, "currentpreset") => ((EQ_PRESETS.len() - 1) as f64).into(),
            (Kind::EqualizerSettings, "enabled" | "enable") => false.into(),
            (Kind::EqualizerSettings, "splinetension" | "speakersize") => 1.0.into(),
            (Kind::EqualizerSettings, n) if n.starts_with("gainlevel") => 0.0.into(),
            (Kind::Popup | Kind::ListBox, "selecteditem") => (-1.0).into(),
            // As WMP drew a LISTBOX on Windows 11 (measured on a GitHub runner).
            (Kind::ListBox, "backgroundcolor") => "#FFFFFF".into(),
            (Kind::ListBox, "foregroundcolor") => "#000000".into(),
            (Kind::ListBox, "fontface") => "Segoe UI".into(),
            (Kind::ListBox, "fontsize") => 9.0.into(),
            (Kind::ListBox, "fontstyle") => "normal".into(),
            (Kind::ListBox, "border") => true.into(),
            _ => Value::Null,
        }
    }

    /// Members computed on the fly rather than stored.
    fn el_computed(&self, v: ViewId, e: usize, name: &str) -> Option<Value> {
        let el = self.el(v, e);
        match (el.kind, name) {
            (Kind::View, "left") => Some(self.view(v).left.into()),
            (Kind::View, "top") => Some(self.view(v).top.into()),
            (Kind::Text, "textwidth") => Some(f64::from(self.text_measure(v, e).0.ceil()).into()),
            (Kind::Text, "textheight") => Some(f64::from(self.text_measure(v, e).1.ceil()).into()),
            (Kind::EqualizerSettings, "presetcount") => Some(EQ_PRESETS.len().into()),
            (Kind::EqualizerSettings, "currentpresettitle") => {
                let p = (self.get_int(v, e, "currentpreset").max(0) as usize).min(EQ_PRESETS.len() - 1);
                Some(EQ_PRESETS[p].0.into())
            }
            (Kind::EqualizerSettings, "currentspeakername") => {
                Some(["Small", "Medium", "Large"][self.get_int(v, e, "speakersize").clamp(0, 2) as usize].into())
            }
            (Kind::Popup | Kind::ListBox, "itemcount") => Some(el.state.items.len().into()),
            (Kind::Playlist, "itemcount") => Some(self.history.len().into()),
            (Kind::Playlist, "playlist") => Some(crate::value::ObjRef::CurrentPlaylist.into()),
            (Kind::Effects | Kind::Video, _) => self.effects_computed(v, e, name),
            _ => None,
        }
    }

    pub fn el_computed_names(&self, kind: Kind) -> &'static [&'static str] {
        match kind {
            Kind::View => &["left", "top"],
            Kind::Text => &["textwidth", "textheight"],
            Kind::EqualizerSettings => &["presetcount", "currentpresettitle", "currentspeakername"],
            Kind::Popup | Kind::ListBox => &["itemcount"],
            Kind::Playlist => &["itemcount", "playlist"],
            Kind::Effects | Kind::Video => {
                &["currenteffecttitle", "currentpresettitle", "effectcount", "presetcount", "effectcangofullscreen", "effecthaspropertypage"]
            }
            _ => &[],
        }
    }

    pub fn el_methods(&self, kind: Kind) -> &'static [&'static str] {
        const COMMON_M: &[&str] = &["moveto", "alphablendto", "resizeto", "setfocus", "removeelement"];
        match kind {
            Kind::View => &[
                "moveto",
                "alphablendto",
                "resizeto",
                "setfocus",
                "removeelement",
                "close",
                "minimize",
                "maximize",
                "restore",
                "returntomediacenter",
                "returntomedia",
                "size",
                "beginmove",
                "settimeout",
            ],
            Kind::Effects | Kind::Video => &[
                "moveto",
                "alphablendto",
                "resizeto",
                "setfocus",
                "removeelement",
                "next",
                "previous",
                "nextpreset",
                "previouspreset",
                "nexteffect",
                "previouseffect",
                "settings",
                "showpropertypage",
                "geteffecttitle",
                "geteffecttype",
                "getpresettitle",
            ],
            Kind::EqualizerSettings => &[
                "moveto",
                "alphablendto",
                "resizeto",
                "setfocus",
                "removeelement",
                "presettitle",
                "gainlevels",
                "reset",
                "nextpreset",
                "previouspreset",
                "nextspeaker",
                "previousspeaker",
                "resetsettings",
            ],
            Kind::VideoSettings => &["moveto", "alphablendto", "resizeto", "setfocus", "removeelement", "reset"],
            Kind::Popup => {
                &["moveto", "alphablendto", "resizeto", "setfocus", "removeelement", "appenditem", "deleteitem", "deleteall", "show", "dismiss"]
            }
            Kind::ListBox => &[
                "moveto",
                "alphablendto",
                "resizeto",
                "setfocus",
                "removeelement",
                "appenditem",
                "insertitem",
                "deleteitem",
                "deleteall",
                "getitem",
                "replaceitem",
                "finditem",
                "getnextselecteditem",
                "setselectedstate",
                "show",
                "dismiss",
            ],
            Kind::AutoMenu => &["moveto", "alphablendto", "resizeto", "setfocus", "removeelement", "show"],
            Kind::Playlist => &[
                "moveto",
                "alphablendto",
                "resizeto",
                "setfocus",
                "removeelement",
                "setcolumnresizemode",
                "setcolumnwidth",
                "getcolumnwidth",
                "sortcolumn",
                "getnextselecteditem",
                "getnextcheckeditem",
                "setcheckedstate",
                "getcheckedstate",
                "deleteselected",
                "moveselectedup",
                "moveselecteddown",
            ],
            _ => COMMON_M,
        }
    }

    // ---- geometry ----------------------------------------------------------------------

    pub fn width(&self, v: ViewId, e: usize) -> i32 {
        self.get_int(v, e, "width")
    }

    pub fn height(&self, v: ViewId, e: usize) -> i32 {
        self.get_int(v, e, "height")
    }

    pub fn visible(&self, v: ViewId, e: usize) -> bool {
        self.get_bool(v, e, "visible")
    }

    pub fn enabled(&self, v: ViewId, e: usize) -> bool {
        self.get_bool(v, e, "enabled") && self.el(v, e).parent.is_none_or(|p| self.enabled(v, p))
    }

    pub fn opacity(&self, v: ViewId, e: usize) -> f32 {
        (self.get_num(v, e, "alphablend").clamp(0.0, 255.0) / 255.0) as f32
    }

    /// Position in view coordinates.
    pub fn absolute(&self, v: ViewId, e: usize) -> (f64, f64) {
        let (mut x, mut y, mut cur) = (0.0, 0.0, Some(e));
        while let Some(i) = cur.filter(|&i| i != 0) {
            x += f64::from(self.get_int(v, i, "left"));
            y += f64::from(self.get_int(v, i, "top"));
            cur = self.el(v, i).parent;
        }
        (x, y)
    }

    pub fn bounds(&self, v: ViewId, e: usize) -> Rect {
        if e == 0 {
            return Rect::new(0.0, 0.0, f64::from(self.width(v, 0).max(0)), f64::from(self.height(v, 0).max(0)));
        }
        let (x, y) = self.absolute(v, e);
        Rect::new(x, y, f64::from(self.width(v, e).max(0)), f64::from(self.height(v, e).max(0)))
    }

    pub fn effectively_visible(&self, v: ViewId, e: usize) -> bool {
        let mut cur = Some(e);
        while let Some(i) = cur {
            if !self.visible(v, i) {
                return false;
            }
            cur = self.el(v, i).parent;
        }
        true
    }

    pub fn children_in_draw_order(&self, v: ViewId, e: usize) -> Vec<usize> {
        let mut c = self.el(v, e).children.clone();
        c.sort_by_key(|&i| (self.get_int(v, i, "zindex"), self.el(v, i).order));
        c
    }

    /// Children drawn under a container's background image (negative zIndex, seen through its
    /// holes as in Goo, Creed and Radio), then those drawn after. They lie over its colour.
    pub fn children_behind_and_above(&self, v: ViewId, e: usize) -> (Vec<usize>, Vec<usize>) {
        if self.el(v, e).kind == Kind::ButtonGroup {
            return (vec![], vec![]);
        }
        let behind_allowed = self.clips_children(v, e);
        self.children_in_draw_order(v, e).into_iter().partition(|&c| behind_allowed && self.get_int(v, c, "zindex") < 0)
    }

    pub fn clips_children(&self, v: ViewId, e: usize) -> bool {
        matches!(self.el(v, e).kind, Kind::View | Kind::Subview)
    }

    // ---- images ------------------------------------------------------------------------

    pub fn img(&self, v: ViewId, e: usize, attr: &str) -> Option<Rc<SkinBitmap>> {
        let name = self.get_str(v, e, attr);
        if name.is_empty() {
            return None;
        }
        let key = parse_color(&self.get_str(v, e, "transparencycolor")).filter(|_| e != 0);
        self.images.get(&name, key, self.get_num(v, e, "hueshift"), self.get_num(v, e, "saturation"), None)
    }

    pub fn raw_img(&self, v: ViewId, e: usize, attr: &str) -> Option<Rc<SkinBitmap>> {
        let name = self.get_str(v, e, attr);
        if name.is_empty() { None } else { self.images.get_raw(&name) }
    }

    /// A subview's background, keyed by its transparencyColor (and its clippingColor, except for a
    /// title-less VIEW, whose clippingColor pixels are keyed out of the composed window instead).
    fn background(&self, v: ViewId, e: usize) -> Option<Rc<SkinBitmap>> {
        self.background_keyed(v, e, false)
    }

    /// The colour a title-less VIEW cuts from its window (its clippingColor).
    pub fn view_cut_color(&self, v: ViewId) -> Option<u32> {
        (!self.get_bool(v, 0, "titlebar") && self.get_str(v, 0, "clippingimage").is_empty())
            .then(|| parse_color(&self.get_str(v, 0, "clippingcolor")))
            .flatten()
    }

    /// `background`, with a title-less VIEW's clippingColor pixels cleared too when `cut_view`
    /// (children behind the view show through them).
    fn background_keyed(&self, v: ViewId, e: usize, cut_view: bool) -> Option<Rc<SkinBitmap>> {
        let name = self.get_str(v, e, "backgroundimage");
        let second = (self.get_str(v, e, "clippingimage").is_empty() && (e != 0 || cut_view || self.get_bool(v, 0, "titlebar")))
            .then(|| parse_color(&self.get_str(v, e, "clippingcolor")))
            .flatten();
        self.images.get(
            &name,
            parse_color(&self.get_str(v, e, "transparencycolor")),
            self.get_num(v, e, "hueshift"),
            self.get_num(v, e, "saturation"),
            second,
        )
    }

    /// The frame of `bmp` to draw now. Animated GIFs run from when this element first showed them,
    /// and the view is asked to redraw when the next frame is due.
    fn frame<'a>(&self, v: ViewId, e: usize, bmp: &'a SkinBitmap, key: &str) -> &'a tiny_skia::Pixmap {
        if !bmp.is_animated() {
            return bmp.source();
        }
        let start = *self.el(v, e).anim_start.borrow_mut().entry(key.to_string()).or_insert(self.now);
        let (frame, next) = bmp.frame_at(self.now.duration_since(start).as_secs_f64() * 1000.0);
        if let Some(ms) = next {
            self.view(v).request_frame(self.now, ms);
        }
        frame
    }

    fn natural_size(&self, v: ViewId, e: usize) -> (i32, i32) {
        let size = |b: Option<Rc<SkinBitmap>>| b.map(|b| (b.width as i32, b.height as i32));
        let el = self.el(v, e);
        match el.kind {
            Kind::View => size(self.background(v, e)).unwrap_or_default(),
            // With no picture it holds its children, as WMP showed them: Catwoman's time digits
            // sit in a subview given only a position. A text sized by its words adds nothing, so
            // the string tables many skins keep in such a subview (WoW, Disney) stay hidden.
            Kind::Subview => size(self.background(v, e)).unwrap_or_else(|| {
                el.children.iter().filter(|&&c| self.el(v, c).kind != Kind::Text || self.has_value(v, c, "width")).fold((0, 0), |(w, h), &c| {
                    (w.max(self.get_int(v, c, "left") + self.width(v, c)), h.max(self.get_int(v, c, "top") + self.height(v, c)))
                })
            }),
            Kind::Button => size(self.img(v, e, "image")).unwrap_or_default(),
            Kind::ButtonGroup => size(self.raw_img(v, e, "mappingimage")).or_else(|| size(self.img(v, e, "image"))).unwrap_or_default(),
            Kind::GroupButton => el.parent.map_or((0, 0), |p| (self.width(v, p), self.height(v, p))),
            Kind::Slider => {
                let (bg, th) = (size(self.img(v, e, "backgroundimage")).unwrap_or_default(), size(self.img(v, e, "thumbimage")).unwrap_or_default());
                (bg.0.max(th.0), bg.1.max(th.1))
            }
            Kind::CustomSlider => size(self.raw_img(v, e, "positionimage")).or_else(|| size(self.img(v, e, "image"))).unwrap_or_default(),
            Kind::Text => {
                let (w, h) = self.text_measure(v, e);
                (w.ceil() as i32 + 1, h.ceil() as i32)
            }
            _ => (0, 0),
        }
    }

    pub fn clip_mask(&self, v: ViewId, e: usize) -> Option<Rc<SkinBitmap>> {
        let img = self.get_str(v, e, "clippingimage");
        let clip = parse_color(&self.get_str(v, e, "clippingcolor"));
        if img.is_empty() {
            let clip = clip?;
            let primary = match self.el(v, e).kind {
                Kind::View | Kind::Subview => self.get_str(v, e, "backgroundimage"),
                _ => self.get_str(v, e, "image"),
            };
            if primary.is_empty() {
                return None;
            }
            // A subview's clippingColor cuts its background image into the subview's shape, which
            // clips its children too; clear pixels stay inside.
            if self.el(v, e).kind == Kind::Subview {
                return self.images.get_raw(&primary)?.shape_of(clip).map(Rc::new);
            }
            return self.images.get(&primary, Some(clip), 0.0, 1.0, None);
        }
        if clip.is_none() {
            // Black-and-white mask with no colour given: white is cut (unless it has real transparency).
            let raw = self.images.get_raw(&img)?;
            let has_alpha = raw.original.iter().any(|p| (p >> 24) < 255);
            return if has_alpha { Some(raw) } else { self.images.get(&img, Some(0xFF_FFFF), 0.0, 1.0, None) };
        }
        self.images.get(&img, clip, 0.0, 1.0, None)
    }

    /// WMP hosted EFFECTS and VIDEO as a window cut to their parent subview's shape. With no
    /// clippingColor that shape is the transparencyColor outline (Plus! Bionic Dot's dome); other
    /// children paint over those pixels. Returns the shape and where its top-left sits.
    pub fn host_mask(&self, v: ViewId, e: usize) -> Option<(Rc<SkinBitmap>, Rect)> {
        let el = self.el(v, e);
        if !matches!(el.kind, Kind::Effects | Kind::Video) {
            return None;
        }
        let p = el.parent?;
        if self.el(v, p).kind != Kind::Subview || self.mask_tiled(v, p) || self.clip_mask(v, p).is_some() {
            return None;
        }
        let key = parse_color(&self.get_str(v, p, "transparencycolor"))?;
        let shape = self.images.get_raw(&self.get_str(v, p, "backgroundimage"))?.shape_of(key)?;
        Some((Rc::new(shape), self.bounds(v, p)))
    }

    pub fn mask_tiled(&self, v: ViewId, e: usize) -> bool {
        matches!(self.el(v, e).kind, Kind::View | Kind::Subview)
            && self.get_bool(v, e, "backgroundtiled")
            && self.get_str(v, e, "clippingimage").is_empty()
    }

    pub fn mask_allows(&self, v: ViewId, e: usize, lx: f64, ly: f64) -> bool {
        if let Some((m, at)) = self.host_mask(v, e) {
            let b = self.bounds(v, e);
            if m.alpha((lx + b.x - at.x).floor() as i32, (ly + b.y - at.y).floor() as i32) == 0 {
                return false;
            }
        }
        let Some(m) = self.clip_mask(v, e) else { return true };
        let (mut x, mut y) = (lx.floor() as i32, ly.floor() as i32);
        if self.mask_tiled(v, e) && m.width > 0 && m.height > 0 {
            x = x.rem_euclid(m.width as i32);
            y = y.rem_euclid(m.height as i32);
        }
        m.alpha(x, y) > 0
    }

    // ---- text --------------------------------------------------------------------------

    fn text_style(&self, v: ViewId, e: usize) -> TextStyle {
        let el = self.el(v, e);
        let pick = |state_attr: &str, cond: bool| cond && !self.get_str(v, e, state_attr).is_empty();
        let attr = if pick("disabledfontstyle", !self.enabled(v, e)) {
            "disabledfontstyle"
        } else if pick("hoverfontstyle", el.hovered) {
            "hoverfontstyle"
        } else {
            "fontstyle"
        };
        TextStyle::new(&self.get_str(v, e, "fontface"), self.get_num(v, e, "fontsize"), &self.get_str(v, e, attr))
    }

    fn text_color(&self, v: ViewId, e: usize) -> u32 {
        let pick = |a: &str, cond: bool| cond && !self.get_str(v, e, a).is_empty();
        let attr = if pick("disabledforegroundcolor", !self.enabled(v, e)) {
            "disabledforegroundcolor"
        } else if pick("hoverforegroundcolor", self.el(v, e).hovered) {
            "hoverforegroundcolor"
        } else {
            "foregroundcolor"
        };
        parse_color(&self.get_str(v, e, attr)).unwrap_or(0xFF_FFFF)
    }

    /// Only an explicit width constrains wrapping (the natural width is derived from this measure).
    fn wrap_width(&self, v: ViewId, e: usize) -> Option<f32> {
        (self.get_bool(v, e, "wordwrap") && self.has_value(v, e, "width")).then(|| self.get_int(v, e, "width") as f32).filter(|w| *w > 0.0)
    }

    pub fn text_measure(&self, v: ViewId, e: usize) -> (f32, f32) {
        text::measure(&self.get_str(v, e, "value"), &self.text_style(v, e), self.wrap_width(v, e))
    }

    pub fn text_overflows(&self, v: ViewId, e: usize) -> bool {
        !self.get_bool(v, e, "wordwrap") && self.text_measure(v, e).0 > self.width(v, e) as f32 + 0.5
    }

    pub fn text_scrolling(&self, v: ViewId, e: usize) -> bool {
        self.get_bool(v, e, "scrolling") && self.text_overflows(v, e) && self.effectively_visible(v, e)
    }

    // ---- interaction -------------------------------------------------------------------

    pub fn has_handler(&self, v: ViewId, e: usize, name: &str) -> bool {
        let el = self.el(v, e);
        el.handlers.contains_key(name) || matches!(el.values.get(name), Some(Value::Js(f)) if f.is_function)
    }

    /// Whether a click here goes to the element rather than dragging the window.
    pub fn interactive(&self, v: ViewId, e: usize) -> bool {
        match self.el(v, e).kind {
            Kind::Button | Kind::ButtonGroup | Kind::GroupButton | Kind::Slider | Kind::CustomSlider | Kind::Playlist | Kind::ListBox => true,
            Kind::Effects | Kind::Video => ["onclick", "ondblclick", "onmousedown"].iter().any(|h| self.has_handler(v, e, h)),
            _ => ["onclick", "onmousedown", "onmouseup", "ondblclick"].iter().any(|h| self.has_handler(v, e, h)),
        }
    }

    /// Whether clicks pass through `e` to what lies beneath: it has passThrough, or the subview
    /// holding it does. Measured in WMP: a subview's passThrough reaches its buttons, text and
    /// visualizers (even one set passThrough="false") but not the subviews inside it.
    pub fn passes_clicks(&self, v: ViewId, e: usize) -> bool {
        self.get_bool(v, e, "passthrough")
            || (!matches!(self.el(v, e).kind, Kind::View | Kind::Subview)
                && std::iter::successors(self.el(v, e).parent, |&p| self.el(v, p).parent)
                    .find(|&p| matches!(self.el(v, p).kind, Kind::View | Kind::Subview))
                    .is_some_and(|p| self.get_bool(v, p, "passthrough")))
    }

    /// Whether an element that draws nothing to click does something with the mouse anyway.
    fn uses_mouse(&self, v: ViewId, e: usize) -> bool {
        self.tooltip(v, e).is_some()
            || ["onclick", "onmousedown", "onmouseup", "ondblclick", "onmouseover", "onmouseout", "onmousemove"]
                .iter()
                .any(|h| self.has_handler(v, e, h))
    }

    /// Local hit test (point relative to the element's origin).
    pub fn hit_test(&self, v: ViewId, e: usize, x: f64, y: f64) -> bool {
        let inside = x >= 0.0 && y >= 0.0 && x < f64::from(self.width(v, e)) && y < f64::from(self.height(v, e));
        if !inside {
            return false;
        }
        let (ix, iy) = (x.floor() as i32, y.floor() as i32);
        match self.el(v, e).kind {
            Kind::NonVisual | Kind::EqualizerSettings | Kind::VideoSettings | Kind::Popup | Kind::AutoMenu => false,
            Kind::Button => self.img(v, e, "image").is_none_or(|i| i.alpha(ix, iy) > 0),
            Kind::ButtonGroup => self.group_element_at(v, e, x, y).is_some(),
            Kind::CustomSlider => self.custom_slider_value(v, e, x, y).is_some(),
            // Text that does nothing with the mouse lets clicks through: Blue Crush's title box
            // overlaps its close button.
            Kind::Text => self.uses_mouse(v, e),
            Kind::View | Kind::Subview => {
                if !self.mask_allows(v, e, x, y) {
                    return false;
                }
                // A transparent background with no colour isn't hit (clicks reach the window behind).
                let bg = self.background(v, e);
                let has_color = parse_color(&self.get_str(v, e, "backgroundcolor")).is_some();
                // Nor is one with nothing to draw: skins group controls in window-sized empty
                // subviews drawn above other controls (Constantine, Media Center), which WMP let
                // clicks pass.
                match bg {
                    Some(bg) if !has_color => self.background_alpha(v, e, &bg, x, y) > 0,
                    None if !has_color => e == 0 || self.uses_mouse(v, e),
                    _ => true,
                }
            }
            _ => true,
        }
    }

    /// The BUTTONGROUP element under a group-local point, by mapping colour.
    pub fn group_element_at(&self, v: ViewId, g: usize, x: f64, y: f64) -> Option<usize> {
        let map = self.raw_img(v, g, "mappingimage")?;
        let rgb = map.rgb(x.floor() as i32, y.floor() as i32)?;
        self.el(v, g)
            .children
            .iter()
            .copied()
            .find(|&c| self.el(v, c).kind == Kind::GroupButton && self.visible(v, c) && parse_color(&self.get_str(v, c, "mappingcolor")) == Some(rgb))
    }

    pub fn tooltip(&self, v: ViewId, e: usize) -> Option<String> {
        let non_empty = |s: String| (!s.is_empty()).then_some(s);
        let base = non_empty(self.get_str(v, e, "tooltip")).or_else(|| non_empty(self.get_str(v, e, "uptooltip")));
        match self.el(v, e).kind {
            Kind::Button | Kind::GroupButton if self.get_bool(v, e, "down") => non_empty(self.get_str(v, e, "downtooltip")).or(base),
            _ => base,
        }
    }

    // ---- sliders -----------------------------------------------------------------------

    pub fn slider_vertical(&self, v: ViewId, e: usize) -> bool {
        self.get_str(v, e, "direction").to_lowercase().starts_with('v')
    }

    fn slider_thumb(&self, v: ViewId, e: usize) -> Option<Rc<SkinBitmap>> {
        let el = self.el(v, e);
        if !self.enabled(v, e) {
            return self.img(v, e, "thumbdisabledimage").or_else(|| self.img(v, e, "thumbimage"));
        }
        if el.state.dragging || (el.pressed && el.hovered) {
            return self.img(v, e, "thumbdownimage").or_else(|| self.img(v, e, "thumbhoverimage")).or_else(|| self.img(v, e, "thumbimage"));
        }
        if el.hovered {
            return self.img(v, e, "thumbhoverimage").or_else(|| self.img(v, e, "thumbimage"));
        }
        self.img(v, e, "thumbimage")
    }

    /// Thumb centre along the track for a value (pixels from the start).
    fn slider_center(&self, v: ViewId, e: usize, value: f64) -> f64 {
        let (min, max) = (self.get_num(v, e, "min"), self.get_num(v, e, "max"));
        let b = f64::from(self.get_int(v, e, "bordersize").max(0));
        let len = f64::from(if self.slider_vertical(v, e) { self.height(v, e) } else { self.width(v, e) });
        let range = max - min;
        let mut r = if range == 0.0 { 0.0 } else { ((value - min) / range).clamp(0.0, 1.0) };
        if self.slider_vertical(v, e) {
            r = 1.0 - r; // min at the bottom
        }
        b + r * (len - 2.0 * b).max(0.0)
    }

    pub fn slider_value_at(&self, v: ViewId, e: usize, pos: f64) -> f64 {
        let (min, max) = (self.get_num(v, e, "min"), self.get_num(v, e, "max"));
        let b = f64::from(self.get_int(v, e, "bordersize").max(0));
        let len = f64::from(if self.slider_vertical(v, e) { self.height(v, e) } else { self.width(v, e) });
        let mut r = if len - 2.0 * b <= 0.0 { 0.0 } else { ((pos - b) / (len - 2.0 * b)).clamp(0.0, 1.0) };
        if self.slider_vertical(v, e) {
            r = 1.0 - r;
        }
        min + r * (max - min)
    }

    /// The thumb's rectangle relative to the slider's origin.
    pub fn slider_thumb_rect(&self, v: ViewId, e: usize) -> Option<Rect> {
        let t = self.slider_thumb(v, e)?;
        let (w, h) = (f64::from(self.width(v, e)), f64::from(self.height(v, e)));
        let (tw, th) = (f64::from(t.width), f64::from(t.height));
        let c = self.slider_center(v, e, self.get_num(v, e, "value"));
        Some(if self.slider_vertical(v, e) {
            Rect::new((w - tw) / 2.0, (c - th / 2.0).clamp(0.0, (h - th).max(0.0)), tw, th)
        } else {
            Rect::new((c - tw / 2.0).clamp(0.0, (w - tw).max(0.0)), (h - th) / 2.0, tw, th)
        })
    }

    /// CUSTOMSLIDER: the value under a local point, from the grey level of positionImage.
    pub fn custom_slider_value(&self, v: ViewId, e: usize, x: f64, y: f64) -> Option<f64> {
        let map = self.raw_img(v, e, "positionimage")?;
        let rgb = map.rgb(x.floor() as i32, y.floor() as i32)?;
        if parse_color(&self.get_str(v, e, "transparencycolor")) == Some(rgb) {
            return None;
        }
        let gray = ((rgb >> 16) & 255) * 30 / 100 + ((rgb >> 8) & 255) * 59 / 100 + (rgb & 255) * 11 / 100;
        let (min, max) = (self.get_num(v, e, "min"), self.get_num(v, e, "max"));
        Some(min + f64::from(gray) / 255.0 * (max - min))
    }

    // ---- drawing -----------------------------------------------------------------------

    /// Draws one element at `b` (its absolute bounds), inside `clip`; children are drawn by the caller.
    pub fn render_element(&self, v: ViewId, e: usize, c: &mut Canvas, b: Rect, clip: Rect) {
        let el = self.el(v, e);
        match el.kind {
            Kind::View | Kind::Subview => {
                self.render_subview_fill(v, e, c, b, clip);
                self.render_subview_image(v, e, c, b, clip, false);
            }
            Kind::Button => self.render_button(v, e, c, b, clip),
            Kind::ButtonGroup => self.render_group(v, e, c, b, clip),
            Kind::Slider => self.render_slider(v, e, c, b, clip),
            Kind::CustomSlider => self.render_custom_slider(v, e, c, b, clip),
            Kind::Text => self.render_text(v, e, c, b, clip),
            Kind::Effects => self.render_effects(v, e, c, b, clip),
            Kind::Video => {
                // WMP showed video in a window of its own, never keyed with the skin.
                c.cover_holes(b.intersect(&clip));
                if let Some(color) = parse_color(&self.get_str(v, e, "backgroundcolor")) {
                    c.fill(b, color, clip);
                }
                if !self.has_effects {
                    self.render_effects(v, e, c, b, clip);
                }
            }
            Kind::Playlist => self.render_playlist(v, e, c, b, clip),
            Kind::ListBox => self.render_listbox(v, e, c, b, clip),
            _ => {}
        }
    }

    /// A subview's backgroundColor, which lies under any children behind its image.
    pub fn render_subview_fill(&self, v: ViewId, e: usize, c: &mut Canvas, b: Rect, clip: Rect) {
        let mut color = parse_color(&self.get_str(v, e, "backgroundcolor"));
        if color.is_none() {
            // A common typo WMP tolerated.
            color = self.el(v, e).values.get("backgoundcolor").and_then(|t| parse_color(&t.to_str()));
        }
        if let Some(color) = color {
            c.fill(b, color, clip);
            c.cover_holes(b.intersect(&clip));
        }
    }

    /// Whether a subview's background image has a visible pixel at local (x, y): there it covers
    /// the children behind it.
    pub fn background_covers(&self, v: ViewId, e: usize, x: f64, y: f64) -> bool {
        let Some(bg) = self.background_keyed(v, e, e == 0) else { return false };
        self.background_alpha(v, e, &bg, x, y) > 0
    }

    /// The alpha of background `bg` at local (x, y) in the frame showing now (anime's animated
    /// background covers its sliders only in some frames).
    fn background_alpha(&self, v: ViewId, e: usize, bg: &SkinBitmap, x: f64, y: f64) -> u8 {
        let (bx, by) = self.background_pixel(v, e, bg)(x, y);
        let shown = match self.el(v, e).anim_start.borrow().get(&self.get_str(v, e, "backgroundimage")) {
            Some(start) if bg.is_animated() => bg.frame_at(self.now.duration_since(*start).as_secs_f64() * 1000.0).0,
            _ => bg.source(),
        };
        pixel_at(shown, bx, by).map_or(0, |p| p.alpha())
    }

    fn background_stretched(&self, v: ViewId, e: usize) -> bool {
        self.get_bool(v, e, "resizebackgroundimage")
            || self.get_bool(v, e, "resizeimages")
            || images::is_dynamic(&self.get_str(v, e, "backgroundimage"))
    }

    /// Maps a subview's local point (x, y) to the pixel of its background image `bg` drawn there:
    /// stretched over it, tiled, or once at its top-left. The settings are read once, so it suits a
    /// loop over pixels.
    pub fn background_pixel(&self, v: ViewId, e: usize, bg: &SkinBitmap) -> impl Fn(f64, f64) -> (i32, i32) + use<> {
        let (w, h) = (f64::from(self.width(v, e)), f64::from(self.height(v, e)));
        let (bw, bh) = (bg.width, bg.height);
        let stretched = self.background_stretched(v, e) && w > 0.0 && h > 0.0;
        let tiled = self.get_bool(v, e, "backgroundtiled") && bw > 0 && bh > 0;
        move |x, y| {
            if stretched {
                return ((x * f64::from(bw) / w).floor() as i32, (y * f64::from(bh) / h).floor() as i32);
            }
            let (ix, iy) = (x.floor() as i32, y.floor() as i32);
            if tiled { (ix.rem_euclid(bw as i32), iy.rem_euclid(bh as i32)) } else { (ix, iy) }
        }
    }

    pub fn render_subview_image(&self, v: ViewId, e: usize, c: &mut Canvas, b: Rect, clip: Rect, cut_view: bool) {
        let Some(bg) = self.background_keyed(v, e, cut_view) else { return };
        let name = self.get_str(v, e, "backgroundimage");
        let frame = self.frame(v, e, &bg, &name);
        let (bw, bh) = (f64::from(bg.width), f64::from(bg.height));
        if self.background_stretched(v, e) {
            c.draw_image(frame, b, None, clip, 1.0);
        } else if self.get_bool(v, e, "backgroundtiled") && bw > 0.0 && bh > 0.0 {
            let mut y = b.y;
            while y < b.bottom() {
                let mut x = b.x;
                while x < b.right() {
                    c.draw_image(frame, Rect::new(x, y, bw, bh), None, clip.intersect(&b), 1.0);
                    x += bw;
                }
                y += bh;
            }
        } else {
            c.draw_image(frame, Rect::new(b.x, b.y, bw, bh), None, clip, 1.0);
        }
        self.mark_holes(v, e, c, &bg, b, clip);
    }

    /// Records where a subview's transparencyColor leaves the window open and where its picture
    /// covers it; a button or slider drawn over the open part is cut later. A VIEW's own keys
    /// already cut the whole window, so its picture only covers (Radio's frame over the tray
    /// behind it).
    fn mark_holes(&self, v: ViewId, e: usize, c: &mut Canvas, bg: &SkinBitmap, b: Rect, clip: Rect) {
        let key = parse_color(&self.get_str(v, e, "transparencycolor")).filter(|_| e != 0);
        let raw = key.and_then(|_| self.images.get_raw(&self.get_str(v, e, "backgroundimage")));
        let at = self.background_pixel(v, e, bg);
        c.mark_holes(b.intersect(&clip), |x, y| {
            let (ix, iy) = at(x - b.x, y - b.y);
            match (key, &raw) {
                (Some(k), Some(raw)) if raw.rgb(ix, iy).is_some_and(|rgb| images::matches_key(rgb, k)) => Backdrop::Open(k),
                _ if bg.alpha(ix, iy) > 0 => Backdrop::Covered,
                _ => Backdrop::Outside,
            }
        });
    }

    fn button_image(&self, v: ViewId, e: usize) -> Option<Rc<SkinBitmap>> {
        let el = self.el(v, e);
        let image = || self.img(v, e, "image");
        if !self.enabled(v, e) {
            return self.img(v, e, "disabledimage").or_else(image);
        }
        let pressed = (el.pressed && el.hovered) || self.get_bool(v, e, "down");
        if pressed && el.hovered {
            return self.img(v, e, "hoverdownimage").or_else(|| self.img(v, e, "downimage")).or_else(|| self.img(v, e, "hoverimage")).or_else(image);
        }
        if pressed {
            return self.img(v, e, "downimage").or_else(image);
        }
        if el.hovered {
            return self.img(v, e, "hoverimage").or_else(image);
        }
        image()
    }

    fn render_button(&self, v: ViewId, e: usize, c: &mut Canvas, b: Rect, clip: Rect) {
        let Some(img) = self.button_image(v, e) else { return };
        // Album art is drawn to fit the button; skin bitmaps at their own size.
        let fit = images::is_dynamic(&self.get_str(v, e, "image")) && self.has_value(v, e, "width");
        let key = format!("{:p}", Rc::as_ptr(&img));
        let frame = self.frame(v, e, &img, &key);
        let dst = if fit { b } else { Rect::new(b.x, b.y, f64::from(img.width), f64::from(img.height)) };
        c.draw_image(frame, dst, None, clip.intersect(&b), 1.0);
    }

    fn render_group(&self, v: ViewId, g: usize, c: &mut Canvas, b: Rect, clip: Rect) {
        let background = self.get_bool(v, g, "showbackground");
        let Some(map) = self.raw_img(v, g, "mappingimage") else { return };
        if background && let Some(img) = self.img(v, g, "image") {
            c.draw_image(img.source(), Rect::new(b.x, b.y, f64::from(img.width), f64::from(img.height)), None, clip, 1.0);
        }
        let has = |attr: &str| self.img(v, g, attr).is_some();
        for &child in &self.el(v, g).children {
            let ch = self.el(v, child);
            let Some(color) = parse_color(&self.get_str(v, child, "mappingcolor")) else { continue };
            if ch.kind != Kind::GroupButton || !self.visible(v, child) {
                continue;
            }
            let attr = if !self.enabled(v, child) {
                if has("disabledimage") { "disabledimage" } else { "image" }
            } else {
                let pressed = (ch.pressed && ch.hovered) || self.get_bool(v, child, "down");
                if pressed && ch.hovered {
                    ["hoverdownimage", "downimage"].into_iter().find(|a| has(a)).unwrap_or("image")
                } else if pressed {
                    if has("downimage") { "downimage" } else { "image" }
                } else if ch.hovered && has("hoverimage") {
                    "hoverimage"
                } else {
                    "image"
                }
            };
            if attr == "image" && background {
                continue;
            }
            if let Some(m) = self.img(v, g, attr).and_then(|img| img.masked_by(&map, color)) {
                c.draw_image(m.source(), Rect::new(b.x, b.y, f64::from(m.width), f64::from(m.height)), None, clip, 1.0);
            }
        }
    }

    fn render_slider(&self, v: ViewId, e: usize, c: &mut Canvas, b: Rect, clip: Rect) {
        let clip = clip.intersect(&b);
        let el = self.el(v, e);
        let bg = (!self.enabled(v, e))
            .then(|| self.img(v, e, "disabledimage"))
            .flatten()
            .or_else(|| el.hovered.then(|| self.img(v, e, "backgroundhoverimage")).flatten())
            .or_else(|| self.img(v, e, "backgroundimage"));
        let border = self.get_int(v, e, "bordersize").max(0);
        let tiled = self.get_bool(v, e, "tiled");
        let vertical = self.slider_vertical(v, e);
        if let Some(bg) = bg {
            draw_sliced(c, &bg, b, border, tiled, vertical, clip);
        }
        let fg = el.hovered.then(|| self.img(v, e, "foregroundhoverimage")).flatten().or_else(|| self.img(v, e, "foregroundimage"));
        if let Some(fg) = fg {
            let value =
                if self.get_bool(v, e, "useforegroundprogress") { self.get_num(v, e, "foregroundprogress") } else { self.get_num(v, e, "value") };
            let end = self.slider_center(v, e, value);
            let fill = if vertical { Rect::new(b.x, b.y + end, b.w, (b.h - end).max(0.0)) } else { Rect::new(b.x, b.y, end.max(0.0), b.h) };
            if !fill.is_empty() {
                let narrow = if vertical { f64::from(fg.height) < b.h / 2.0 } else { f64::from(fg.width) < b.w / 2.0 };
                draw_sliced(c, &fg, b, border, tiled || narrow, vertical, clip.intersect(&fill));
            }
        }
        if let (Some(t), Some(r)) = (self.slider_thumb(v, e), self.slider_thumb_rect(v, e)) {
            c.draw_image(t.source(), Rect::new(b.x + r.x, b.y + r.y, r.w, r.h), None, clip, 1.0);
        }
    }

    fn render_custom_slider(&self, v: ViewId, e: usize, c: &mut Canvas, b: Rect, clip: Rect) {
        let Some(strip) = self.img(v, e, "image") else { return };
        let (w, h) = (self.width(v, e), self.height(v, e));
        let fw = if w > 0 { w as u32 } else { strip.width }.max(1);
        let fh = if h > 0 { h as u32 } else { strip.height }.max(1);
        // Frames run left to right, or top to bottom when the strip is one frame wide
        // (Windows XP Media Center Edition's seek bar).
        let vertical = strip.width / fw <= 1 && strip.height / fh > 1;
        let (len, step) = if vertical { (strip.height, fh) } else { (strip.width, fw) };
        let frames = (len / step).max(1);
        let (min, max) = (self.get_num(v, e, "min"), self.get_num(v, e, "max"));
        let r = if max - min == 0.0 { 0.0 } else { ((self.get_num(v, e, "value") - min) / (max - min)).clamp(0.0, 1.0) };
        let f = (r * f64::from(frames - 1)).round() as u32;
        let at = (f * step).min(len.saturating_sub(step));
        let size = step.min(len - at);
        let src = if vertical {
            Rect::new(0.0, f64::from(at), f64::from(strip.width), f64::from(size))
        } else {
            Rect::new(f64::from(at), 0.0, f64::from(size), f64::from(strip.height))
        };
        c.draw_image(strip.source(), Rect::new(b.x, b.y, src.w, src.h), Some(src), clip, 1.0);
    }

    fn render_text(&self, v: ViewId, e: usize, c: &mut Canvas, b: Rect, clip: Rect) {
        let el = self.el(v, e);
        let bg_attr = if el.hovered && !self.get_str(v, e, "hoverbackgroundcolor").is_empty() { "hoverbackgroundcolor" } else { "backgroundcolor" };
        if let Some(bg) = parse_color(&self.get_str(v, e, bg_attr)) {
            c.fill(b, bg, clip);
        }
        let value = self.get_str(v, e, "value");
        if value.is_empty() {
            return;
        }
        let style = self.text_style(v, e);
        let color = self.text_color(v, e);
        let clip = clip.intersect(&b);
        let wordwrap = self.get_bool(v, e, "wordwrap");
        let (tw, th) = self.text_measure(v, e);
        let (tw, th) = (f64::from(tw), f64::from(th));
        let y = if wordwrap { b.y } else { b.y + ((b.h - th) / 2.0).max(0.0) };
        let justification = self.get_str(v, e, "justification").to_lowercase();
        if self.text_scrolling(v, e) {
            let cov = text::rasterize(&value, &style, c.scale, None);
            let span = tw + 24.0;
            let right = self.get_str(v, e, "scrollingdirection").eq_ignore_ascii_case("right");
            let scroll = el.state.scroll;
            let x = if right { b.x - span + scroll } else { b.x - scroll };
            for k in 0..if right { 3 } else { 2 } {
                c.draw_text(&cov, x + span * f64::from(k), y, color, clip);
            }
        } else if wordwrap {
            let align = match justification.as_str() {
                "center" => Align::Center,
                "right" => Align::Right,
                _ => Align::Left,
            };
            let width = self.wrap_width(v, e).unwrap_or(b.w as f32);
            let cov = text::rasterize(&value, &style, c.scale, Some((width, align)));
            c.draw_text(&cov, b.x, y, color, clip);
        } else {
            let truncated = self.get_bool(v, e, "truncate") && tw > b.w;
            let shown = if truncated { text::ellipsize(&value, &style, b.w as f32) } else { value };
            let w = if truncated { f64::from(text::measure(&shown, &style, None).0) } else { tw };
            let x = match justification.as_str() {
                _ if truncated => b.x,
                "center" => b.x + (b.w - w) / 2.0,
                "right" => b.right() - w,
                _ => b.x,
            };
            let cov = text::rasterize(&shown, &style, c.scale, None);
            c.draw_text(&cov, x, y, color, clip);
        }
    }

    fn render_effects(&self, v: ViewId, e: usize, c: &mut Canvas, b: Rect, clip: Rect) {
        if b.w < 1.0 || b.h < 1.0 {
            return;
        }
        // A visualization ran in a window of its own, never keyed with the skin.
        c.cover_holes(b.intersect(&clip));
        match self.el(v, e).state.vis.as_ref().and_then(|r| r.image.as_ref()) {
            Some(image) => c.draw_image(image, b, None, clip, 1.0),
            None => c.fill(b, 0, clip),
        }
    }

    fn render_playlist(&self, v: ViewId, e: usize, c: &mut Canvas, b: Rect, clip: Rect) {
        if b.w < 4.0 || b.h < 4.0 {
            return;
        }
        let clip = clip.intersect(&b);
        let color = |attr: &str, fallback: u32| parse_color(&self.get_str(v, e, attr)).unwrap_or(fallback);
        if parse_color(&self.get_str(v, e, "backgroundcolor")).is_some() {
            c.fill(b, color("backgroundcolor", 0), clip);
        }
        if let Some(bg) = self.img(v, e, "backgroundimage") {
            c.draw_image(bg.source(), Rect::new(b.x, b.y, f64::from(bg.width), f64::from(bg.height)), None, clip, 1.0);
        }
        let face = self.get_str(v, e, "fontface");
        let points = self.get_num(v, e, "fontsize").max(6.0);
        let normal = TextStyle::new(&face, points, "normal");
        let bold = TextStyle::new(&face, points, "bold");
        let row_h = (f64::from(normal.px) * 1.45).ceil();
        let (fg, playing) = (color("foregroundcolor", 0xC0C0C0), color("itemplayingcolor", 0xFF_FFFF));
        let items = &self.history;
        let rows = (b.h / row_h) as usize;
        let top = self.el(v, e).state.playlist_top.min(items.len().saturating_sub(rows));
        let mut y = b.y + 2.0;
        for (i, it) in items.iter().enumerate().skip(top) {
            if y >= b.bottom() {
                break;
            }
            let current = i == items.len() - 1;
            if current && let Some(hb) = parse_color(&self.get_str(v, e, "itemplayingbackgroundcolor")) {
                c.fill(Rect::new(b.x, y - 1.0, b.w, row_h), hb, clip);
            }
            let (style, rgb) = if current { (&bold, playing) } else { (&normal, fg) };
            let name = if it.artist.is_empty() { it.title.clone() } else { format!("{} - {}", it.title, it.artist) };
            let label = text::ellipsize(&format!("{}. {name}", i + 1), style, (b.w - 52.0).max(1.0) as f32);
            c.draw_text(&text::rasterize(&label, style, c.scale, None), b.x + 4.0, y, rgb, clip);
            if it.duration > 0.0 {
                let dur = crate::player::format_time(it.duration);
                let dw = f64::from(text::measure(&dur, style, None).0);
                c.draw_text(&text::rasterize(&dur, style, c.scale, None), b.right() - dw - 4.0, y, rgb, clip);
            }
            y += row_h;
        }
        if items.is_empty() {
            let hint = text::ellipsize("Play something in Spotify, Music or your browser", &normal, (b.w - 8.0).max(1.0) as f32);
            c.draw_text(&text::rasterize(&hint, &normal, c.scale, None), b.x + 4.0, b.y + 4.0, fg, clip);
        }
    }

    /// A LISTBOX's text style, row height and inset: WMP drew it as a Windows list control, rows
    /// about 1.3 times the font's pixel size, inside a one-pixel border when it has one.
    pub fn listbox_layout(&self, v: ViewId, e: usize) -> (TextStyle, f64, f64) {
        let style = TextStyle::new(&self.get_str(v, e, "fontface"), self.get_num(v, e, "fontsize").max(6.0), &self.get_str(v, e, "fontstyle"));
        let row = (f64::from(style.px) * 1.3).ceil();
        let inset = if self.get_bool(v, e, "border") { 1.0 } else { 0.0 };
        (style, row, inset)
    }

    /// The LISTBOX line under local (x, y), if any.
    pub fn listbox_item_at(&self, v: ViewId, e: usize, y: f64) -> Option<usize> {
        let (_, row, inset) = self.listbox_layout(v, e);
        let first = self.get_int(v, e, "firstvisibleitem").max(0) as usize;
        let i = first + ((y - inset) / row).floor().max(0.0) as usize;
        (i < self.el(v, e).state.items.len()).then_some(i)
    }

    fn render_listbox(&self, v: ViewId, e: usize, c: &mut Canvas, b: Rect, clip: Rect) {
        let clip = clip.intersect(&b);
        let (style, row, inset) = self.listbox_layout(v, e);
        if inset > 0.0 {
            c.fill(b, 0x7A_7A7A, clip);
        }
        let inner = Rect::new(b.x + inset, b.y + inset, b.w - 2.0 * inset, b.h - 2.0 * inset);
        let clip = clip.intersect(&inner);
        if let Some(bg) = parse_color(&self.get_str(v, e, "backgroundcolor")) {
            c.fill(inner, bg, clip);
        }
        let fg = parse_color(&self.get_str(v, e, "foregroundcolor")).unwrap_or(0);
        let selected = self.get_int(v, e, "selecteditem");
        let first = self.get_int(v, e, "firstvisibleitem").max(0) as usize;
        let mut y = inner.y;
        for (i, line) in self.el(v, e).state.items.iter().enumerate().skip(first) {
            if y >= inner.bottom() {
                break;
            }
            let rgb = if i as i32 == selected {
                c.fill(Rect::new(inner.x, y, inner.w, row), 0x00_78D4, clip);
                0xFF_FFFF
            } else {
                fg
            };
            let label = text::ellipsize(line, &style, (inner.w - 4.0).max(1.0) as f32);
            c.draw_text(&text::rasterize(&label, &style, c.scale, None), inner.x + 2.0, y + 1.0, rgb, clip);
            y += row;
        }
    }
}

/// Draws an image across `b` keeping `border`-pixel end caps and filling the middle (stretched or tiled).
fn draw_sliced(c: &mut Canvas, img: &SkinBitmap, b: Rect, border: i32, tiled: bool, vertical: bool, clip: Rect) {
    let len = if vertical { b.h } else { b.w };
    let img_len = f64::from(if vertical { img.height } else { img.width });
    let cross = f64::from(if vertical { img.width } else { img.height });
    let cross_off = (if vertical { b.w - cross } else { b.h - cross } / 2.0).max(0.0);
    let put = |c: &mut Canvas, src_start: f64, src_len: f64, dst_start: f64, dst_len: f64| {
        if src_len <= 0.0 || dst_len <= 0.0 {
            return;
        }
        let src_len = src_len.min(img_len - src_start);
        let (src, dst) = if vertical {
            (Rect::new(0.0, src_start, cross, src_len), Rect::new(b.x + cross_off, b.y + dst_start, cross, dst_len))
        } else {
            (Rect::new(src_start, 0.0, src_len, cross), Rect::new(b.x + dst_start, b.y + cross_off, dst_len, cross))
        };
        c.draw_image(img.source(), dst, Some(src), clip, 1.0);
    };
    if (img_len - len).abs() < 0.5 || (border == 0 && !tiled && img_len >= len) {
        put(c, 0.0, img_len, 0.0, img_len);
        return;
    }
    let cap = f64::from(border).min((img_len / 2.0).floor());
    let mid = img_len - 2.0 * cap;
    if cap > 0.0 {
        put(c, 0.0, cap, 0.0, cap);
        put(c, img_len - cap, cap, len - cap, cap);
    }
    let mid_len = len - 2.0 * cap;
    if mid <= 0.0 || mid_len <= 0.0 {
        return;
    }
    if tiled {
        let mut p = 0.0;
        while p < mid_len {
            let step = mid.min(mid_len - p);
            put(c, cap, step.ceil(), cap + p, step);
            p += mid;
        }
    } else {
        put(c, cap, mid, cap, mid_len);
    }
}
