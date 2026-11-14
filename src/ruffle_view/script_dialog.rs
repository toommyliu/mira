use std::{collections::VecDeque, rc::Rc};

use gpui::{
    App, AppContext as _, ClickEvent, Context, Entity, FocusHandle, Focusable as _,
    InteractiveElement as _, IntoElement, KeyBinding, KeyDownEvent, NoAction, ParentElement as _,
    Pixels, Render, ScrollHandle, SharedString, StatefulInteractiveElement as _, Styled as _, Task,
    Window, div, linear_color_stop, linear_gradient, prelude::FluentBuilder as _, px,
};
use mira_ui::{
    ActiveTheme as _,
    primitives::{Button, Dialog, Input, Scrollbar, button::Variant},
    theme::{LINE_BODY, TEXT_BODY},
};

use crate::scripting::{CallId, Dialog as Asked, DialogRequest, ToDialogs};

const CONTEXT: &str = "ScriptDialog";

pub(crate) fn bind_keys(cx: &mut App) {
    cx.bind_keys([KeyBinding::new(
        "enter",
        NoAction,
        Some(&format!("{CONTEXT} > Input")),
    )]);
}

pub(super) struct ScriptDialogs {
    script: SharedString,
    queue: VecDeque<Asked>,
    last: Option<DialogRequest>,
    prompt: Option<Prompt>,
    focus: FocusHandle,
    focused: Option<CallId>,
    scroll: ScrollHandle,
    armed: bool,
    stop: Rc<dyn Fn(&mut App)>,
    listening: Option<Task<()>>,
}

const MAX_LABEL: usize = 24;
const MAX_TITLE: usize = 60;
const WIDTH: Pixels = px(420.0);
const WINDOW_MARGIN: Pixels = px(16.0);
const STOP_INSET: Pixels = px(12.0);

#[derive(Debug, PartialEq)]
struct Content {
    title: SharedString,
    message: SharedString,
    cancel: Option<SharedString>,
    accept: SharedString,
}

impl Content {
    fn of(request: &DialogRequest, script: &SharedString) -> Self {
        let label = |label: &Option<String>, fallback: &'static str| -> SharedString {
            label
                .as_deref()
                .filter(|label| !label.trim().is_empty())
                .map_or_else(|| fallback.into(), |label| shorten(label, MAX_LABEL))
        };
        let (message, title, cancel, accept) = match request {
            DialogRequest::Alert { message, title } => (message, title, None, "OK".into()),
            DialogRequest::Confirm {
                message,
                title,
                confirm,
                cancel,
            } => (
                message,
                title,
                Some(label(cancel, "Cancel")),
                label(confirm, "Confirm"),
            ),
            DialogRequest::Prompt { message, title, .. } => {
                (message, title, Some("Cancel".into()), "OK".into())
            }
        };
        let title = title
            .as_deref()
            .filter(|title| !title.trim().is_empty())
            .map_or_else(|| script.clone(), |title| shorten(title, MAX_TITLE));
        Self {
            title,
            message: message.clone().into(),
            cancel,
            accept,
        }
    }
}

fn shorten(text: &str, max: usize) -> SharedString {
    if text.chars().count() <= max {
        return text.to_owned().into();
    }
    let kept: String = text.chars().take(max - 1).collect();
    format!("{}…", kept.trim_end()).into()
}

#[derive(Clone, Copy, PartialEq)]
enum By {
    Key,
    Pointer,
}

struct Prompt {
    id: CallId,
    input: Entity<Input>,
}

impl ScriptDialogs {
    pub(super) fn new(stop: impl Fn(&mut App) + 'static, cx: &mut Context<Self>) -> Self {
        Self {
            script: SharedString::default(),
            queue: VecDeque::new(),
            last: None,
            prompt: None,
            focus: cx.focus_handle().tab_stop(true),
            focused: None,
            scroll: ScrollHandle::new(),
            armed: true,
            stop: Rc::new(stop),
            listening: None,
        }
    }

    pub(super) fn start(
        &mut self,
        script: SharedString,
        messages: async_channel::Receiver<ToDialogs>,
        cx: &mut Context<Self>,
    ) {
        self.script = script;
        self.queue.clear();
        self.prompt = None;
        self.focused = None;
        self.listening = Some(cx.spawn(async move |this, cx| {
            while let Ok(message) = messages.recv().await {
                if this
                    .update(cx, |this, cx| this.handle(message, cx))
                    .is_err()
                {
                    break;
                }
            }
        }));
        cx.notify();
    }

    pub(super) fn waiting(&self) -> bool {
        !self.queue.is_empty()
    }

    pub(super) fn end(&mut self, cx: &mut Context<Self>) {
        self.listening = None;
        self.queue.clear();
        cx.notify();
    }

    fn handle(&mut self, message: ToDialogs, cx: &mut Context<Self>) {
        match message {
            ToDialogs::Open(dialog) => {
                if self.queue.is_empty() {
                    self.armed = true;
                }
                self.queue.push_back(dialog);
            }
            ToDialogs::Close(id) => self.queue.retain(|dialog| dialog.id != id),
        }
        self.prepare(cx);
        cx.notify();
    }

    fn answer(&mut self, accept: bool, by: By, cx: &mut Context<Self>) {
        if by == By::Key && !self.armed {
            return;
        }
        let Some(dialog) = self.queue.pop_front() else {
            return;
        };
        if by == By::Key {
            self.armed = false;
        }
        if accept {
            let entered = self
                .prompt
                .as_ref()
                .filter(|prompt| prompt.id == dialog.id)
                .map(|prompt| prompt.input.read(cx).text().to_owned())
                .unwrap_or_default();
            dialog.accept(&entered);
        } else {
            dialog.cancel();
        }
        self.prepare(cx);
        cx.notify();
    }

    fn prepare(&mut self, cx: &mut Context<Self>) {
        let Some(front) = self.queue.front() else {
            return;
        };
        let DialogRequest::Prompt {
            message,
            default,
            placeholder,
            ..
        } = &front.request
        else {
            self.prompt = None;
            return;
        };
        if self
            .prompt
            .as_ref()
            .is_some_and(|prompt| prompt.id == front.id)
        {
            return;
        }
        let input = cx.new(|cx| {
            let mut input = Input::new(message.clone(), cx)
                .placeholder(placeholder.clone().unwrap_or_default());
            if let Some(default) = default {
                input.set_text(default.clone(), cx);
            }
            input
        });
        self.prompt = Some(Prompt {
            id: front.id,
            input,
        });
    }
}

impl Render for ScriptDialogs {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(front) = self.queue.front() {
            self.last = Some(front.request.clone());
            if self.focused != Some(front.id) {
                self.focused = Some(front.id);
                self.scroll = ScrollHandle::new();
                window.defer(cx, |window, _| window.refresh());
                let target = match &self.prompt {
                    Some(prompt) if prompt.id == front.id => prompt.input.focus_handle(cx),
                    _ => self.focus.clone(),
                };
                // The nested defer runs after the Dialog's own initial focus, which would pick the Stop button.
                window.defer(cx, move |window, cx| {
                    window.defer(cx, move |window, cx| window.focus(&target, cx));
                });
            }
        }
        let Some(request) = &self.last else {
            return div().into_any_element();
        };
        let Content {
            title,
            message,
            cancel,
            accept,
        } = Content::of(request, &self.script);
        let theme = cx.theme();
        let scroller = div()
            .id("script-dialog-message")
            .min_h(LINE_BODY * 2.0)
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .when(self.scroll.max_offset().y > px(0.0), |scroller| {
                scroller.pr(Scrollbar::GUTTER)
            })
            .text_size(TEXT_BODY)
            .line_height(LINE_BODY)
            .text_color(theme.text_secondary)
            .child(message);
        let prompt = match (&self.prompt, request) {
            (Some(prompt), DialogRequest::Prompt { .. }) => Some(prompt.input.clone()),
            _ => None,
        };
        let enter = || {
            cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key != "enter" {
                    return;
                }
                cx.stop_propagation();
                if !event.is_held && !event.keystroke.modifiers.modified() {
                    this.answer(true, By::Key, cx);
                }
            })
        };
        let scroller = match prompt {
            Some(_) => scroller,
            None => scroller.track_focus(&self.focus).on_key_down(enter()),
        };
        let prompt = prompt.map(|input| div().on_key_down(enter()).child(input));
        let scrolled = -self.scroll.offset().y;
        let hidden_below = self.scroll.max_offset().y - scrolled;
        let fade = |toward_top: bool| {
            let (from, to) = if toward_top {
                (theme.overlay, theme.overlay.opacity(0.0))
            } else {
                (theme.overlay.opacity(0.0), theme.overlay)
            };
            div()
                .absolute()
                .left_0()
                .right_0()
                .h(LINE_BODY)
                .bg(linear_gradient(
                    180.0,
                    linear_color_stop(from, 0.0),
                    linear_color_stop(to, 1.0),
                ))
        };
        let message = div()
            .relative()
            .flex()
            .flex_col()
            .min_h_0()
            .child(scroller)
            .when(scrolled > px(0.5), |message| {
                message.child(
                    fade(true)
                        .top_0()
                        .debug_selector(|| "script-dialog-fade-top".into()),
                )
            })
            .when(hidden_below > px(0.5), |message| {
                message.child(
                    fade(false)
                        .bottom_0()
                        .debug_selector(|| "script-dialog-fade-bottom".into()),
                )
            })
            .child(Scrollbar::new(
                "script-dialog-scrollbar",
                self.scroll.clone(),
            ));
        let stop = self.stop.clone();
        let actions = div()
            .flex()
            .flex_wrap()
            .items_center()
            .justify_between()
            .gap(px(8.0))
            .pt(px(4.0))
            .child(
                div().ml(-STOP_INSET).child(
                    Button::new("script-dialog-stop", "Stop script")
                        .variant(Variant::Ghost)
                        .on_click(move |_, _, cx| stop(cx)),
                ),
            )
            .child(
                div()
                    .ml_auto()
                    .flex()
                    .flex_wrap()
                    .justify_end()
                    .gap(px(8.0))
                    .when_some(cancel, |answers, cancel| {
                        answers.child(Button::new("script-dialog-cancel", cancel).on_click(
                            cx.listener(|this, event: &ClickEvent, _, cx| {
                                if event.click_count() == 1 {
                                    this.answer(false, By::Pointer, cx);
                                }
                            }),
                        ))
                    })
                    .child(
                        Button::new("script-dialog-accept", accept)
                            .variant(Variant::Primary)
                            .on_click(cx.listener(|this, event: &ClickEvent, _, cx| {
                                if event.click_count() == 1 {
                                    this.answer(true, By::Pointer, cx);
                                }
                            })),
                    ),
            );
        let body = div()
            .flex()
            .flex_col()
            .min_h_0()
            .gap(px(12.0))
            .debug_selector(|| "script-dialog-body".into())
            .child(message)
            .children(prompt)
            .child(actions);
        let width = WIDTH.min(window.viewport_size().width - WINDOW_MARGIN * 2.0);
        let this = cx.weak_entity();
        let dialog = Dialog::new("script-dialog", self.waiting(), title)
            .alert()
            .fit_window()
            .width(width)
            .body(body)
            .on_close(move |_, cx| {
                let _ = this.update(cx, |this, cx| this.answer(false, By::Key, cx));
            });
        div()
            .key_context(CONTEXT)
            .capture_key_down(|event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" && event.is_held {
                    cx.stop_propagation();
                }
            })
            .on_key_up(cx.listener(|this, event: &gpui::KeyUpEvent, _, _| {
                if matches!(event.keystroke.key.as_str(), "enter" | "escape") {
                    this.armed = true;
                }
            }))
            .child(dialog)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, rc::Rc};

    use gpui::{
        Bounds, KeyUpEvent, Keystroke, Modifiers, Pixels, TestAppContext, VisualTestContext, point,
    };
    use mira_ui::{FOCUS_SCOPE, theme::SYSTEM_SANS};
    use serde_json::{Value as Json, json};

    use super::*;

    struct Host {
        focus: FocusHandle,
        dialogs: Entity<ScriptDialogs>,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let theme = cx.theme();
            div()
                .key_context(FOCUS_SCOPE)
                .track_focus(&self.focus)
                .size_full()
                .bg(theme.base)
                .text_color(theme.text)
                .font_family(SYSTEM_SANS)
                .text_size(TEXT_BODY)
                .child(self.dialogs.clone())
        }
    }

    struct Asks {
        sender: async_channel::Sender<ToDialogs>,
        next: u64,
    }

    impl Asks {
        fn open(&mut self, request: DialogRequest) -> (CallId, impl Fn() -> Option<Json> + use<>) {
            self.next += 1;
            let (dialog, answer) = Asked::test(self.next, request);
            let id = dialog.id;
            let _ = self.sender.try_send(ToDialogs::Open(dialog));
            (id, answer)
        }

        fn close(&self, id: CallId) {
            self.sender.try_send(ToDialogs::Close(id)).unwrap();
        }
    }

    fn host(
        cx: &mut TestAppContext,
    ) -> (&mut VisualTestContext, Asks, Entity<Host>, Rc<Cell<bool>>) {
        cx.update(|cx| {
            mira_ui::init(cx);
            bind_keys(cx);
        });
        let (sender, asked) = async_channel::unbounded();
        let stopped = Rc::new(Cell::new(false));
        let stop = stopped.clone();
        let (host, cx) = cx.add_window_view(|window, cx| {
            let focus = cx.focus_handle();
            window.activate_window();
            window.focus(&focus, cx);
            let dialogs = cx.new(|cx| {
                let mut dialogs = ScriptDialogs::new(move |_| stop.set(true), cx);
                dialogs.start("hunt.ts".into(), asked, cx);
                dialogs
            });
            Host { focus, dialogs }
        });
        (cx, Asks { sender, next: 0 }, host, stopped)
    }

    fn settle(cx: &mut VisualTestContext) {
        cx.run_until_parked();
    }

    fn press(cx: &mut VisualTestContext, key: &'static str) {
        cx.simulate_keystrokes(key);
        cx.simulate_event(KeyUpEvent {
            keystroke: Keystroke::parse(key).unwrap(),
        });
        settle(cx);
    }

    fn hold(cx: &mut VisualTestContext, key: &'static str) {
        cx.simulate_keystrokes(key);
        for _ in 0..5 {
            cx.simulate_event(KeyDownEvent {
                keystroke: Keystroke::parse(key).unwrap(),
                is_held: true,
                prefer_character_input: false,
            });
            settle(cx);
        }
        cx.simulate_event(KeyUpEvent {
            keystroke: Keystroke::parse(key).unwrap(),
        });
        settle(cx);
    }

    fn host_focused(cx: &mut VisualTestContext, host: &Entity<Host>) -> bool {
        let focus = host.read_with(cx, |host, _| host.focus.clone());
        cx.update(|window, _| focus.is_focused(window))
    }

    fn confirm(message: &str) -> DialogRequest {
        DialogRequest::Confirm {
            message: message.into(),
            title: None,
            confirm: None,
            cancel: None,
        }
    }

    fn prompt(message: &str, default: Option<&str>) -> DialogRequest {
        DialogRequest::Prompt {
            message: message.into(),
            title: None,
            default: default.map(Into::into),
            placeholder: None,
        }
    }

    #[gpui::test]
    fn enter_confirms_and_escape_cancels_without_stopping_the_script(cx: &mut TestAppContext) {
        let (cx, mut asks, _, stopped) = host(cx);
        let (_, deposit) = asks.open(confirm("Deposit?"));
        let (_, sell) = asks.open(confirm("Sell?"));
        settle(cx);
        press(cx, "enter");
        assert_eq!(deposit(), Some(json!(true)));
        assert_eq!(sell(), None);
        press(cx, "escape");
        assert_eq!(sell(), Some(json!(false)));
        assert!(!stopped.get());
    }

    #[gpui::test]
    fn a_prompt_returns_what_was_typed_and_the_next_one_starts_with_its_default(
        cx: &mut TestAppContext,
    ) {
        let (cx, mut asks, _, _) = host(cx);
        let (_, monster) = asks.open(prompt("Monster?", None));
        let (_, kills) = asks.open(prompt("Kills?", Some("100")));
        settle(cx);
        cx.simulate_input("Frogzard");
        press(cx, "enter");
        assert_eq!(monster(), Some(json!("Frogzard")));
        cx.simulate_input("0");
        press(cx, "enter");
        assert_eq!(kills(), Some(json!("1000")));
    }

    #[gpui::test]
    fn escape_cancels_a_prompt_while_typing_in_it(cx: &mut TestAppContext) {
        let (cx, mut asks, _, _) = host(cx);
        let (_, kills) = asks.open(prompt("Kills?", Some("100")));
        settle(cx);
        cx.simulate_input("abc");
        press(cx, "escape");
        assert_eq!(kills(), Some(Json::Null));
    }

    #[gpui::test]
    fn focus_goes_back_to_the_game_when_the_last_dialog_closes_or_the_script_ends(
        cx: &mut TestAppContext,
    ) {
        let (cx, mut asks, host, _) = host(cx);
        let _ = asks.open(confirm("Deposit?"));
        settle(cx);
        assert!(!host_focused(cx, &host));
        press(cx, "enter");
        assert!(host_focused(cx, &host));

        let (_, kills) = asks.open(prompt("Kills?", None));
        settle(cx);
        assert!(!host_focused(cx, &host));
        let dialogs = host.read_with(cx, |host, _| host.dialogs.clone());
        dialogs.update(cx, |dialogs, cx| dialogs.end(cx));
        settle(cx);
        assert!(host_focused(cx, &host));
        assert_eq!(kills(), None);
        let _ = asks.open(confirm("After the end"));
        settle(cx);
        assert!(host_focused(cx, &host));
    }

    #[gpui::test]
    fn closing_the_dialog_in_front_hands_focus_to_the_next_one(cx: &mut TestAppContext) {
        let (cx, mut asks, _, _) = host(cx);
        let (monster_id, monster) = asks.open(prompt("Monster?", None));
        let (_, kills) = asks.open(prompt("Kills?", None));
        settle(cx);
        cx.simulate_input("half typed");
        asks.close(monster_id);
        settle(cx);
        cx.simulate_input("50");
        press(cx, "enter");
        assert_eq!(monster(), None);
        assert_eq!(kills(), Some(json!("50")));
    }

    #[gpui::test]
    fn a_held_key_answers_only_the_dialog_that_was_in_front_when_it_went_down(
        cx: &mut TestAppContext,
    ) {
        let (cx, mut asks, _, _) = host(cx);
        let (_, deposit) = asks.open(confirm("Deposit?"));
        let (_, sell) = asks.open(confirm("Sell everything?"));
        let (_, kills) = asks.open(prompt("Kills?", Some("100")));
        let (_, quit) = asks.open(confirm("Quit?"));
        settle(cx);
        hold(cx, "enter");
        assert_eq!(deposit(), Some(json!(true)));
        assert_eq!(sell(), None);
        hold(cx, "escape");
        assert_eq!(sell(), Some(json!(false)));
        assert_eq!(kills(), None);
        hold(cx, "enter");
        assert_eq!(kills(), Some(json!("100")));
        assert_eq!(quit(), None);
        press(cx, "enter");
        assert_eq!(quit(), Some(json!(true)));
    }

    #[gpui::test]
    fn the_first_dialog_after_an_empty_queue_takes_enter_right_away(cx: &mut TestAppContext) {
        let (cx, mut asks, _, _) = host(cx);
        let (_, first) = asks.open(confirm("First?"));
        settle(cx);
        cx.simulate_keystrokes("enter");
        settle(cx);
        assert_eq!(first(), Some(json!(true)));
        let (_, second) = asks.open(confirm("Second?"));
        settle(cx);
        press(cx, "enter");
        assert_eq!(second(), Some(json!(true)));
    }

    fn held(cx: &mut VisualTestContext, key: &'static str) {
        for _ in 0..3 {
            cx.simulate_event(KeyDownEvent {
                keystroke: Keystroke::parse(key).unwrap(),
                is_held: true,
                prefer_character_input: false,
            });
            settle(cx);
        }
        cx.simulate_event(KeyUpEvent {
            keystroke: Keystroke::parse(key).unwrap(),
        });
        settle(cx);
    }

    #[gpui::test]
    fn a_key_already_held_when_a_dialog_opens_does_not_answer_it(cx: &mut TestAppContext) {
        let (cx, mut asks, _, _) = host(cx);
        let (_, kills) = asks.open(prompt("Kills?", Some("100")));
        settle(cx);
        held(cx, "enter");
        assert_eq!(kills(), None);
        held(cx, "escape");
        assert_eq!(kills(), None);
        press(cx, "enter");
        assert_eq!(kills(), Some(json!("100")));

        let (_, sell) = asks.open(confirm("Sell everything?"));
        settle(cx);
        held(cx, "escape");
        held(cx, "enter");
        assert_eq!(sell(), None);
        press(cx, "escape");
        assert_eq!(sell(), Some(json!(false)));
    }

    #[gpui::test]
    fn enter_with_a_modifier_answers_nothing(cx: &mut TestAppContext) {
        let (cx, mut asks, _, _) = host(cx);
        let (_, deposit) = asks.open(confirm("Deposit?"));
        let (_, kills) = asks.open(prompt("Kills?", Some("100")));
        settle(cx);
        for keys in ["cmd-enter", "shift-enter", "ctrl-enter", "alt-enter"] {
            press(cx, keys);
        }
        assert_eq!(deposit(), None);
        press(cx, "enter");
        for keys in ["cmd-enter", "shift-enter"] {
            press(cx, keys);
        }
        assert_eq!(deposit(), Some(json!(true)));
        assert_eq!(kills(), None);
    }

    #[gpui::test]
    fn a_double_click_answers_only_the_dialog_under_the_first_click(cx: &mut TestAppContext) {
        let (cx, mut asks, _, _) = host(cx);
        let (_, deposit) = asks.open(confirm("Deposit?"));
        let (_, sell) = asks.open(confirm("Sell everything?"));
        settle(cx);
        let body = cx.debug_bounds("script-dialog-body").unwrap();
        let accept = body.bottom_right() - point(px(30.0), px(14.0));
        for click_count in [1, 2] {
            cx.simulate_event(gpui::MouseDownEvent {
                button: gpui::MouseButton::Left,
                position: accept,
                modifiers: Modifiers::none(),
                click_count,
                first_mouse: false,
            });
            cx.simulate_event(gpui::MouseUpEvent {
                button: gpui::MouseButton::Left,
                position: accept,
                modifiers: Modifiers::none(),
                click_count,
            });
            settle(cx);
        }
        assert_eq!(deposit(), Some(json!(true)));
        assert_eq!(sell(), None);
    }

    #[gpui::test]
    fn a_new_run_in_the_same_tab_gives_focus_back_to_the_game(cx: &mut TestAppContext) {
        let (cx, mut asks, host, _) = host(cx);
        let _ = asks.open(confirm("Deposit?"));
        settle(cx);
        assert!(!host_focused(cx, &host));
        let dialogs = host.read_with(cx, |host, _| host.dialogs.clone());
        let (sender, asked) = async_channel::unbounded();
        dialogs.update(cx, |dialogs, cx| {
            dialogs.end(cx);
            dialogs.start("next.ts".into(), asked, cx);
        });
        settle(cx);
        assert!(host_focused(cx, &host));
        let mut next = Asks { sender, next: 0 };
        let (_, again) = next.open(confirm("Deposit again?"));
        settle(cx);
        press(cx, "enter");
        assert_eq!(again(), Some(json!(true)));
    }

    #[gpui::test]
    fn a_long_message_waiting_in_the_queue_gets_its_fade_when_it_comes_up(cx: &mut TestAppContext) {
        let (cx, mut asks, _, _) = host(cx);
        let lines = vec!["A line of a very long message."; 400].join("\n");
        let _ = asks.open(confirm("Short?"));
        let _ = asks.open(confirm(&lines));
        settle(cx);
        press(cx, "enter");
        assert!(cx.debug_bounds("script-dialog-fade-bottom").is_some());
    }

    #[gpui::test]
    fn clicking_outside_the_dialog_answers_nothing(cx: &mut TestAppContext) {
        let (cx, mut asks, _, stopped) = host(cx);
        let (_, deposit) = asks.open(confirm("Deposit?"));
        settle(cx);
        cx.simulate_click(point(px(2.0), px(2.0)), Modifiers::none());
        settle(cx);
        assert_eq!(deposit(), None);
        assert!(!stopped.get());
    }

    #[gpui::test]
    fn a_huge_message_keeps_the_dialog_inside_the_window(cx: &mut TestAppContext) {
        let (cx, mut asks, _, _) = host(cx);
        let lines = vec!["A line of a very long message."; 400].join("\n");
        let word = "W".repeat(5_000);
        let crowded = DialogRequest::Confirm {
            message: lines.clone(),
            title: Some("A very long title ".repeat(40)),
            confirm: Some("Deposit every single AC item".into()),
            cancel: Some("Keep everything for later".into()),
        };
        for request in [confirm(&lines), confirm(&word), crowded] {
            let (_, answer) = asks.open(request);
            settle(cx);
            let viewport = cx.update(|window, _| window.viewport_size());
            let body: Bounds<Pixels> = cx.debug_bounds("script-dialog-body").unwrap();
            assert!(
                body.top() >= px(0.0)
                    && body.bottom() <= viewport.height - px(16.0)
                    && body.left() >= px(0.0)
                    && body.right() <= viewport.width,
                "{body:?} in {viewport:?}"
            );
            press(cx, "enter");
            assert_eq!(answer(), Some(json!(true)));
        }
    }

    #[gpui::test]
    fn the_fades_follow_the_message_as_it_scrolls(cx: &mut TestAppContext) {
        let (cx, mut asks, _, _) = host(cx);
        let lines = vec!["A line of a very long message."; 400].join("\n");
        let _ = asks.open(confirm(&lines));
        settle(cx);
        let shown = |cx: &mut VisualTestContext| {
            (
                cx.debug_bounds("script-dialog-fade-top").is_some(),
                cx.debug_bounds("script-dialog-fade-bottom").is_some(),
            )
        };
        assert_eq!(shown(cx), (false, true));
        let body = cx.debug_bounds("script-dialog-body").unwrap();
        let wheel = |cx: &mut VisualTestContext, by: f32| {
            cx.simulate_event(gpui::ScrollEvent {
                position: body.origin + point(px(20.0), px(20.0)),
                delta: gpui::ScrollDelta::Pixels(point(px(0.0), px(by))),
                ..Default::default()
            });
            settle(cx);
        };
        wheel(cx, -200.0);
        assert_eq!(shown(cx), (true, true));
        wheel(cx, -100_000.0);
        assert_eq!(shown(cx), (true, false));
        wheel(cx, 100_000.0);
        assert_eq!(shown(cx), (false, true));
    }

    #[gpui::test]
    fn a_full_queue_closed_out_of_order_leaves_no_dialog_behind(cx: &mut TestAppContext) {
        let (cx, mut asks, host, _) = host(cx);
        let opened: Vec<_> = (0..100)
            .map(|i| asks.open(prompt(&format!("Prompt {i}"), None)))
            .collect();
        settle(cx);
        for step in 0..100 {
            asks.close(opened[(step * 37) % 100].0);
        }
        settle(cx);
        assert!(opened.iter().all(|(_, answer)| answer().is_none()));
        assert!(host_focused(cx, &host));
        let (_, last) = asks.open(confirm("Still works?"));
        settle(cx);
        press(cx, "enter");
        assert_eq!(last(), Some(json!(true)));
    }

    #[test]
    fn blank_titles_and_labels_fall_back_to_the_defaults() {
        let content = Content::of(
            &DialogRequest::Confirm {
                message: String::new(),
                title: Some("  ".into()),
                confirm: Some(String::new()),
                cancel: Some(" ".into()),
            },
            &"hunt.ts".into(),
        );
        assert_eq!(
            content,
            Content {
                title: "hunt.ts".into(),
                message: "".into(),
                cancel: Some("Cancel".into()),
                accept: "Confirm".into(),
            }
        );
    }

    #[test]
    fn labels_longer_than_a_button_end_in_an_ellipsis() {
        let content = Content::of(
            &DialogRequest::Confirm {
                message: String::new(),
                title: None,
                confirm: Some("Deposit every single AC item into the bank".into()),
                cancel: Some("Exactly twenty-four char".into()),
            },
            &"hunt.ts".into(),
        );
        assert_eq!(content.accept, "Deposit every single AC…");
        assert_eq!(content.cancel.as_deref(), Some("Exactly twenty-four char"));
    }

    fn snapshot(name: &str, width: f32, height: f32, requests: &[DialogRequest]) {
        let requests = requests.to_vec();
        let paths = mira_ui::snapshot(
            &format!("script-dialog-{name}"),
            gpui::size(px(width), px(height)),
            move |window, cx| {
                let requests = requests.clone();
                cx.new(|cx| {
                    let (sender, asked) = async_channel::unbounded();
                    for (id, request) in (1..).zip(requests) {
                        let (dialog, _) = Asked::test(id, request);
                        sender.try_send(ToDialogs::Open(dialog)).unwrap();
                    }
                    let focus = cx.focus_handle();
                    window.focus(&focus, cx);
                    let dialogs = cx.new(|cx| {
                        let mut dialogs = ScriptDialogs::new(|_| {}, cx);
                        dialogs.start("hunt.ts".into(), asked, cx);
                        dialogs
                    });
                    Host { focus, dialogs }
                })
            },
        );
        for path in paths {
            assert!(path.exists(), "{} was not written", path.display());
        }
    }

    #[test]
    fn snapshot_script_dialogs() {
        let long_message = (1..=400)
            .map(|line| format!("Line {line} of a message that keeps going."))
            .collect::<Vec<_>>()
            .join("\n");
        let scenes: [(&str, f32, f32, DialogRequest); 10] = [
            (
                "confirm",
                640.0,
                400.0,
                DialogRequest::Confirm {
                    message: "Your bank is full. Deposit AC items?".into(),
                    title: Some("Bank".into()),
                    confirm: Some("Deposit".into()),
                    cancel: Some("Skip".into()),
                },
            ),
            (
                "alert-lines",
                640.0,
                400.0,
                DialogRequest::Alert {
                    message: "Farming finished.\n\nKills: 1,000\nDrops: 42\n\nThe bank is nearly full."
                        .into(),
                    title: None,
                },
            ),
            ("prompt", 640.0, 400.0, prompt("How many kills?", Some("100"))),
            (
                "prompt-placeholder",
                640.0,
                400.0,
                DialogRequest::Prompt {
                    message: "Which monster?".into(),
                    title: Some("Hunt".into()),
                    default: None,
                    placeholder: Some("Frogzard".into()),
                },
            ),
            ("long-message", 900.0, 600.0, confirm(&long_message)),
            ("unbroken-word", 640.0, 400.0, confirm(&"W".repeat(5_000))),
            (
                "long-labels",
                640.0,
                400.0,
                DialogRequest::Confirm {
                    message: "Labels and titles longer than the dialog is wide.".into(),
                    title: Some("A title that goes on far longer than any dialog title should ever need to go".into()),
                    confirm: Some("Deposit every single AC item into the bank right now".into()),
                    cancel: Some("Keep everything in the inventory for later".into()),
                },
            ),
            (
                "long-default",
                640.0,
                400.0,
                prompt("Paste a long value", Some(&"abcdefghij".repeat(30))),
            ),
            ("small-window", 360.0, 260.0, confirm(&long_message)),
            (
                "tall-everything",
                480.0,
                420.0,
                DialogRequest::Confirm {
                    message: long_message.clone(),
                    title: Some("A very long title ".repeat(40)),
                    confirm: Some("Deposit every single AC item".into()),
                    cancel: Some("Keep everything for later".into()),
                },
            ),
        ];
        for (name, width, height, request) in scenes {
            snapshot(name, width, height, &[request]);
        }
    }
}
