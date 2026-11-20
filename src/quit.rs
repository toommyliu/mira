use std::{
    cell::Cell,
    ptr::NonNull,
    rc::Rc,
    time::{Duration, Instant},
};

use block2::RcBlock;
use gpui::{
    App, Context, Global, IntoElement, KeyBinding, RenderOnce, Styled as _, Task, Window, WindowId,
    actions, div,
};
use mira_ui::{components::shortcut_hint, motion};
use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSApplication, NSEvent, NSEventMask, NSEventModifierFlags, NSEventSubtype, NSEventType,
};
use objc2_foundation::ns_string;

use crate::settings::{self, QuitShortcut};

actions!(mira, [Quit]);

const DOUBLE_PRESS: Duration = Duration::from_millis(500);
const HOLD: Duration = Duration::from_millis(1200);
const HINT_LINGER: Duration = Duration::from_millis(1500);

#[derive(Clone, Copy)]
enum Signal {
    Pressed {
        window: WindowId,
        hold: bool,
        at: Duration,
    },
    Released {
        at: Duration,
    },
}

impl Global for Signal {}

struct Hint {
    hold: bool,
    at: Duration,
    phase: Phase,
}

enum Phase {
    Held,
    Reading { _dismiss: Task<()> },
    Leaving(Instant),
}

impl Hint {
    fn label(&self) -> &'static str {
        if self.hold {
            "Hold ⌘Q to quit"
        } else {
            "Press ⌘Q twice to quit"
        }
    }

    fn release(&mut self, cx: &mut Context<Option<Hint>>) {
        let dismiss = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(HINT_LINGER).await;
            let _ = this.update(cx, |hint, cx| {
                if let Some(hint) = hint {
                    hint.phase = Phase::Leaving(Instant::now());
                    cx.notify();
                }
            });
        });
        self.phase = Phase::Reading { _dismiss: dismiss };
    }
}

#[derive(IntoElement)]
pub(crate) struct Overlay;

impl RenderOnce for Overlay {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let hint = window.use_state(cx, |window, cx: &mut Context<Option<Hint>>| {
            let owner = window.window_handle().window_id();
            cx.observe_global::<Signal>(move |hint, cx| match *cx.global::<Signal>() {
                Signal::Pressed { window, hold, at } if window == owner => {
                    *hint = Some(Hint {
                        hold,
                        at,
                        phase: Phase::Held,
                    });
                    cx.notify();
                }
                Signal::Released { at } => {
                    if let Some(hint) = hint
                        .as_mut()
                        .filter(|hint| hint.at == at && matches!(hint.phase, Phase::Held))
                    {
                        hint.release(cx);
                        cx.notify();
                    }
                }
                Signal::Pressed { .. } => {}
            })
            .detach();
            cx.observe_window_activation(window, |hint, window, cx| {
                if !window.is_window_active() && hint.take().is_some() {
                    cx.notify();
                }
            })
            .detach();
            None
        });
        let Some(hint) = hint.read(cx) else {
            return div().absolute().into_any_element();
        };
        let opacity = match hint.phase {
            Phase::Held | Phase::Reading { .. } => Some(1.0),
            Phase::Leaving(since) => motion::leaving(since, window, cx),
        };
        match opacity {
            Some(opacity) => shortcut_hint(hint.label(), opacity, window, cx).into_any_element(),
            None => div().absolute().into_any_element(),
        }
    }
}

#[derive(Clone, Copy)]
struct Press {
    mode: QuitShortcut,
    at: Duration,
    key_code: u16,
    held: bool,
}

#[derive(Default)]
struct Confirmation {
    first: Cell<Option<Press>>,
}

#[derive(Debug, PartialEq, Eq)]
enum Response {
    Quit,
    Hint { hold: bool },
    Wait,
}

impl Confirmation {
    fn respond(&self, mode: QuitShortcut, event: Option<&NSEvent>, cx: &mut App) {
        match self.response(mode, event) {
            Response::Quit => cx.quit(),
            Response::Hint { hold } => {
                if let (Some(window), Some(press)) = (cx.active_window(), self.first.get()) {
                    cx.set_global(Signal::Pressed {
                        window: window.window_id(),
                        hold,
                        at: press.at,
                    });
                }
            }
            Response::Wait => {}
        }
    }

    fn response(&self, mode: QuitShortcut, event: Option<&NSEvent>) -> Response {
        let Some(key) = shortcut_event(event) else {
            return Response::Quit;
        };
        let key_code = key.keyCode();
        let at = Duration::from_secs_f64(key.timestamp());
        let repeat = key.isARepeat();
        let first = self
            .first
            .get()
            .filter(|press| press.mode == mode && press.key_code == key_code);
        let elapsed = first
            .filter(|press| !repeat || press.held)
            .and_then(|press| at.checked_sub(press.at));
        let response = match (mode, repeat) {
            (QuitShortcut::Direct, _) => Response::Quit,
            (QuitShortcut::Hold, true) if elapsed.is_some_and(|elapsed| elapsed >= HOLD) => {
                Response::Quit
            }
            (_, true) => Response::Wait,
            (_, false) if elapsed.is_some_and(|elapsed| elapsed <= DOUBLE_PRESS) => Response::Quit,
            (QuitShortcut::Hold, false) => Response::Hint { hold: true },
            (QuitShortcut::DoublePress, false) => Response::Hint { hold: false },
        };
        match response {
            Response::Quit => self.first.set(None),
            Response::Hint { .. } => self.first.set(Some(Press {
                mode,
                at,
                key_code,
                held: true,
            })),
            Response::Wait => {}
        }
        response
    }

    fn observe(&self, event: &NSEvent) -> Option<Signal> {
        let press = self.first.get()?;
        match event.r#type() {
            NSEventType::KeyUp if event.keyCode() == press.key_code => {}
            NSEventType::FlagsChanged if !command_only(event) => {}
            NSEventType::AppKitDefined
                if event.subtype() == NSEventSubtype::ApplicationDeactivated =>
            {
                self.first.set(None);
                return None;
            }
            _ => return None,
        }
        if !press.held {
            return None;
        }
        self.first.set(Some(Press {
            held: false,
            ..press
        }));
        Some(Signal::Released { at: press.at })
    }
}

fn command_only(event: &NSEvent) -> bool {
    use NSEventModifierFlags as Flags;
    let modifiers =
        Flags::Command | Flags::Shift | Flags::Control | Flags::Option | Flags::Function;
    event.modifierFlags() & modifiers == Flags::Command
}

pub(crate) fn init(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);
    let confirmation = Rc::new(Confirmation::default());
    watch_releases(confirmation.clone(), cx);
    cx.on_action(move |_: &Quit, cx| {
        // AppKit routes both the menu's key equivalent and an explicit selection to Quit.
        let event = MainThreadMarker::new()
            .and_then(|main| NSApplication::sharedApplication(main).currentEvent());
        confirmation.respond(settings::quit_shortcut(cx), event.as_deref(), cx);
    });
}

fn shortcut_event(event: Option<&NSEvent>) -> Option<&NSEvent> {
    event.filter(|event| {
        event.r#type() == NSEventType::KeyDown
            && command_only(event)
            && (event
                .characters()
                .is_some_and(|key| key.isEqualToString(ns_string!("q")))
                || event
                    .charactersByApplyingModifiers(NSEventModifierFlags::Command)
                    .is_none_or(|key| key.isEqualToString(ns_string!("q"))))
    })
}

fn watch_releases(confirmation: Rc<Confirmation>, cx: &App) {
    let events = NSEventMask::KeyUp | NSEventMask::FlagsChanged | NSEventMask::AppKitDefined;
    let app = cx.to_async();
    let observe = RcBlock::new(move |event: NonNull<NSEvent>| {
        // AppKit lends the event to this block and expects the same pointer to continue dispatch.
        if let Some(signal) = confirmation.observe(unsafe { event.as_ref() }) {
            app.spawn(async move |cx| cx.update(|cx| cx.set_global(signal)))
                .detach();
        }
        event.as_ptr()
    });
    let monitor =
        unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(events, &observe) };
    if let Some(monitor) = monitor {
        cx.on_app_quit(move |_| {
            unsafe { NSEvent::removeMonitor(&monitor) };
            async {}
        })
        .detach();
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use objc2::rc::Retained;
    use objc2_foundation::NSPoint;

    const Q: u16 = 12;

    fn key(
        kind: NSEventType,
        modifiers: NSEventModifierFlags,
        ms: u64,
        repeat: bool,
    ) -> Retained<NSEvent> {
        NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
            kind, NSPoint::ZERO, modifiers, Duration::from_millis(ms).as_secs_f64(), 0, None, ns_string!("q"), ns_string!("q"), repeat, Q,
        ).unwrap()
    }

    fn cmd_q(ms: u64, repeat: bool) -> Retained<NSEvent> {
        key(
            NSEventType::KeyDown,
            NSEventModifierFlags::Command,
            ms,
            repeat,
        )
    }

    pub(crate) fn press_quit_shortcut(cx: &mut App) {
        Confirmation::default().respond(settings::quit_shortcut(cx), Some(&cmd_q(0, false)), cx);
    }

    #[gpui::test]
    fn hint_stays_in_its_window_while_held_then_expires(cx: &mut gpui::TestAppContext) {
        use gpui::{
            FocusHandle, InteractiveElement as _, ParentElement as _, Render,
            StatefulInteractiveElement as _, VisualTestContext,
        };

        struct Content {
            focus: FocusHandle,
            clicks: usize,
        }
        impl Render for Content {
            fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
                div()
                    .size_full()
                    .child(
                        div()
                            .id("content")
                            .size_full()
                            .track_focus(&self.focus)
                            .on_click(cx.listener(|this, _, _, _| this.clicks += 1)),
                    )
                    .child(Overlay)
            }
        }
        let build = |window: &mut Window, cx: &mut Context<Content>| {
            let focus = cx.focus_handle();
            window.focus(&focus, cx);
            Content { focus, clicks: 0 }
        };
        cx.update(|cx| {
            mira_ui::init(cx);
            cx.set_reduce_motion(true);
        });
        let mut other = cx.add_window_view(build).1.clone();
        let (view, active) = cx.add_window_view(build);
        active.update(|window, _| window.activate_window());
        active.run_until_parked();

        let confirmation = Confirmation::default();
        let press = |mode, ms, cx: &mut VisualTestContext| {
            cx.update(|_, cx| confirmation.respond(mode, Some(&cmd_q(ms, false)), cx));
            cx.run_until_parked();
        };
        let release = |ms, cx: &mut VisualTestContext| {
            cx.update(|_, cx| {
                let up = key(NSEventType::KeyUp, NSEventModifierFlags::Command, ms, false);
                if let Some(signal) = confirmation.observe(&up) {
                    cx.set_global(signal);
                }
            });
            cx.run_until_parked();
        };
        let wait = |ms, cx: &mut VisualTestContext| {
            cx.executor().advance_clock(Duration::from_millis(ms));
            cx.run_until_parked();
            cx.update(|window, _| window.refresh());
            cx.run_until_parked();
        };

        press(QuitShortcut::DoublePress, 0, active);
        let panel = active
            .debug_bounds("shortcut-hint")
            .expect("show the hint on the first press");
        assert!(other.debug_bounds("shortcut-hint").is_none());
        active.update(|window, cx| {
            assert!(view.read(cx).focus.is_focused(window));
            let size = window.viewport_size();
            assert!((panel.center().x - size.width / 2.0).abs() <= gpui::px(1.0));
            assert!(panel.bottom() < size.height / 2.0);
        });
        active.simulate_click(panel.center(), gpui::Modifiers::default());
        assert_eq!(active.update(|_, cx| view.read(cx).clicks), 1);

        wait(3000, active);
        assert!(active.debug_bounds("shortcut-hint").is_some());
        release(3000, active);
        wait(1499, active);
        assert!(active.debug_bounds("shortcut-hint").is_some());
        wait(2, active);
        assert!(active.debug_bounds("shortcut-hint").is_none());

        press(QuitShortcut::Hold, 5000, active);
        active.deactivate_window();
        active.update(|window, _| window.activate_window());
        active.run_until_parked();
        assert!(active.debug_bounds("shortcut-hint").is_none());
    }

    #[test]
    fn menu_selection_quits_immediately_in_every_mode() {
        let click = NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
            NSEventType::LeftMouseUp, NSPoint::ZERO, NSEventModifierFlags::empty(), 1.0, 0, None, 1, 1, 0.0,
        ).unwrap();
        let return_key = NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
            NSEventType::KeyDown, NSPoint::ZERO, NSEventModifierFlags::empty(), 1.0, 0, None, ns_string!("\r"), ns_string!("\r"), false, 36,
        ).unwrap();
        for mode in [
            QuitShortcut::Direct,
            QuitShortcut::Hold,
            QuitShortcut::DoublePress,
        ] {
            let confirmation = Confirmation::default();
            assert_eq!(confirmation.response(mode, Some(&click)), Response::Quit);
            assert_eq!(
                confirmation.response(mode, Some(&return_key)),
                Response::Quit
            );
            assert_eq!(confirmation.response(mode, None), Response::Quit);
        }
    }

    #[test]
    fn only_a_repeat_can_complete_a_hold() {
        let confirmation = Confirmation::default();
        let press =
            |ms, repeat| confirmation.response(QuitShortcut::Hold, Some(&cmd_q(ms, repeat)));
        let hint = Response::Hint { hold: true };
        assert_eq!(press(0, true), Response::Wait);
        assert_eq!(press(100, false), hint);
        assert_eq!(press(200, true), Response::Wait);
        assert_eq!(press(1299, true), Response::Wait);
        assert_eq!(press(1300, true), Response::Quit);
        assert_eq!(press(1500, false), hint);
        assert_eq!(press(3000, false), hint);
        assert_eq!(press(3300, true), Response::Wait);
        confirmation.observe(&key(
            NSEventType::KeyUp,
            NSEventModifierFlags::Command,
            3350,
            false,
        ));
        assert_eq!(press(4200, true), Response::Wait);
        assert_eq!(press(5000, false), hint);
        confirmation.observe(&key(
            NSEventType::FlagsChanged,
            NSEventModifierFlags::empty(),
            5100,
            false,
        ));
        assert_eq!(press(6200, true), Response::Wait);
    }

    #[test]
    fn two_distinct_presses_must_be_within_half_a_second() {
        for (mode, hold) in [
            (QuitShortcut::Hold, true),
            (QuitShortcut::DoublePress, false),
        ] {
            let confirmation = Confirmation::default();
            let press = |ms, repeat| confirmation.response(mode, Some(&cmd_q(ms, repeat)));
            assert_eq!(press(0, false), Response::Hint { hold });
            assert_eq!(press(400, true), Response::Wait);
            assert_eq!(press(501, false), Response::Hint { hold });
            confirmation.observe(&key(
                NSEventType::KeyUp,
                NSEventModifierFlags::Command,
                550,
                false,
            ));
            assert_eq!(press(1001, false), Response::Quit);
        }
        let confirmation = Confirmation::default();
        let press = |mode, ms, repeat| confirmation.response(mode, Some(&cmd_q(ms, repeat)));
        assert!(matches!(
            press(QuitShortcut::DoublePress, 0, false),
            Response::Hint { .. }
        ));
        assert_eq!(press(QuitShortcut::DoublePress, 1200, true), Response::Wait);
        assert_eq!(press(QuitShortcut::Direct, 2000, false), Response::Quit);
    }
}
