use std::rc::Rc;

use gpui::{
    AnyElement, App, ElementId, FocusHandle, IntoElement, ParentElement as _, Pixels, RenderOnce,
    SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
    prelude::FluentBuilder as _, px,
};

use super::{
    Align, Button, Icon, Placement, Side,
    menu::{
        MenuItem, PopupKind, SELECT_MAX_HEIGHT, WINDOW_MARGIN, dismiss, popup_layer, popup_panel,
        state, trigger_handlers, trigger_probe,
    },
};
use crate::{ActiveTheme as _, IndexHandler};

pub struct SelectOption {
    label: SharedString,
    start: Option<AnyElement>,
    end: Option<AnyElement>,
    description: Option<SharedString>,
}

impl SelectOption {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            start: None,
            end: None,
            description: None,
        }
    }

    pub fn start_slot(mut self, element: impl IntoElement) -> Self {
        self.start = Some(element.into_any_element());
        self
    }

    pub fn end_slot(mut self, element: impl IntoElement) -> Self {
        self.end = Some(element.into_any_element());
        self
    }

    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }
}

impl From<SharedString> for SelectOption {
    fn from(label: SharedString) -> Self {
        Self::new(label)
    }
}

impl From<&'static str> for SelectOption {
    fn from(label: &'static str) -> Self {
        Self::new(label)
    }
}

impl From<String> for SelectOption {
    fn from(label: String) -> Self {
        Self::new(label)
    }
}

#[derive(IntoElement)]
pub struct Select {
    id: ElementId,
    label: SharedString,
    options: Vec<SelectOption>,
    selected: Option<usize>,
    placeholder: SharedString,
    width: Option<Pixels>,
    placement: Placement,
    default_open: bool,
    disabled: bool,
    invalid: bool,
    small: bool,
    focus: Option<FocusHandle>,
    on_select: Option<IndexHandler>,
}

impl Select {
    pub fn new(
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        options: impl IntoIterator<Item = impl Into<SelectOption>>,
        selected: Option<usize>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            options: options.into_iter().map(Into::into).collect(),
            selected,
            placeholder: "Choose".into(),
            width: Some(px(200.0)),
            placement: Placement::new(Side::Bottom, Align::Center),
            default_open: false,
            disabled: false,
            invalid: false,
            small: false,
            focus: None,
            on_select: None,
        }
    }

    pub fn placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    pub fn width(mut self, width: Pixels) -> Self {
        self.width = Some(width);
        self
    }

    pub fn small(mut self) -> Self {
        self.small = true;
        self
    }

    pub fn full_width(mut self) -> Self {
        self.width = None;
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

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn invalid(mut self, invalid: bool) -> Self {
        self.invalid = invalid;
        self
    }

    pub fn focus(mut self, focus: FocusHandle) -> Self {
        self.focus = Some(focus);
        self
    }

    pub fn on_select(mut self, f: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_select = Some(Rc::new(f));
        self
    }
}

impl RenderOnce for Select {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = state(&self.id, self.default_open, window, cx);
        let is_open = state.read(cx).open;
        let muted = cx.theme().muted_foreground;
        let selected = self.selected.filter(|&index| index < self.options.len());
        let value = selected.map(|index| self.options[index].label.clone());
        let count = self.options.len();
        let initial = selected.or((count > 0).then_some(0));
        let (press, keys) = trigger_handlers(&state, None, initial, false);
        let trigger = Button::new((self.id.clone(), "trigger"), self.label.clone())
            .combobox(value.clone())
            .when(self.small, |button| button.size(super::button::Size::Small))
            .when_some(self.focus, Button::focus)
            .disabled(self.disabled)
            .invalid(self.invalid)
            .end_slot(div().text_color(muted).child(Icon::ChevronDown))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .when(value.is_none(), |text| text.text_color(muted))
                    .child(value.unwrap_or(self.placeholder)),
            )
            .font_weight(gpui::FontWeight::NORMAL)
            .map(|trigger| match self.width {
                Some(width) => trigger.w(width),
                None => trigger.w_full(),
            })
            .trigger(is_open, press, keys)
            .attach(trigger_probe(&state).into_any_element());
        let bounds = state.read(cx).trigger_bounds();
        if !is_open || bounds.is_empty() {
            return trigger;
        }
        let on_select = self.on_select;
        let items: Vec<MenuItem> = self
            .options
            .into_iter()
            .enumerate()
            .map(|(index, option)| {
                let on_select = on_select.clone();
                let mut item = MenuItem::action(option.label, move |window, cx| {
                    if let Some(on_select) = &on_select {
                        on_select(index, window, cx);
                    }
                })
                .checked(selected == Some(index));
                if let Some(start) = option.start {
                    item = item.start_slot(start);
                }
                if let Some(end) = option.end {
                    item = item.end_slot(end);
                }
                if let Some(description) = option.description {
                    item = item.description(description);
                }
                item
            })
            .collect();
        let max_height = match self.placement.side {
            Side::Bottom | Side::Top => {
                let room = bounds
                    .top()
                    .max(window.viewport_size().height - bounds.bottom());
                (room - self.placement.side_offset - WINDOW_MARGIN)
                    .clamp(Pixels::ZERO, SELECT_MAX_HEIGHT)
            }
            Side::Left | Side::Right => SELECT_MAX_HEIGHT,
        };
        let panel = popup_panel(
            &self.id,
            items,
            PopupKind::ListBox,
            max_height,
            &state,
            window,
            cx,
        )
        .w(bounds.size.width.max(px(128.0)))
        .aria_label(self.label);
        trigger.attach(
            popup_layer(
                self.placement.anchored_to(bounds),
                &state,
                dismiss(&state),
                [panel.into_any_element()],
            )
            .into_any_element(),
        )
    }
}
