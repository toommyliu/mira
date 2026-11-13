use std::{
    cell::Cell,
    rc::Rc,
    time::{Duration, Instant},
};

use gpui::{
    AnyElement, App, BoxShadow, ClickEvent, Div, ElementId, FontWeight, Hsla,
    InteractiveElement as _, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, ParentElement,
    Pixels, Refineable as _, RenderOnce, Role, SharedString, Stateful,
    StatefulInteractiveElement as _, StyleRefinement, Styled, Window, div,
    prelude::FluentBuilder as _, px,
};

use super::{Icon, Spinner, icon, tooltip};
use crate::{
    ActiveTheme as _, PointerCursor as _, h_flex, nav,
    theme::{CONTROL_MD, CONTROL_SM, CONTROL_XS, LINE_BODY, TEXT_BODY, TEXT_SMALL},
};

pub type ClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

pub(crate) const FACE_GROUP: &str = "face";

const MIN_PRESS: Duration = Duration::from_millis(120);

pub(crate) struct Face {
    pub radius: Pixels,
    pub bg: Hsla,
    pub border: Option<Hsla>,
    pub states: Option<(Hsla, Hsla)>,
    pub edge: Vec<BoxShadow>,
}

impl Face {
    pub fn render(
        self,
        id: impl Into<ElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> Stateful<Div> {
        let pressed_at = window
            .use_keyed_state(id, cx, |_, _| Rc::new(Cell::new(None::<Instant>)))
            .read(cx)
            .clone();
        let lingering = pressed_at.get().is_some_and(|at| at.elapsed() < MIN_PRESS);
        if lingering {
            window.request_animation_frame();
        }
        let pressed =
            move |style: StyleRefinement, active: Hsla| style.bg(active).shadow(Vec::new());
        let face = div()
            .id("face")
            .absolute()
            .inset_0()
            .rounded(self.radius)
            .bg(self.bg)
            .when_some(self.border, |face, border| {
                face.border_1().border_color(border)
            })
            .shadow(self.edge);
        match self.states {
            None => face,
            Some((hover, active)) => {
                let face = face
                    .group_hover(FACE_GROUP, move |style| style.bg(hover))
                    .group_active(FACE_GROUP, move |style| pressed(style, active))
                    .on_mouse_down(MouseButton::Left, move |_, window, _| {
                        pressed_at.set(Some(Instant::now()));
                        window.refresh();
                    });
                if lingering {
                    let mut face = face;
                    let held = pressed(face.style().clone(), active);
                    *face.style() = held;
                    face
                } else {
                    face
                }
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Variant {
    Primary,
    #[default]
    Secondary,
    Outline,
    Ghost,
    Danger,
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Size {
    XSmall,
    Small,
    #[default]
    Medium,
}

impl Size {
    fn height(self) -> Pixels {
        match self {
            Size::XSmall => CONTROL_XS,
            Size::Small => CONTROL_SM,
            Size::Medium => CONTROL_MD,
        }
    }

    fn padding(self) -> Pixels {
        match self {
            Size::XSmall => px(6.0),
            Size::Small => px(8.0),
            Size::Medium => px(10.0),
        }
    }
}

pub(crate) type PressHandler = Rc<dyn Fn(&MouseDownEvent, &mut Window, &mut App)>;
pub(crate) type KeyHandler = Rc<dyn Fn(&KeyDownEvent, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    label: SharedString,
    icon: Option<Icon>,
    start: Option<AnyElement>,
    end: Option<AnyElement>,
    children: Vec<AnyElement>,
    variant: Variant,
    size: Size,
    disabled: bool,
    busy: bool,
    selected: Option<bool>,
    expanded: Option<bool>,
    tooltip: Option<SharedString>,
    focus: Option<gpui::FocusHandle>,
    on_click: Option<ClickHandler>,
    on_press: Option<PressHandler>,
    on_key_down: Option<KeyHandler>,
    attachments: Vec<AnyElement>,
    role: Role,
    value: Option<SharedString>,
    style: StyleRefinement,
}

impl Button {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            icon: None,
            start: None,
            end: None,
            children: Vec::new(),
            variant: Variant::default(),
            size: Size::default(),
            disabled: false,
            busy: false,
            selected: None,
            expanded: None,
            tooltip: None,
            focus: None,
            on_click: None,
            on_press: None,
            on_key_down: None,
            attachments: Vec::new(),
            role: Role::Button,
            value: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn icon(id: impl Into<ElementId>, icon: Icon, label: impl Into<SharedString>) -> Self {
        let mut button = Self::new(id, label);
        button.icon = Some(icon);
        button.variant = Variant::Ghost;
        button.tooltip = (icon != Icon::X).then(|| button.label.clone());
        button
    }

    pub fn start_slot(mut self, element: impl IntoElement) -> Self {
        self.start = Some(element.into_any_element());
        self
    }

    pub fn end_slot(mut self, element: impl IntoElement) -> Self {
        self.end = Some(element.into_any_element());
        self
    }

    pub fn variant(mut self, variant: Variant) -> Self {
        self.variant = variant;
        self
    }

    pub fn size(mut self, size: Size) -> Self {
        self.size = size;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn busy(mut self, busy: bool) -> Self {
        self.busy = busy;
        self
    }

    pub fn pressed(mut self, pressed: bool) -> Self {
        self.selected = Some(pressed);
        self
    }

    pub fn tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    pub fn hide_tooltip(mut self) -> Self {
        self.tooltip = None;
        self
    }

    pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(f));
        self
    }

    pub(crate) fn trigger(
        mut self,
        expanded: bool,
        on_press: PressHandler,
        on_key_down: KeyHandler,
    ) -> Self {
        self.expanded = Some(expanded);
        self.on_press = Some(on_press);
        self.on_key_down = Some(on_key_down);
        self
    }

    pub(crate) fn attach(mut self, element: AnyElement) -> Self {
        self.attachments.push(element);
        self
    }

    pub(crate) fn combobox(mut self, value: Option<SharedString>) -> Self {
        self.role = Role::ComboBox;
        self.value = value;
        self
    }

    pub(crate) fn focus(mut self, focus: gpui::FocusHandle) -> Self {
        self.focus = Some(focus);
        self
    }

    pub(crate) fn label(&self) -> &SharedString {
        &self.label
    }

    pub(crate) fn is_disabled(&self) -> bool {
        self.disabled
    }
}

impl ParentElement for Button {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl Styled for Button {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Button {
    fn render(mut self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let focus = match self.focus.take() {
            Some(focus) => focus,
            None => nav::focus_handle((self.id.clone(), "focus"), window, cx),
        };
        let ring = !self.disabled && nav::keyboard_focused(&focus, window, cx);
        let theme = cx.theme().clone();
        let inert = self.disabled || self.busy;
        let lit = self.selected == Some(true) || self.expanded == Some(true);
        let (bg, hover, active, fg, border) = match self.variant {
            Variant::Primary => (
                theme.inverse,
                theme.inverse_hover,
                theme.inverse_hover,
                theme.on_inverse,
                None,
            ),
            Variant::Secondary => (
                if lit { theme.fill_pressed } else { theme.fill },
                theme.fill_hover,
                theme.fill_pressed,
                theme.text,
                Some(theme.border),
            ),
            Variant::Outline => (
                if lit {
                    theme.fill_pressed
                } else {
                    gpui::transparent_black()
                },
                theme.fill_hover,
                theme.fill_pressed,
                theme.text,
                Some(theme.border),
            ),
            Variant::Ghost => (
                if lit {
                    theme.selected
                } else {
                    gpui::transparent_black()
                },
                theme.fill_hover,
                theme.fill_pressed,
                if lit {
                    theme.text
                } else {
                    theme.text_secondary
                },
                None,
            ),
            Variant::Danger => (
                theme.danger,
                theme.danger_hover,
                theme.danger_hover,
                theme.on_danger,
                None,
            ),
        };
        let fg = if self.disabled {
            theme.text_disabled
        } else {
            fg
        };
        let bg = if self.disabled && matches!(self.variant, Variant::Primary | Variant::Danger) {
            theme.fill
        } else {
            bg
        };
        let height = self.size.height();
        let icon_only = self.icon.is_some() && self.children.is_empty();
        let tip = self.tooltip.clone();
        let describe = tip.clone().filter(|tip| *tip != self.label);
        let content = h_flex()
            .flex_1()
            .min_w_0()
            .gap(px(6.0))
            .justify_center()
            .when(self.busy, |content| content.opacity(0.0))
            .when_some(self.start, |content, start| {
                content.child(h_flex().flex_none().child(start))
            })
            .map(|content| match (self.icon, self.children.is_empty()) {
                (Some(glyph), true) => content.child(icon(glyph, fg)),
                (_, true) => content.child(div().truncate().child(self.label.clone())),
                (_, false) => content.children(self.children),
            })
            .when_some(self.end, |content, end| {
                content.child(h_flex().flex_none().child(end))
            });
        let mut button = h_flex()
            .id(self.id.clone())
            .role(self.role)
            .aria_label(self.label.clone())
            .when_some(self.value.clone(), |button, value| button.aria_value(value))
            .when_some(describe, |button, tip| button.aria_description(tip))
            .when_some(self.selected, |button, pressed| {
                button.aria_toggled(if pressed {
                    gpui::Toggled::True
                } else {
                    gpui::Toggled::False
                })
            })
            .when_some(self.expanded, |button, expanded| {
                button.aria_expanded(expanded)
            })
            .when(self.busy, |button| button.aria_description("Working"))
            .when(!self.disabled, |button| {
                button.track_focus(&focus.clone().tab_stop(true))
            })
            .relative()
            .flex_none()
            .justify_center()
            .h(height)
            .map(|button| {
                if icon_only {
                    button.w(height)
                } else {
                    button.px(self.size.padding())
                }
            })
            .text_size(match self.size {
                Size::XSmall | Size::Small => TEXT_SMALL,
                Size::Medium => TEXT_BODY,
            })
            .line_height(LINE_BODY)
            .font_weight(FontWeight::MEDIUM)
            .text_color(fg)
            .group(FACE_GROUP)
            .child(
                Face {
                    radius: theme.radius.control,
                    bg,
                    border: border.filter(|_| !self.disabled),
                    states: (!inert).then_some((hover, active)),
                    edge: if matches!(self.variant, Variant::Ghost | Variant::Outline)
                        || self.disabled
                    {
                        Vec::new()
                    } else {
                        theme.edge()
                    },
                }
                .render((self.id.clone(), "face"), window, cx),
            )
            .when(!inert, |button| button.pointer_cursor(&theme))
            .when_some(self.on_click.filter(|_| !inert), |button, on_click| {
                button.on_click(move |event, window, cx| {
                    cx.stop_propagation();
                    on_click(event, window, cx)
                })
            })
            .when_some(self.on_press.filter(|_| !inert), |button, on_press| {
                button.on_mouse_down(MouseButton::Left, move |event, window, cx| {
                    on_press(event, window, cx)
                })
            })
            .when_some(
                self.on_key_down.filter(|_| !inert),
                |button, on_key_down| {
                    let own = focus.clone();
                    button.on_key_down(move |event, window, cx| {
                        if own.is_focused(window) {
                            on_key_down(event, window, cx)
                        }
                    })
                },
            )
            .child(content)
            .children(self.attachments)
            .when(self.busy, |button| {
                button.child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(Spinner::new("busy").color(fg)),
                )
            })
            .when(ring, |button| {
                button.child(theme.focus_ring(theme.radius.control))
            });
        button.style().refine(&self.style);
        match tip {
            Some(tip) => tooltip::attach(button, (self.id, "tooltip"), tip, ring, window, cx),
            None => button,
        }
    }
}
