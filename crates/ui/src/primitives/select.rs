use std::rc::Rc;

use gpui::{
    AnyElement, App, ElementId, IntoElement, ParentElement as _, Pixels, RenderOnce, SharedString,
    Styled as _, Window, anchored, deferred, div, prelude::FluentBuilder as _, px,
};

use super::{
    Button, Icon,
    menu::{
        Align, MenuItem, PopupKind, WINDOW_MARGIN, attach, dismiss, dismiss_probe,
        item_over_trigger, popup, presence, state, trigger_handlers, trigger_probe,
    },
};
use crate::{ActiveTheme as _, IndexHandler, motion};

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
    align: Align,
    align_item: bool,
    default_open: bool,
    disabled: bool,
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
            align: Align::Start,
            align_item: false,
            default_open: false,
            disabled: false,
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

    pub fn full_width(mut self) -> Self {
        self.width = None;
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

    pub fn default_open(mut self, open: bool) -> Self {
        self.default_open = open;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
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
        let shown = presence(&state, window, cx).is_some();
        let theme = cx.theme().clone();
        let selected = self.selected.filter(|&index| index < self.options.len());
        let value = selected.map(|index| self.options[index].label.clone());
        let count = self.options.len();
        let initial = selected.or((count > 0).then_some(0));
        let (press, keys) = trigger_handlers(&state, None, initial);
        let mut trigger = Button::new((self.id.clone(), "trigger"), self.label.clone())
            .combobox(value.clone())
            .disabled(self.disabled)
            .end_slot(
                div()
                    .text_color(theme.text_tertiary)
                    .child(Icon::ChevronDown),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .when(value.is_none(), |text| text.text_color(theme.text_tertiary))
                    .child(value.unwrap_or(self.placeholder)),
            )
            .font_weight(gpui::FontWeight::NORMAL)
            .map(|trigger| match self.width {
                Some(width) => trigger.w(width),
                None => trigger.w_full(),
            })
            .trigger(is_open, press, keys)
            .attach(trigger_probe(&state).into_any_element());
        if !shown {
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
        let min_width = state.read(cx).trigger_width();
        let panel = |from, min_width, window: &mut Window| {
            div()
                .min_w(min_width)
                .child(popup(
                    &self.id,
                    self.label.clone(),
                    items,
                    PopupKind::ListBox,
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
        trigger = match over {
            Some(offset) => trigger.attach(
                div()
                    .absolute()
                    .top(-offset.y)
                    .left(-offset.x)
                    .child(
                        deferred(
                            anchored().snap_to_window_with_margin(WINDOW_MARGIN).child(
                                div()
                                    .relative()
                                    .child(dismiss_probe(&state, dismiss(&state)))
                                    .child(panel(
                                        motion::From::Above,
                                        min_width + offset.x,
                                        window,
                                    )),
                            ),
                        )
                        .with_priority(super::layer::POPUP),
                    )
                    .into_any_element(),
            ),
            None => trigger.attach(
                attach(
                    self.align,
                    &state,
                    dismiss(&state),
                    window,
                    cx,
                    |from, window| panel(from, min_width, window),
                )
                .into_any_element(),
            ),
        };
        trigger
    }
}
