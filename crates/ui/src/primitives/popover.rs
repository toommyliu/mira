use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use gpui::{
    AnyElement, App, Div, ElementId, Entity, FocusHandle, InteractiveElement as _, IntoElement,
    ParentElement as _, Pixels, RenderOnce, Role, SharedString, Stateful,
    StatefulInteractiveElement as _, Styled as _, Window, div, prelude::FluentBuilder as _, px,
};

use super::{
    Align, Button, Placement, Side, input,
    menu::{self, MenuState},
    placement::COLLISION_MARGIN,
};
use crate::{
    ActiveTheme as _, FOCUS_SCOPE, FocusNext, FocusPrevious, motion,
    nav::{self, Axis, Step},
};

pub(crate) type Content = Rc<dyn Fn(&PopoverClose, &mut Window, &mut App) -> AnyElement>;

pub(super) const POPOVER_WIDTH: Pixels = px(288.0);

#[derive(Clone)]
pub struct PopoverClose {
    state: Entity<MenuState>,
    focus: Option<FocusHandle>,
}

impl PopoverClose {
    pub(crate) fn returning_focus(state: Entity<MenuState>) -> Self {
        Self { state, focus: None }
    }

    pub fn close(&self, window: &mut Window, cx: &mut App) {
        menu::close(&self.state, self.focus.is_none(), window, cx);
        if let Some(focus) = &self.focus {
            window.focus(focus, cx);
        }
    }
}

pub(crate) fn focus_first_on_open(
    id: &ElementId,
    state: &Entity<MenuState>,
    rows: bool,
    window: &mut Window,
    cx: &mut App,
) {
    let (is_open, panel) = {
        let state = state.read(cx);
        (state.is_open(), state.focus())
    };
    let was_open = window
        .use_keyed_state((id.clone(), "was-open"), cx, |_, _| {
            Rc::new(Cell::new(false))
        })
        .read(cx)
        .clone();
    if was_open.replace(is_open) != is_open && is_open {
        let pointer = rows && !state.read(cx).opened_from_keyboard;
        window.defer(cx, move |window, cx| {
            if pointer {
                window.focus(&panel, cx);
            } else {
                nav::focus_first(&panel, window, cx);
            }
        });
    }
}

type LeftField = Rc<RefCell<Option<FocusHandle>>>;

fn left_field(id: &ElementId, window: &mut Window, cx: &mut App) -> LeftField {
    window
        .use_keyed_state((id.clone(), "left-field"), cx, |_, _| LeftField::default())
        .read(cx)
        .clone()
}

fn resume(panel: &FocusHandle, left: &LeftField, window: &mut Window, cx: &mut App) {
    if panel.is_focused(window)
        && let Some(field) = left.borrow_mut().take()
    {
        window.focus(&field, cx);
    }
}

fn editing(window: &Window) -> bool {
    window
        .context_stack()
        .iter()
        .any(|context| context.contains(input::CONTEXT))
}

pub(crate) fn panel(
    id: &ElementId,
    label: SharedString,
    close: PopoverClose,
    width: Option<Pixels>,
    window: &mut Window,
    cx: &mut App,
) -> Stateful<Div> {
    let left = left_field(id, window, cx);
    let theme = cx.theme();
    let available = (window.viewport_size().width - COLLISION_MARGIN * 2.0).max(Pixels::ZERO);
    let focus = close.state.read(cx).focus();
    let (next, previous, panel) = (focus.clone(), focus.clone(), focus.clone());
    let (next_left, previous_left) = (left.clone(), left.clone());
    div()
        .id((id.clone(), "panel"))
        .debug_selector(|| "popover-panel".into())
        .role(Role::Dialog)
        .aria_label(label)
        .occlude()
        .key_context(FOCUS_SCOPE)
        .track_focus(&focus)
        .on_action(move |_: &FocusNext, window, cx| {
            resume(&next, &next_left, window, cx);
            nav::trap_tab(&next, false, window, cx)
        })
        .on_action(move |_: &FocusPrevious, window, cx| {
            resume(&previous, &previous_left, window, cx);
            nav::trap_tab(&previous, true, window, cx)
        })
        .on_key_down(move |event, window, cx| {
            if event.keystroke.key != "escape" {
                return;
            }
            if editing(window) {
                *left.borrow_mut() = window.focused(cx);
                window.focus(&panel, cx);
            } else {
                close.close(window, cx);
            }
            cx.stop_propagation();
        })
        .max_w(available)
        .when_some(width, |panel, width| panel.w(width))
        .p(theme.space(2.5))
        .rounded(theme.radius.panel)
        .bg(theme.popover)
        .text_color(theme.popover_foreground)
        .shadow(theme.popup_shadow())
        .child(theme.surface_ring(theme.radius.panel))
}

pub(crate) fn rows(
    panel: Stateful<Div>,
    id: &ElementId,
    close: &PopoverClose,
    window: &mut Window,
    cx: &mut App,
) -> Stateful<Div> {
    let left = left_field(id, window, cx);
    let focus = close.state.read(cx).focus();
    panel
        .p(menu::PANEL_INSET)
        .on_key_down(move |event, window, cx| {
            let Some(step) = Step::from_key(event, Axis::Vertical) else {
                return;
            };
            resume(&focus, &left, window, cx);
            match step {
                Step::Next => nav::trap_tab(&focus, false, window, cx),
                Step::Prev => nav::trap_tab(&focus, true, window, cx),
                Step::First | Step::PageUp => nav::focus_first(&focus, window, cx),
                Step::Last | Step::PageDown => {
                    nav::focus_first(&focus, window, cx);
                    nav::trap_tab(&focus, true, window, cx);
                }
            }
            cx.stop_propagation();
        })
}

#[derive(IntoElement)]
pub struct Popover {
    id: ElementId,
    trigger: Option<Button>,
    content: Content,
    placement: Placement,
    width: Pixels,
    inset: Option<Pixels>,
    default_open: bool,
}

impl Popover {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            trigger: None,
            content: Rc::new(|_, _, _| div().into_any_element()),
            placement: Placement::new(Side::Bottom, Align::Center),
            width: POPOVER_WIDTH,
            inset: None,
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
        self.width = width;
        self
    }

    pub(crate) fn inset(mut self, inset: Pixels) -> Self {
        self.inset = Some(inset);
        self
    }

    pub fn placement(mut self, placement: Placement) -> Self {
        self.placement = placement;
        self
    }

    pub fn align(mut self, align: Align) -> Self {
        self.placement.align = align;
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
        let trigger_focus = nav::focus_handle((self.id.clone(), "trigger"), window, cx);
        let closer = PopoverClose {
            state: state.clone(),
            focus: Some(trigger_focus.clone()),
        };
        focus_first_on_open(&self.id, &state, false, window, cx);
        let is_open = state.read(cx).is_open();
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
            .focus(trigger_focus)
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
        let content = (self.content)(&closer, window, cx);
        let panel = panel(&self.id, label, closer, Some(self.width), window, cx)
            .when_some(self.inset, |panel, inset| panel.p(inset))
            .child(content);
        trigger = trigger.attach(
            menu::attach(
                self.placement,
                &state,
                menu::dismiss(&state),
                window,
                cx,
                |from, _| {
                    menu::fading(
                        motion::arrive((self.id.clone(), "popover"), panel, from),
                        shown,
                    )
                },
            )
            .into_any_element(),
        );
        trigger
    }
}
