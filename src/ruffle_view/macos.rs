use std::ptr::NonNull;

use crate::ruffle::Surface;
use gpui::{Bounds, Pixels, Window};
use objc2::{
    AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, NSObject, NSObjectProtocol},
    sel,
};
use objc2_app_kit::{
    NSColor, NSView, NSWindow, NSWindowDidChangeOcclusionStateNotification, NSWindowOcclusionState,
    NSWindowOrderingMode,
};
use objc2_core_graphics::CGColor;
use objc2_foundation::{
    NSNotificationCenter, NSPoint, NSRect, NSRunLoop, NSRunLoopCommonModes, NSSize,
};
use objc2_quartz_core::{CADisplayLink, CAFrameRateRange, CAMetalLayer, CATransaction};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the window has no AppKit view")]
    NoAppKitView,
    #[error("GPUI's view isn't in a window's content view")]
    NoContentView,
}

pub(super) struct NativeView {
    view: Retained<NSView>,
    input_view: Retained<NSView>,
    layer: Retained<CAMetalLayer>,
    frame_clock: Option<Retained<CADisplayLink>>,
    occlusion_observer: Option<Retained<Callback>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "LyraCallback"]
    #[ivars = Box<dyn Fn()>]
    struct Callback;

    impl Callback {
        #[unsafe(method(call:))]
        fn call(&self, _sender: &AnyObject) {
            (self.ivars())();
        }
    }
);

impl Callback {
    fn new(callback: impl Fn() + 'static) -> Retained<Callback> {
        unsafe { msg_send![super(Callback::alloc().set_ivars(Box::new(callback))), init] }
    }
}

fn is_visible(window: &NSWindow) -> bool {
    window
        .occlusionState()
        .contains(NSWindowOcclusionState::Visible)
}

impl NativeView {
    pub(super) fn attach(window: &Window) -> Result<NativeView, Error> {
        let handle = HasWindowHandle::window_handle(window).map_err(|_| Error::NoAppKitView)?;
        let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
            return Err(Error::NoAppKitView);
        };
        let gpui_view = unsafe { handle.ns_view.cast::<NSView>().as_ref() };
        let content = unsafe { gpui_view.superview() }.ok_or(Error::NoContentView)?;
        // GPUI's layer is transparent over the players, so any area no player covers shows the window background.
        if let Some(window) = content.window() {
            window.setBackgroundColor(Some(&NSColor::blackColor()));
        }
        let mtm = MainThreadMarker::new().unwrap();
        let layer = CAMetalLayer::new();
        layer.setBackgroundColor(Some(&CGColor::new_srgb(0.0, 0.0, 0.0, 1.0)));
        layer.setOpaque(true);
        let view = NSView::initWithFrame(NSView::alloc(mtm), NSRect::ZERO);
        view.setLayer(Some(&layer));
        view.setWantsLayer(true);
        content.addSubview_positioned_relativeTo(&view, NSWindowOrderingMode::Below, None);
        let input_view =
            unsafe { Retained::retain(handle.ns_view.cast::<NSView>().as_ptr()) }.unwrap();
        Ok(NativeView {
            view,
            input_view,
            layer,
            frame_clock: None,
            occlusion_observer: None,
        })
    }

    pub(super) fn supports_display_frames(&self) -> bool {
        self.view
            .respondsToSelector(sel!(displayLinkWithTarget:selector:))
    }

    pub(super) fn connect_frames(&mut self, on_frame: impl Fn() + 'static) {
        if !self.supports_display_frames() {
            return;
        }
        let target = Callback::new(on_frame);
        let clock = unsafe {
            self.view
                .displayLinkWithTarget_selector(&target, sel!(call:))
        };
        clock.setPaused(true);
        clock.setPreferredFrameRateRange(CAFrameRateRange {
            minimum: 60.0,
            maximum: 72.0,
            preferred: 72.0,
        });
        unsafe {
            clock.addToRunLoop_forMode(&NSRunLoop::mainRunLoop(), NSRunLoopCommonModes);
        }
        self.frame_clock = Some(clock);
    }

    pub(super) fn connect_visibility(&mut self, on_change: impl Fn(bool) + 'static) {
        let Some(window) = self.view.window() else {
            return;
        };
        on_change(is_visible(&window));
        let observer = Callback::new({
            let window = window.clone();
            move || on_change(is_visible(&window))
        });
        unsafe {
            NSNotificationCenter::defaultCenter().addObserver_selector_name_object(
                &observer,
                sel!(call:),
                Some(NSWindowDidChangeOcclusionStateNotification),
                Some(&window),
            );
        }
        self.occlusion_observer = Some(observer);
    }

    pub(super) fn set_shown(&self, shown: bool) {
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        self.view.setHidden(!shown);
        CATransaction::commit();
    }

    pub(super) fn set_frame_clock(&self, enabled: bool) {
        if let Some(clock) = &self.frame_clock {
            clock.setPaused(!enabled);
        }
    }

    pub(super) fn surface(&self) -> Surface {
        unsafe { Surface::from_metal_layer(NonNull::from(&*self.layer).cast()) }
    }

    pub(super) fn menu_views(&self) -> (Retained<NSView>, Retained<NSView>) {
        (self.view.clone(), self.input_view.clone())
    }

    pub(super) fn set_frame(&self, bounds: Bounds<Pixels>, scale: f32) {
        let Some(content) = (unsafe { self.view.superview() }) else {
            return;
        };
        let width = f64::from(f32::from(bounds.size.width));
        let height = f64::from(f32::from(bounds.size.height));
        let x = f64::from(f32::from(bounds.origin.x));
        let y = content.bounds().size.height - f64::from(f32::from(bounds.origin.y)) - height;
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        let frame = NSRect::new(NSPoint::new(x, y), NSSize::new(width, height));
        self.view.setFrame(frame);
        self.layer.setFrame(frame);
        self.layer.setContentsScale(scale.into());
        CATransaction::commit();
    }
}

impl Drop for NativeView {
    fn drop(&mut self) {
        if let Some(clock) = &self.frame_clock {
            clock.invalidate();
        }
        if let Some(observer) = &self.occlusion_observer {
            unsafe { NSNotificationCenter::defaultCenter().removeObserver(observer) };
        }
        self.view.removeFromSuperview();
    }
}
