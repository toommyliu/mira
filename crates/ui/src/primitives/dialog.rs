use std::rc::Rc;

use gpui::{
    AnyElement, App, Context, ElementId, FocusHandle, FontWeight, InteractiveElement as _,
    IntoElement, MouseButton, ParentElement as _, Pixels, RenderOnce, Role, ScrollHandle,
    SharedString, StatefulInteractiveElement as _, Styled as _, Window, anchored, deferred, div,
    prelude::FluentBuilder as _, px,
};

use super::{Button, Icon, Scrollbar, button::Size};
use crate::{
    ActiveTheme as _, FOCUS_SCOPE, FocusNext, FocusPrevious, h_flex, motion, nav, theme::RING_ROOM,
};

type CloseHandler = Rc<dyn Fn(&mut Window, &mut App)>;

pub(crate) struct DialogState {
    was_open: bool,
    closed_at: Option<std::time::Instant>,
    return_focus: Option<FocusHandle>,
    focus: FocusHandle,
    footer_start: FocusHandle,
    actions: FocusHandle,
    scroll: ScrollHandle,
}

impl DialogState {
    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            was_open: false,
            closed_at: None,
            return_focus: None,
            focus: cx.focus_handle(),
            footer_start: cx.focus_handle(),
            actions: cx.focus_handle(),
            scroll: ScrollHandle::new(),
        }
    }
}

#[derive(IntoElement)]
pub struct Dialog {
    id: ElementId,
    open: bool,
    alert: bool,
    title: SharedString,
    width: Option<Pixels>,
    description: Option<SharedString>,
    header_end: Option<AnyElement>,
    body: Vec<AnyElement>,
    footer_start: Option<AnyElement>,
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
            width: None,
            description: None,
            header_end: None,
            body: Vec::new(),
            footer_start: None,
            actions: Vec::new(),
            on_close: None,
        }
    }

    pub fn alert(mut self) -> Self {
        self.alert = true;
        self
    }

    pub fn width(mut self, width: Pixels) -> Self {
        self.width = Some(width);
        self
    }

    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn header_end_slot(mut self, element: impl IntoElement) -> Self {
        self.header_end = Some(element.into_any_element());
        self
    }

    pub fn body(mut self, body: impl IntoElement) -> Self {
        self.body.push(body.into_any_element());
        self
    }

    pub fn footer_start_slot(mut self, element: impl IntoElement) -> Self {
        self.footer_start = Some(element.into_any_element());
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
        let layout = cx.theme().layout;
        let typography = cx.theme().typography;
        let state = window.use_keyed_state((self.id.clone(), "dialog"), cx, |_, cx| {
            DialogState::new(cx)
        });
        let (was_open, focus, footer_start, actions, scroll) = {
            let state = state.read(cx);
            (
                state.was_open,
                state.focus.clone(),
                state.footer_start.clone(),
                state.actions.clone(),
                state.scroll.clone(),
            )
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
            let (footer_start, actions) = (footer_start.clone(), actions.clone());
            window.defer(cx, move |window, cx| {
                if opening {
                    nav::focus_first(&focus, window, cx);
                    if footer_start.contains_focused(window, cx) {
                        nav::focus_first(&actions, window, cx);
                    }
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
        let close_size = Size::Small;
        let backdrop_close = dismiss.clone();
        let close_focus = nav::focus_handle((self.id.clone(), "close"), window, cx).tab_index(1);
        let close_id = self.id.clone();
        let next_focus = focus.clone();
        let prev_focus = focus.clone();
        let viewport = window.viewport_size();
        let inset = layout.space(4.0);
        let narrow = viewport.width < layout.compact_overlay;
        let stack_actions = narrow && !self.alert;
        let equal_choices = narrow && self.alert && self.actions.len() == 2;
        if self.open {
            nav::set_scroll_root(&scroll, window, cx);
        }
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
                        .bg(gpui::black().opacity(0.8))
                        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                            if let Some(close) = &backdrop_close {
                                close(window, cx);
                            }
                        })
                        .child(motion::fade(
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
                                .key_context(format!("{FOCUS_SCOPE} Modal").as_str())
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
                                .gap(theme.space(if self.alert { 3.0 } else { 4.0 }))
                                .w(self.width.unwrap_or_else(|| {
                                    if !narrow {
                                        layout.overlay_max_width
                                    } else if self.alert {
                                        px(320.0)
                                    } else {
                                        viewport.width - inset * 2.0
                                    }
                                }))
                                .max_w(viewport.width - inset * 2.0)
                                .max_h(viewport.height - inset * 2.0)
                                .p(inset)
                                .rounded(theme.radius.dialog)
                                .bg(theme.popover)
                                .text_color(theme.popover_foreground)
                                .shadow(theme.surface_edge())
                                .child(
                                    div()
                                        .flex()
                                        .flex_none()
                                        .flex_col()
                                        .gap(layout.space(0.5))
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .justify_between()
                                                .gap(layout.space(3.0))
                                                .child(
                                                    div()
                                                        .id((self.id.clone(), "title"))
                                                        .role(Role::Heading)
                                                        .aria_level(2)
                                                        .min_w_0()
                                                        .text_size(typography.body)
                                                        .line_height(typography.body_line)
                                                        .font_weight(FontWeight::MEDIUM)
                                                        .child(self.title),
                                                )
                                                .when_some(self.header_end, |row, end| {
                                                    row.child(div().flex_1().min_w_0().child(end))
                                                })
                                                .when_some(dismiss, |row, dismiss| {
                                                    row.child(
                                                        Button::icon(
                                                            (close_id, "close"),
                                                            Icon::X,
                                                            "Close",
                                                        )
                                                        .size(close_size)
                                                        .focus(close_focus)
                                                        .on_click(move |_, window, cx| {
                                                            dismiss(window, cx)
                                                        }),
                                                    )
                                                }),
                                        )
                                        .when_some(self.description, |header, description| {
                                            header.child(
                                                div()
                                                    .text_size(typography.body)
                                                    .line_height(typography.body_line)
                                                    .text_color(theme.muted_foreground)
                                                    .child(description),
                                            )
                                        }),
                                )
                                .when(!self.body.is_empty(), |panel| {
                                    panel.child(
                                        div()
                                            .relative()
                                            .flex()
                                            .flex_col()
                                            .min_h_0()
                                            .mx(-inset)
                                            .my(-RING_ROOM)
                                            .child(
                                                div()
                                                    .id((self.id.clone(), "body"))
                                                    .flex()
                                                    .flex_col()
                                                    .gap(layout.space(3.0))
                                                    .min_h_0()
                                                    .px(inset)
                                                    .py(RING_ROOM)
                                                    .overflow_y_scroll()
                                                    .track_scroll(&scroll)
                                                    .children(self.body),
                                            )
                                            .child(Scrollbar::new(
                                                (self.id.clone(), "scrollbar"),
                                                scroll,
                                            )),
                                    )
                                })
                                .when(
                                    !self.actions.is_empty() || self.footer_start.is_some(),
                                    |panel| {
                                        panel.child(
                                            h_flex()
                                                .flex_none()
                                                .flex_wrap()
                                                .justify_end()
                                                .gap(theme.space(2.0))
                                                .pt(layout.space(1.0))
                                                .when(stack_actions, |footer| {
                                                    footer.flex_col_reverse().items_stretch()
                                                })
                                                .when_some(self.footer_start, |footer, start| {
                                                    footer.child(
                                                        h_flex()
                                                            .id((self.id.clone(), "footer-start"))
                                                            .track_focus(&footer_start)
                                                            .flex_grow(1.0)
                                                            .min_w_0()
                                                            .gap(theme.space(2.0))
                                                            .child(start),
                                                    )
                                                })
                                                .child(
                                                    h_flex()
                                                        .id((self.id.clone(), "actions"))
                                                        .track_focus(&actions)
                                                        .when(equal_choices, |group| group.w_full())
                                                        .flex_wrap()
                                                        .when(stack_actions, |group| {
                                                            group.flex_col_reverse().items_stretch()
                                                        })
                                                        .justify_end()
                                                        .gap(theme.space(2.0))
                                                        .children(self.actions.into_iter().map(
                                                            |action| {
                                                                if equal_choices {
                                                                    crate::v_flex()
                                                                        .flex_1()
                                                                        .min_w_0()
                                                                        .child(action)
                                                                        .into_any_element()
                                                                } else {
                                                                    action
                                                                }
                                                            },
                                                        )),
                                                ),
                                        )
                                    },
                                ),
                        )),
                )),
            ),
        )
        .with_priority(super::layer::DIALOG)
        .into_any_element()
    }
}
