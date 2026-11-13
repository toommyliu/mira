use std::{cell::Cell, rc::Rc};

use gpui::{
    AnyElement, App, ElementId, Entity, FocusHandle, InteractiveElement as _, IntoElement,
    ParentElement as _, Pixels, RenderOnce, Role, StatefulInteractiveElement as _, Styled as _,
    Window, div, prelude::FluentBuilder as _, px,
};

use super::{
    Button,
    menu::{self, MenuState},
};
use crate::{ActiveTheme as _, motion, nav};

type Content = Rc<dyn Fn(&PopoverClose, &mut Window, &mut App) -> AnyElement>;

const MIN_WIDTH: Pixels = px(200.0);
const MAX_WIDTH: Pixels = px(420.0);

#[derive(Clone)]
pub struct PopoverClose {
    state: Entity<MenuState>,
    trigger: FocusHandle,
}

impl PopoverClose {
    pub fn close(&self, window: &mut Window, cx: &mut App) {
        menu::close(&self.state, false, window, cx);
        window.focus(&self.trigger, cx);
    }
}

#[derive(IntoElement)]
pub struct Popover {
    id: ElementId,
    trigger: Option<Button>,
    content: Content,
    align: menu::Align,
    width: Option<Pixels>,
    default_open: bool,
}

impl Popover {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            trigger: None,
            content: Rc::new(|_, _, _| div().into_any_element()),
            align: menu::Align::Start,
            width: None,
            default_open: false,
        }
    }

    pub fn trigger(mut self, trigger: Button) -> Self {
        self.trigger = Some(trigger);
        self
    }

    pub fn content(
        mut self,
        f: impl Fn(&PopoverClose, &mut Window, &mut App) -> AnyElement + 'static,
    ) -> Self {
        self.content = Rc::new(f);
        self
    }

    pub fn width(mut self, width: Pixels) -> Self {
        self.width = Some(width);
        self
    }

    pub fn align(mut self, align: menu::Align) -> Self {
        self.align = align;
        self
    }

    pub fn default_open(mut self, open: bool) -> Self {
        self.default_open = open;
        self
    }
}

impl RenderOnce for Popover {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = menu::state(&self.id, self.default_open, window, cx);
        let trigger = self
            .trigger
            .unwrap_or_else(|| Button::new((self.id.clone(), "trigger"), "Open"));
        let label = trigger.label().clone();
        let trigger_focus = trigger.focus_handle(window, cx);
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
        let closer = PopoverClose {
            state: state.clone(),
            trigger: trigger_focus,
        };
        let press_state = state.clone();
        let press_close = closer.clone();
        let key_state = state.clone();
        let key_close = closer.clone();
        let toggle = move |state: &Entity<MenuState>,
                           close: &PopoverClose,
                           window: &mut Window,
                           cx: &mut App| {
            if state.read(cx).is_open() {
                close.close(window, cx);
            } else {
                menu::open(state, None, None, window, cx);
            }
        };
        let mut trigger = trigger
            .trigger(
                is_open,
                Rc::new(move |_, window, cx| {
                    toggle(&press_state, &press_close, window, cx);
                    cx.stop_propagation();
                }),
                Rc::new(move |event, window, cx| {
                    if matches!(event.keystroke.key.as_str(), "enter" | "space" | "down") {
                        toggle(&key_state, &key_close, window, cx);
                        cx.stop_propagation();
                    }
                }),
            )
            .attach(menu::trigger_probe(&state).into_any_element());
        let Some(shown) = menu::presence(&state, window, cx) else {
            return trigger;
        };
        let theme = cx.theme().clone();
        let content = (self.content)(&closer, window, cx);
        let key_closer = closer.clone();
        let out_state = state.clone();
        trigger = trigger.attach(
            menu::attach(self.align, &state, window, cx, |from, _| {
                menu::fading(
                    motion::arrive(
                        (self.id.clone(), "popover"),
                        div()
                            .id((self.id.clone(), "panel"))
                            .role(Role::Dialog)
                            .aria_label(label)
                            .occlude()
                            .track_focus(&panel_focus)
                            .on_key_down(move |event, window, cx| {
                                if event.keystroke.key == "escape" {
                                    key_closer.close(window, cx);
                                    cx.stop_propagation();
                                }
                            })
                            .on_mouse_down_out(move |event, window, cx| {
                                if menu::outside(&out_state, event.position, cx) {
                                    menu::close(&out_state, false, window, cx)
                                }
                            })
                            .map(|panel| match self.width {
                                Some(width) => panel.w(width),
                                None => panel.min_w(MIN_WIDTH).max_w(MAX_WIDTH),
                            })
                            .p(px(12.0))
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
            })
            .into_any_element(),
        );
        trigger
    }
}
