use std::{cell::Cell, rc::Rc};

use gpui::{
    AnyElement, App, ElementId, InteractiveElement as _, IntoElement, MouseButton,
    ParentElement as _, Pixels, RenderOnce, Role, SharedString, StatefulInteractiveElement as _,
    Styled as _, Window, div, prelude::FluentBuilder as _, px,
};

use super::{Icon, icon, menu};
use crate::{ActiveTheme as _, motion, nav};

type Content = Rc<dyn Fn(&mut Window, &mut App) -> AnyElement>;

#[derive(IntoElement)]
pub struct Popover {
    id: ElementId,
    label: SharedString,
    content: Content,
    align: menu::Align,
    width: Pixels,
}

impl Popover {
    pub fn new(
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        content: impl Fn(&mut Window, &mut App) -> AnyElement + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            content: Rc::new(content),
            align: menu::Align::Start,
            width: px(280.0),
        }
    }

    pub fn width(mut self, width: Pixels) -> Self {
        self.width = width;
        self
    }

    pub fn align(mut self, align: menu::Align) -> Self {
        self.align = align;
        self
    }
}

impl RenderOnce for Popover {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = menu::state(&self.id, window, cx);
        let trigger_focus = nav::focus_handle((self.id.clone(), "trigger"), window, cx);
        let ring = nav::keyboard_focused(&trigger_focus, window, cx);
        let (is_open, panel_focus) = {
            let state = state.read(cx);
            (state.is_open(), state.focus())
        };
        let was_open = window
            .use_keyed_state((self.id.clone(), "was-open"), cx, |_, _| {
                Rc::new(Cell::new(false))
            })
            .read(cx)
            .clone();
        if was_open.replace(is_open) != is_open && is_open {
            let panel_focus = panel_focus.clone();
            window.defer(cx, move |window, cx| {
                nav::focus_first(&panel_focus, window, cx)
            });
        }
        let theme = cx.theme().clone();
        let close_to_trigger = {
            let trigger_focus = trigger_focus.clone();
            move |state: &gpui::Entity<menu::MenuState>, window: &mut Window, cx: &mut App| {
                menu::close(state, false, window, cx);
                window.focus(&trigger_focus, cx);
            }
        };
        let toggle = {
            let state = state.clone();
            let close_to_trigger = close_to_trigger.clone();
            move |window: &mut Window, cx: &mut App| {
                if state.read(cx).is_open() {
                    close_to_trigger(&state, window, cx);
                } else {
                    menu::open(&state, None, None, window, cx);
                }
            }
        };
        div()
            .relative()
            .flex_none()
            .child(
                menu::trigger_style(
                    div()
                        .id(self.id.clone())
                        .role(Role::Button)
                        .aria_label(self.label.clone())
                        .aria_expanded(is_open)
                        .track_focus(&trigger_focus.tab_stop(true)),
                    menu::trigger_face(is_open, &theme).render(
                        (self.id.clone(), "face"),
                        window,
                        cx,
                    ),
                    &theme,
                    ring,
                )
                .on_mouse_down(MouseButton::Left, {
                    let toggle = toggle.clone();
                    move |_, window, cx| {
                        toggle(window, cx);
                        cx.stop_propagation();
                    }
                })
                .on_key_down(move |event, window, cx| {
                    if matches!(event.keystroke.key.as_str(), "enter" | "space" | "down") {
                        toggle(window, cx);
                        cx.stop_propagation();
                    }
                })
                .child(self.label.clone())
                .child(icon(Icon::ChevronDown, theme.text_tertiary))
                .child(menu::trigger_probe(&state)),
            )
            .when_some(menu::presence(&state, window, cx), |container, shown| {
                let content = (self.content)(window, cx);
                let key_state = state.clone();
                let out_state = state.clone();
                container.child(menu::attach(self.align, &state, window, cx, |from, _| {
                    menu::fading(
                        motion::arrive(
                            (self.id.clone(), "popover"),
                            div()
                                .id((self.id.clone(), "panel"))
                                .role(Role::Dialog)
                                .aria_label(self.label.clone())
                                .occlude()
                                .track_focus(&panel_focus)
                                .on_key_down(move |event, window, cx| {
                                    if event.keystroke.key == "escape" {
                                        close_to_trigger(&key_state, window, cx);
                                        cx.stop_propagation();
                                    }
                                })
                                .on_mouse_down_out(move |event, window, cx| {
                                    if menu::outside(&out_state, event.position, cx) {
                                        menu::close(&out_state, false, window, cx)
                                    }
                                })
                                .w(self.width)
                                .p(px(16.0))
                                .rounded(theme.radius.panel)
                                .bg(theme.overlay)
                                .border_1()
                                .border_color(theme.border)
                                .shadow(theme.floating())
                                .child(content),
                            from,
                        ),
                        shown,
                    )
                }))
            })
    }
}
