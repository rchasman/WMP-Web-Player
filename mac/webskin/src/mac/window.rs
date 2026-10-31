//! A borderless, see-through window per skin view. Its layer shows the rendered view, so clicks
//! pass through where the skin is transparent, as they did on Windows.

use super::current;
use crate::skin::{AppAction, MouseButton, MouseResult, ViewId};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{AllocAnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSApplication, NSBackingStoreType, NSColor, NSCursor, NSEvent, NSEventMask, NSEventModifierFlags, NSResponder, NSTrackingArea,
    NSTrackingAreaOptions, NSView, NSWindow, NSWindowStyleMask,
};
use objc2_core_foundation::CFRetained;
use objc2_core_graphics::{CGColorSpace, CGImage, CGImageAlphaInfo, CGImageByteOrderInfo};
use objc2_foundation::{NSObject, NSPoint, NSRect, NSSize, NSString};
use objc2_quartz_core::{kCAFilterNearest, kCAGravityTopLeft};
use std::cell::{Cell, RefCell};

pub struct SurfaceIvars {
    view: Cell<ViewId>,
}

define_class!(
    #[unsafe(super(NSWindow, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "WebSkinWindow"]
    pub struct SkinWindow;

    impl SkinWindow {
        // Borderless windows don't take the keyboard unless they say they can.
        #[unsafe(method(canBecomeKeyWindow))]
        fn can_become_key(&self) -> bool {
            true
        }

        #[unsafe(method(canBecomeMainWindow))]
        fn can_become_main(&self) -> bool {
            true
        }
    }
);

define_class!(
    #[unsafe(super(NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "WebSkinSurface"]
    #[ivars = SurfaceIvars]
    pub struct Surface;

    impl Surface {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool {
            true
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(updateTrackingAreas))]
        fn update_tracking_areas(&self) {
            for area in self.trackingAreas().iter() {
                self.removeTrackingArea(&area);
            }
            let options = NSTrackingAreaOptions::MouseMoved
                | NSTrackingAreaOptions::MouseEnteredAndExited
                | NSTrackingAreaOptions::ActiveAlways
                | NSTrackingAreaOptions::InVisibleRect;
            let area = unsafe {
                NSTrackingArea::initWithRect_options_owner_userInfo(NSTrackingArea::alloc(), self.bounds(), options, Some(self), None)
            };
            self.addTrackingArea(&area);
            let _: () = unsafe { msg_send![super(self), updateTrackingAreas] };
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            self.press(event, MouseButton::Left);
        }

        #[unsafe(method(rightMouseDown:))]
        fn right_mouse_down(&self, event: &NSEvent) {
            self.press(event, MouseButton::Right);
        }

        #[unsafe(method(otherMouseDown:))]
        fn other_mouse_down(&self, event: &NSEvent) {
            self.press(event, MouseButton::Middle);
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            self.release(event, MouseButton::Left);
        }

        #[unsafe(method(rightMouseUp:))]
        fn right_mouse_up(&self, event: &NSEvent) {
            self.release(event, MouseButton::Right);
        }

        #[unsafe(method(mouseMoved:))]
        fn mouse_moved(&self, event: &NSEvent) {
            self.motion(event);
        }

        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            self.motion(event);
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            let v = self.ivars().view.get();
            if let Some(shared) = current() {
                shared.with(|s| {
                    if s.views.get(v).is_some_and(Option::is_some) {
                        s.mouse_leave(v);
                    }
                });
            }
        }

        #[unsafe(method(scrollWheel:))]
        fn scroll_wheel(&self, event: &NSEvent) {
            let delta = event.scrollingDeltaY();
            if delta.abs() < 0.5 {
                return;
            }
            let (x, y) = self.point(event);
            let v = self.ivars().view.get();
            if let Some(shared) = current() {
                shared.with(|s| {
                    if s.views.get(v).is_some_and(Option::is_some) {
                        s.mouse_wheel(v, x, y, delta);
                    }
                });
            }
        }

        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            let v = self.ivars().view.get();
            let handled = virtual_key(event).is_some_and(|vk| {
                current().is_some_and(|shared| shared.with(|s| s.views.get(v).is_some_and(Option::is_some) && s.key_down(v, vk, modifiers(event))))
            });
            if !handled {
                let _: () = unsafe { msg_send![super(self), keyDown: event] };
            }
        }

        #[unsafe(method(keyUp:))]
        fn key_up(&self, event: &NSEvent) {
            let v = self.ivars().view.get();
            let Some(vk) = virtual_key(event) else { return };
            if let Some(shared) = current() {
                shared.with(|s| {
                    if s.views.get(v).is_some_and(Option::is_some) {
                        s.key_up(v, vk, modifiers(event));
                    }
                });
            }
        }
    }
);

fn modifiers(event: &NSEvent) -> (bool, bool, bool) {
    let f = event.modifierFlags();
    (
        f.contains(NSEventModifierFlags::Shift),
        f.contains(NSEventModifierFlags::Control) || f.contains(NSEventModifierFlags::Command),
        f.contains(NSEventModifierFlags::Option),
    )
}

/// A Mac key as the Windows virtual-key code skins compare event.keyCode with.
fn virtual_key(event: &NSEvent) -> Option<u32> {
    let vk = match event.keyCode() {
        123 => 0x25,
        124 => 0x27,
        125 => 0x28,
        126 => 0x26,
        115 => 0x24,
        119 => 0x23,
        116 => 0x21,
        121 => 0x22,
        36 | 76 => 0x0D,
        53 => 0x1B,
        48 => 0x09,
        49 => 0x20,
        51 => 0x08,
        117 => 0x2E,
        122 => 0x70,
        120 => 0x71,
        99 => 0x72,
        118 => 0x73,
        96 => 0x74,
        97 => 0x75,
        98 => 0x76,
        100 => 0x77,
        101 => 0x78,
        109 => 0x79,
        _ => {
            let chars = event.charactersIgnoringModifiers()?.to_string().to_ascii_uppercase();
            let c = chars.chars().next()?;
            if !c.is_ascii_alphanumeric() {
                return None;
            }
            c as u32
        }
    };
    Some(vk)
}

impl Surface {
    fn new(mtm: MainThreadMarker, view: ViewId, frame: NSRect) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(SurfaceIvars { view: Cell::new(view) });
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: frame] };
        this.setWantsLayer(true);
        if let Some(layer) = this.layer() {
            layer.setContentsGravity(unsafe { kCAGravityTopLeft });
            layer.setMagnificationFilter(unsafe { kCAFilterNearest });
            layer.setOpaque(false);
        }
        this
    }

    fn point(&self, event: &NSEvent) -> (f64, f64) {
        let p = self.convertPoint_fromView(event.locationInWindow(), None);
        (p.x, p.y)
    }

    fn press(&self, event: &NSEvent, button: MouseButton) {
        let Some(shared) = current() else { return };
        let v = self.ivars().view.get();
        let (x, y) = self.point(event);
        let clicks = event.clickCount().max(1) as u32;
        let result = shared.with(|s| {
            if s.views.get(v).is_some_and(Option::is_some) { s.mouse_down(v, x, y, button, clicks, modifiers(event)) } else { MouseResult::Ignored }
        });
        match result {
            MouseResult::Drag if button == MouseButton::Left && clicks == 1 => {
                if let Some(w) = self.window() {
                    w.performWindowDragWithEvent(event);
                }
            }
            MouseResult::ContextMenu if shared.show_app_menu(v, "context", (x, y)) == Some(AppAction::Quit) => shared.close(),
            _ => {}
        }
    }

    fn release(&self, event: &NSEvent, button: MouseButton) {
        let Some(shared) = current() else { return };
        let v = self.ivars().view.get();
        let (x, y) = self.point(event);
        shared.with(|s| {
            if s.views.get(v).is_some_and(Option::is_some) {
                s.mouse_up(v, x, y, button, modifiers(event));
            }
        });
    }

    fn motion(&self, event: &NSEvent) {
        let Some(shared) = current() else { return };
        let v = self.ivars().view.get();
        let (x, y) = self.point(event);
        shared.with(|s| {
            if s.views.get(v).is_some_and(Option::is_some) {
                s.mouse_move(v, x, y, modifiers(event));
            }
        });
    }
}

/// The height of the primary screen: Cocoa's y axis runs up from its bottom edge.
fn primary_height(mtm: MainThreadMarker) -> f64 {
    objc2_app_kit::NSScreen::screens(mtm).firstObject().map_or(900.0, |s| s.frame().size.height)
}

pub struct ViewWindow {
    pub window: Retained<SkinWindow>,
    surface: Retained<Surface>,
    /// Where the window was last placed or seen, as skin coordinates (top-left).
    placed: Cell<(f64, f64)>,
    size: Cell<(f64, f64)>,
    /// The drawing shown now, which a redrawn region is pasted into.
    shown: RefCell<Option<crate::render::Canvas>>,
}

impl ViewWindow {
    pub fn open(mtm: MainThreadMarker, view: ViewId, title: &str, left: f64, top: f64, w: f64, h: f64) -> Self {
        let rect = NSRect::new(NSPoint::new(left, primary_height(mtm) - top - h), NSSize::new(w.max(1.0), h.max(1.0)));
        let style = NSWindowStyleMask::Borderless | NSWindowStyleMask::Miniaturizable;
        let window: Retained<SkinWindow> = unsafe {
            msg_send![SkinWindow::alloc(mtm), initWithContentRect: rect, styleMask: style, backing: NSBackingStoreType::Buffered, defer: false]
        };
        unsafe { window.setReleasedWhenClosed(false) };
        window.setOpaque(false);
        window.setBackgroundColor(Some(&NSColor::clearColor()));
        window.setHasShadow(false);
        window.setAcceptsMouseMovedEvents(true);
        window.setTitle(&NSString::from_str(title));
        let surface = Surface::new(mtm, view, NSRect::new(NSPoint::new(0.0, 0.0), rect.size));
        window.setContentView(Some(&surface));
        window.makeKeyAndOrderFront(None);
        window.makeFirstResponder(Some(&surface));
        ViewWindow { window, surface, placed: Cell::new((left, top)), size: Cell::new((w, h)), shown: RefCell::new(None) }
    }

    /// Shows a rendered view.
    pub fn present(&self, canvas: crate::render::Canvas) {
        // In the display's own colour space the window server shows the pixels as they are, with
        // no conversion each frame (and skins look as on Windows, which didn't colour-manage).
        let space = self.window.colorSpace().and_then(|s| s.CGColorSpace());
        let Some(image) = cg_image(&canvas.pixmap, space.as_deref()) else { return };
        if let Some(layer) = self.surface.layer() {
            let obj: &AnyObject = unsafe { &*CFRetained::as_ptr(&image).as_ptr().cast::<AnyObject>() };
            unsafe { layer.setContents(Some(obj)) };
            layer.setContentsScale(f64::from(canvas.scale));
        }
        *self.shown.borrow_mut() = Some(canvas);
    }

    /// Pastes a redrawn region into the shown drawing and presents it. False when there is no
    /// drawing of that scale to paste into (the caller then redraws all of it).
    pub fn present_region(&self, part: &crate::render::Canvas) -> bool {
        let Some(mut canvas) = self.shown.borrow_mut().take() else { return false };
        if canvas.scale != part.scale {
            return false;
        }
        canvas.paste(part);
        self.present(canvas);
        true
    }

    pub fn scale(&self) -> f32 {
        self.window.backingScaleFactor() as f32
    }

    /// Keeps the window's top-left corner while it takes the view's size.
    pub fn resize(&self, w: f64, h: f64) {
        if self.size.get() == (w, h) {
            return;
        }
        self.size.set((w, h));
        let (left, top) = self.placed.get();
        self.place(left, top);
    }

    pub fn place(&self, left: f64, top: f64) {
        let mtm = MainThreadMarker::from(&*self.window);
        let (w, h) = self.size.get();
        self.placed.set((left, top));
        let frame = NSRect::new(NSPoint::new(left, primary_height(mtm) - top - h), NSSize::new(w.max(1.0), h.max(1.0)));
        self.window.setFrame_display(frame, true);
    }

    /// The window's top-left in skin coordinates, when the user has moved it.
    pub fn moved(&self) -> Option<(f64, f64)> {
        let mtm = MainThreadMarker::from(&*self.window);
        let f = self.window.frame();
        let now = (f.origin.x, primary_height(mtm) - f.origin.y - f.size.height);
        let old = self.placed.get();
        ((now.0 - old.0).abs() > 0.5 || (now.1 - old.1).abs() > 0.5).then(|| {
            self.placed.set(now);
            now
        })
    }

    pub fn set_tooltip(&self, text: Option<&str>) {
        self.surface.setToolTip(text.map(NSString::from_str).as_deref());
    }

    pub fn set_cursor(&self, name: &str) {
        let cursor = match name.to_lowercase().as_str() {
            "hand" => NSCursor::pointingHandCursor(),
            "ibeam" => NSCursor::IBeamCursor(),
            "cross" | "crosshair" => NSCursor::crosshairCursor(),
            "no" => NSCursor::operationNotAllowedCursor(),
            "sizeall" | "move" => NSCursor::openHandCursor(),
            "sizens" => NSCursor::rowResizeCursor(),
            "sizewe" => NSCursor::columnResizeCursor(),
            _ => NSCursor::arrowCursor(),
        };
        cursor.set();
    }

    pub fn close(&self) {
        self.window.orderOut(None);
        self.window.close();
    }

    pub fn minimize(&self) {
        self.window.miniaturize(None);
    }

    pub fn begin_move(&self) {
        let mtm = MainThreadMarker::from(&*self.window);
        if let Some(event) = NSApplication::sharedApplication(mtm).currentEvent() {
            self.window.performWindowDragWithEvent(&event);
        }
    }

    /// view.size("bottomright") etc.: follows the mouse until it's released, resizing the view.
    pub fn begin_resize(&self, view: ViewId, edge: &str) {
        let Some(shared) = current() else { return };
        let mut edge = edge.to_lowercase();
        if !["left", "right", "top", "bottom"].iter().any(|e| edge.contains(e)) {
            edge = "bottomright".into();
        }
        let start = NSEvent::mouseLocation();
        let (w0, h0) = self.size.get();
        let (l0, t0) = self.placed.get();
        loop {
            let mask = NSEventMask::LeftMouseDragged | NSEventMask::LeftMouseUp;
            let Some(event) = self.window.nextEventMatchingMask(mask) else { break };
            if event.r#type() == objc2_app_kit::NSEventType::LeftMouseUp {
                break;
            }
            let p = NSEvent::mouseLocation();
            let (dx, dy) = (p.x - start.x, start.y - p.y);
            let w = w0
                + if edge.contains("left") {
                    -dx
                } else if edge.contains("right") {
                    dx
                } else {
                    0.0
                };
            let h = h0
                + if edge.contains("top") {
                    -dy
                } else if edge.contains("bottom") {
                    dy
                } else {
                    0.0
                };
            let (nw, nh) = shared.with(|s| {
                s.window_resized(view, w, h);
                (f64::from(s.width(view, 0)), f64::from(s.height(view, 0)))
            });
            let left = if edge.contains("left") { l0 + (w0 - nw) } else { l0 };
            let top = if edge.contains("top") { t0 + (h0 - nh) } else { t0 };
            self.size.set((nw, nh));
            self.place(left, top);
            shared.with(|s| s.window_moved(view, left, top));
            super::redraw(view);
        }
    }
}

/// A CoreGraphics image of a premultiplied RGBA pixmap.
fn cg_image(pm: &tiny_skia::Pixmap, space: Option<&CGColorSpace>) -> Option<CFRetained<CGImage>> {
    let device = CGColorSpace::new_device_rgb()?;
    let space = space.unwrap_or(&device);
    let info = CGImageByteOrderInfo::Order32Big.0 | CGImageAlphaInfo::PremultipliedLast.0;
    let mut data = pm.data().to_vec();
    let ctx = unsafe {
        objc2_core_graphics::CGBitmapContextCreate(
            data.as_mut_ptr().cast(),
            pm.width() as usize,
            pm.height() as usize,
            8,
            pm.width() as usize * 4,
            Some(space),
            info,
        )
    }?;
    objc2_core_graphics::CGBitmapContextCreateImage(Some(&ctx))
}
