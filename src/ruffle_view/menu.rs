use std::cell::Cell;

use block2::RcBlock;
use objc2::{
    AnyThread, DefinedClass, MainThreadMarker, define_class, msg_send, rc::Retained,
    runtime::NSObject, sel,
};
use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSEventType, NSMenu, NSMenuItem, NSView};
use objc2_foundation::{NSPoint, NSRunLoop, NSString};

use crate::ruffle::{
    context_menu::{ContextMenu, MenuSelection},
    extras::{Extra, Extras, Group},
};

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "MiraPlayerMenuTarget"]
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
pub(super) async fn show(
    view: Retained<NSView>,
    context: ContextMenu,
    extras: Extras,
) -> Option<MenuSelection> {
    let (sender, selection) = async_channel::bounded(1);
    let pending = Cell::new(Some((view, context)));
    let block = RcBlock::new(move || {
        if let Some((view, context)) = pending.take() {
            let _ = sender.try_send(track(&view, context, extras));
        }
    });
    // SAFETY: the main run loop runs the block on the main thread, where it was created.
    unsafe { NSRunLoop::mainRunLoop().performBlock(&block) };
    selection.recv().await.ok().flatten()
}

fn track(view: &NSView, context: ContextMenu, extras: Extras) -> Option<MenuSelection> {
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
    let mut mira_items = Vec::new();
    let mut add = |menu: &NSMenu,
                   title: &str,
                   checked: bool,
                   tooltip: Option<&str>,
                   selection: MenuSelection| {
        let native_item = NSMenuItem::new(mtm);
        native_item.setTitle(&NSString::from_str(title));
        native_item.setState(isize::from(checked));
        native_item.setToolTip(tooltip.map(NSString::from_str).as_deref());
        native_item.setTag((context.items.len() + mira_items.len()) as isize);
        unsafe {
            native_item.setTarget(Some(&target));
            native_item.setAction(Some(sel!(selectItem:)));
        }
        menu.addItem(&native_item);
        mira_items.push(selection);
    };
    add(
        &menu,
        "Smooth motion",
        context.smooth_motion,
        None,
        MenuSelection::SmoothMotion,
    );
    add(
        &menu,
        "Performance stats",
        context.stats,
        None,
        MenuSelection::Stats,
    );
    for group in Group::ALL {
        menu.addItem(&NSMenuItem::separatorItem(mtm));
        menu.addItem(&NSMenuItem::sectionHeaderWithTitle(
            &NSString::from_str(group.label()),
            mtm,
        ));
        for extra in Extra::ALL
            .into_iter()
            .filter(|extra| extra.group() == group)
        {
            add(
                &menu,
                extra.label(),
                extras.enabled(extra),
                Some(extra.tooltip()),
                MenuSelection::Extra(extra),
            );
        }
    }
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
        index
            .checked_sub(context.items.len())
            .map_or(MenuSelection::Ruffle(index), |offset| mira_items[offset])
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
