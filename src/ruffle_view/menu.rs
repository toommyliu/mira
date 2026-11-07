use std::cell::Cell;

use block2::RcBlock;
use objc2::{
    AnyThread, DefinedClass, MainThreadMarker, define_class, msg_send, rc::Retained,
    runtime::NSObject, sel,
};
use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSEventType, NSMenu, NSMenuItem, NSView};
use objc2_foundation::{NSPoint, NSRunLoop, NSString};

use crate::ruffle::context_menu::{ContextMenu, MenuSelection};

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "LyraPlayerMenuTarget"]
    #[ivars = Cell<Option<usize>>]
    struct MenuTarget;

    impl MenuTarget {
        #[unsafe(method(selectItem:))]
        fn select_item(&self, sender: &NSMenuItem) {
            self.ivars().set(Some(sender.tag() as usize));
        }
    }
);

// GCD won't drain the main queue while one of its blocks is tracking a menu, which stalls GPUI.
pub(super) async fn show(view: Retained<NSView>, context: ContextMenu) -> Option<MenuSelection> {
    let (sender, selection) = async_channel::bounded(1);
    let pending = Cell::new(Some((view, context)));
    let block = RcBlock::new(move || {
        if let Some((view, context)) = pending.take() {
            let _ = sender.try_send(track(&view, context));
        }
    });
    // SAFETY: the main run loop runs the block on the main thread, where it was created.
    unsafe { NSRunLoop::mainRunLoop().performBlock(&block) };
    selection.recv().await.ok().flatten()
}

fn track(view: &NSView, context: ContextMenu) -> Option<MenuSelection> {
    let mtm = MainThreadMarker::new().unwrap();
    let target: Retained<MenuTarget> =
        unsafe { msg_send![super(MenuTarget::alloc().set_ivars(Cell::new(None))), init] };
    let menu = NSMenu::new(mtm);
    menu.setAutoenablesItems(false);
    for (index, item) in context.items.iter().enumerate() {
        if index != 0 && item.separator_before {
            menu.addItem(&NSMenuItem::separatorItem(mtm));
        }
        let native_item = NSMenuItem::new(mtm);
        native_item.setTitle(&NSString::from_str(&item.caption));
        native_item.setEnabled(item.enabled);
        native_item.setState(isize::from(item.checked));
        native_item.setTag(index as isize);
        unsafe {
            native_item.setTarget(Some(&target));
            native_item.setAction(Some(sel!(selectItem:)));
        }
        menu.addItem(&native_item);
    }
    if !context.items.is_empty() {
        menu.addItem(&NSMenuItem::separatorItem(mtm));
    }
    let smooth_motion = NSMenuItem::new(mtm);
    smooth_motion.setTitle(&NSString::from_str("Smooth motion"));
    smooth_motion.setState(isize::from(context.smooth_motion));
    smooth_motion.setTag(context.items.len() as isize);
    unsafe {
        smooth_motion.setTarget(Some(&target));
        smooth_motion.setAction(Some(sel!(selectItem:)));
    }
    menu.addItem(&smooth_motion);
    let stats = NSMenuItem::new(mtm);
    stats.setTitle(&NSString::from_str("Performance stats"));
    stats.setState(isize::from(context.stats));
    stats.setTag(context.items.len() as isize + 1);
    unsafe {
        stats.setTarget(Some(&target));
        stats.setAction(Some(sel!(selectItem:)));
    }
    menu.addItem(&stats);
    let y = if view.isFlipped() {
        f64::from(context.at.y)
    } else {
        view.bounds().size.height - f64::from(context.at.y)
    };
    menu.popUpMenuPositioningItem_atLocation_inView(
        None,
        NSPoint::new(f64::from(context.at.x), y),
        Some(view),
    );
    target.ivars().get().map(|index| {
        if index == context.items.len() {
            MenuSelection::SmoothMotion
        } else if index == context.items.len() + 1 {
            MenuSelection::Stats
        } else {
            MenuSelection::Ruffle(index)
        }
    })
}

pub(super) fn modifiers() -> gpui::Modifiers {
    let flags = NSEvent::modifierFlags_class();
    gpui::Modifiers {
        shift: flags.contains(NSEventModifierFlags::Shift),
        control: flags.contains(NSEventModifierFlags::Control),
        alt: flags.contains(NSEventModifierFlags::Option),
        platform: flags.contains(NSEventModifierFlags::Command),
        function: flags.contains(NSEventModifierFlags::Function),
    }
}

pub(super) fn sync_modifiers(view: &NSView) {
    let Some(window) = view.window() else { return };
    let empty = NSString::from_str("");
    let event = NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
        NSEventType::FlagsChanged, NSPoint::ZERO, NSEvent::modifierFlags_class(), 0.0,
        window.windowNumber(), None, &empty, &empty, false, 0,
    );
    if let Some(event) = event {
        // GPUI deduplicates flagsChanged events using a native modifier cache.
        view.flagsChanged(&event);
    }
}
