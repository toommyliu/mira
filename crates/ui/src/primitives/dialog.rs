use std::rc::Rc;

use gpui::{
    AnyElement, App, Context, ElementId, FocusHandle, FontWeight, InteractiveElement as _,
    IntoElement, MouseButton, ParentElement as _, RenderOnce, Role, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window, anchored, deferred, div,
    prelude::FluentBuilder as _, px,
};

use super::{Button, Icon, button::Size};
use crate::{
    ActiveTheme as _, FOCUS_SCOPE, FocusNext, FocusPrevious,
    motion::{self, From},
    nav,
    theme::{LINE_BODY, LINE_TITLE, TEXT_BODY, TEXT_TITLE},
};

type CloseHandler = Rc<dyn Fn(&mut Window, &mut App)>;

pub(crate) struct DialogState {
    was_open: bool,
    closed_at: Option<std::time::Instant>,
    return_focus: Option<FocusHandle>,
    focus: FocusHandle,
}

impl DialogState {
    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            was_open: false,
            closed_at: None,
            return_focus: None,
            focus: cx.focus_handle(),
        }
    }
}

#[derive(IntoElement)]
pub struct Dialog {
    id: ElementId,
    open: bool,
    alert: bool,
    title: SharedString,
    description: Option<SharedString>,
    body: Option<AnyElement>,
    actions: Vec<AnyElement>,
    on_close: Option<CloseHandler>,
}

impl Dialog {
    pub fn new(id: impl Into<ElementId>, open: bool, title: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            open,
            alert: false,
            title: title.into(),
            description: None,
            body: None,
            actions: Vec::new(),
            on_close: None,
        }
    }

    pub fn alert(mut self) -> Self {
        self.alert = true;
        self
    }

    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn body(mut self, body: impl IntoElement) -> Self {
        self.body = Some(body.into_any_element());
        self
    }

    pub fn action(mut self, action: impl IntoElement) -> Self {
        self.actions.push(action.into_any_element());
        self
    }

    pub fn on_close(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_close = Some(Rc::new(f));
        self
    }
}

impl RenderOnce for Dialog {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = window.use_keyed_state((self.id.clone(), "dialog"), cx, |_, cx| {
            DialogState::new(cx)
        });
        let (was_open, focus) = {
            let state = state.read(cx);
            (state.was_open, state.focus.clone())
        };
        if self.open != was_open {
            let opening = self.open;
            let return_focus = opening.then(|| window.focused(cx)).flatten();
            let restore = state.update(cx, |state, _| {
                state.was_open = opening;
                state.closed_at = (!opening).then(std::time::Instant::now);
                if opening {
                    state.return_focus = return_focus;
                    None
                } else {
                    state.return_focus.take()
                }
            });
            let focus = focus.clone();
            window.defer(cx, move |window, cx| {
                if opening {
                    nav::focus_first(&focus, window, cx);
                } else if let Some(handle) = restore {
                    window.focus(&handle, cx);
                }
            });
        }
        let shown = if self.open {
            Some(1.0)
        } else {
            state
                .read(cx)
                .closed_at
                .and_then(|since| motion::leaving(since, window, cx))
        };
        let Some(shown) = shown else {
            return div().absolute().into_any_element();
        };
        let theme = cx.theme().clone();
        let close = self.on_close.clone();
        let dismiss = self.on_close.clone().filter(|_| !self.alert);
        let dismissable = dismiss.is_some();
        let backdrop_close = dismiss.clone();
        let close_id = self.id.clone();
        let next_focus = focus.clone();
        let prev_focus = focus.clone();
        let viewport = window.viewport_size();
        deferred(
            anchored().position(gpui::point(px(0.0), px(0.0))).child(
                div().opacity(shown).child(motion::fade(
                    (self.id.clone(), "backdrop"),
                    div()
                        .id((self.id.clone(), "backdrop"))
                        .occlude()
                        .w(viewport.width)
                        .h(viewport.height)
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(theme.backdrop)
                        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                            if let Some(close) = &backdrop_close {
                                close(window, cx);
                            }
                        })
                        .child(motion::arrive(
                            (self.id.clone(), "panel"),
                            div()
                                .id((self.id.clone(), "panel"))
                                .role(if self.alert {
                                    Role::AlertDialog
                                } else {
                                    Role::Dialog
                                })
                                .aria_label(self.title.clone())
                                .when_some(self.description.clone(), |panel, description| {
                                    panel.aria_description(description)
                                })
                                .key_context(FOCUS_SCOPE)
                                .track_focus(&focus)
                                .on_action(move |_: &FocusNext, window, cx| {
                                    nav::trap_tab(&next_focus, false, window, cx)
                                })
                                .on_action(move |_: &FocusPrevious, window, cx| {
                                    nav::trap_tab(&prev_focus, true, window, cx)
                                })
                                .on_key_down(move |event, window, cx| {
                                    if event.keystroke.key == "escape" {
                                        if let Some(close) = &close {
                                            close(window, cx);
                                        }
                                        cx.stop_propagation();
                                    }
                                })
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .relative()
                                .flex()
                                .flex_col()
                                .gap(px(16.0))
                                .w(px(420.0))
                                .p(px(20.0))
                                .rounded(theme.radius.dialog)
                                .bg(theme.overlay)
                                .border_1()
                                .border_color(theme.border)
                                .shadow(theme.floating())
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap(px(6.0))
                                        .when(dismissable, |header| header.pr(px(28.0)))
                                        .child(
                                            div()
                                                .id((self.id.clone(), "title"))
                                                .role(Role::Heading)
                                                .aria_level(2)
                                                .text_size(TEXT_TITLE)
                                                .line_height(LINE_TITLE)
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .child(self.title),
                                        )
                                        .when_some(self.description, |header, description| {
                                            header.child(
                                                div()
                                                    .text_size(TEXT_BODY)
                                                    .line_height(LINE_BODY)
                                                    .text_color(theme.text_secondary)
                                                    .child(description),
                                            )
                                        }),
                                )
                                .when_some(self.body, |panel, body| panel.child(body))
                                .when(!self.actions.is_empty(), |panel| {
                                    panel.child(
                                        div()
                                            .flex()
                                            .justify_end()
                                            .gap(px(8.0))
                                            .pt(px(4.0))
                                            .children(self.actions),
                                    )
                                })
                                .when_some(dismiss, |panel, dismiss| {
                                    panel.child(
                                        div().absolute().top(px(14.0)).right(px(14.0)).child(
                                            Button::icon((close_id, "close"), Icon::X, "Close")
                                                .size(Size::Small)
                                                .on_click(move |_, window, cx| dismiss(window, cx)),
                                        ),
                                    )
                                }),
                            From::Below,
                        )),
                )),
            ),
        )
        .with_priority(super::layer::DIALOG)
        .into_any_element()
    }
}
