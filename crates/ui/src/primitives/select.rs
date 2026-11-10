use std::rc::Rc;

use gpui::{
    App, ElementId, InteractiveElement as _, IntoElement, MouseButton, ParentElement as _, Pixels,
    RenderOnce, Role, SharedString, StatefulInteractiveElement as _, Styled as _, Window, anchored,
    deferred, div, prelude::FluentBuilder as _, px,
};

use super::{
    Icon, icon,
    menu::{
        Align, Kind, MenuItem, WINDOW_MARGIN, attach, close, item_over_trigger, open, popup,
        presence, state, trigger_face, trigger_keys, trigger_probe, trigger_style,
    },
};
use crate::{ActiveTheme as _, IndexHandler, motion::From, nav};

#[derive(IntoElement)]
pub struct Select {
    id: ElementId,
    label: SharedString,
    options: Vec<SharedString>,
    selected: Option<usize>,
    placeholder: SharedString,
    width: Pixels,
    align: Align,
    align_item: bool,
    on_select: Option<IndexHandler>,
}

impl Select {
    pub fn new(
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        options: Vec<SharedString>,
        selected: Option<usize>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            options,
            selected,
            placeholder: "Choose".into(),
            width: px(200.0),
            align: Align::Start,
            align_item: false,
            on_select: None,
        }
    }

    pub fn placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    pub fn width(mut self, width: Pixels) -> Self {
        self.width = width;
        self
    }

    pub fn align(mut self, align: Align) -> Self {
        self.align = align;
        self
    }

    pub fn align_item_with_trigger(mut self) -> Self {
        self.align_item = true;
        self
    }

    pub fn on_select(mut self, f: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_select = Some(Rc::new(f));
        self
    }
}

impl RenderOnce for Select {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = state(&self.id, window, cx);
        let trigger_focus = nav::focus_handle((self.id.clone(), "trigger"), window, cx);
        let ring = nav::keyboard_focused(&trigger_focus, window);
        let is_open = state.read(cx).open;
        let shown = presence(&state, window, cx).is_some();
        let theme = cx.theme().clone();
        let selected = self.selected;
        let items: Rc<[MenuItem]> = self
            .options
            .iter()
            .enumerate()
            .map(|(index, option)| {
                let on_select = self.on_select.clone();
                MenuItem::action(option.clone(), move |window, cx| {
                    if let Some(on_select) = &on_select {
                        on_select(index, window, cx);
                    }
                })
                .checked(selected == Some(index))
            })
            .collect();
        let value = selected.and_then(|index| self.options.get(index).cloned());
        let click_state = state.clone();
        div()
            .relative()
            .flex_none()
            .w(self.width)
            .child(
                trigger_style(
                    div()
                        .id(self.id.clone())
                        .role(Role::ComboBox)
                        .aria_label(self.label.clone())
                        .aria_expanded(is_open)
                        .when_some(value.clone(), |trigger, value| trigger.aria_value(value))
                        .track_focus(&trigger_focus.tab_stop(true)),
                    trigger_face(is_open, &theme).render((self.id.clone(), "face"), window, cx),
                    &theme,
                    ring,
                )
                .w_full()
                .on_key_down(trigger_keys(items.clone(), state.clone(), selected))
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    if click_state.read(cx).open {
                        close(&click_state, true, window, cx);
                    } else {
                        open(&click_state, selected, None, window, cx);
                    }
                    cx.stop_propagation();
                })
                .child(
                    div()
                        .flex_1()
                        .truncate()
                        .when(value.is_none(), |text| text.text_color(theme.text_tertiary))
                        .child(value.unwrap_or(self.placeholder)),
                )
                .child(icon(Icon::ChevronDown, theme.text_tertiary))
                .child(trigger_probe(&state)),
            )
            .when(shown, |container| {
                let count = items.len();
                let panel = |from, min_width, window: &mut Window| {
                    div()
                        .min_w(min_width)
                        .child(popup(
                            &self.id,
                            self.label.clone(),
                            items,
                            Kind::ListBox,
                            from,
                            &state,
                            window,
                            cx,
                        ))
                        .into_any_element()
                };
                let over = if is_open {
                    let over = selected
                        .filter(|_| self.align_item)
                        .and_then(|index| item_over_trigger(index, count, &state, window, cx));
                    state.read(cx).item_offset.set(over);
                    over
                } else {
                    state.read(cx).item_offset.get()
                };
                match over {
                    Some(offset) => container.child(
                        div().absolute().top(-offset.y).left(-offset.x).child(
                            deferred(
                                anchored()
                                    .snap_to_window_with_margin(WINDOW_MARGIN)
                                    .child(panel(From::Above, self.width + offset.x, window)),
                            )
                            .with_priority(super::layer::POPUP),
                        ),
                    ),
                    None => {
                        container.child(attach(self.align, &state, window, cx, |from, window| {
                            panel(from, self.width, window)
                        }))
                    }
                }
            })
    }
}
