use std::rc::Rc;

use gpui::{
    AnyElement, App, ClickEvent, Div, ElementId, FontWeight, InteractiveElement as _, IntoElement,
    KeyDownEvent, MouseButton, MouseDownEvent, ParentElement, Pixels, Refineable as _, RenderOnce,
    Role, SharedString, StatefulInteractiveElement as _, StyleRefinement, Styled, Window, div,
    prelude::FluentBuilder as _, px,
};

use super::{Icon, Spinner, icon, tooltip};
use crate::{ActiveTheme as _, PointerCursor as _, h_flex, nav, theme::DISABLED_OPACITY};

pub type ClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

pub(crate) const FACE_GROUP: &str = "face";

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Joined {
    Next,
    Previous,
}

fn corners(element: Div, radius: Pixels, joined: Option<Joined>) -> Div {
    let element = element.rounded(radius);
    match joined {
        Some(Joined::Next) => element.rounded_r(px(0.0)),
        Some(Joined::Previous) => element.rounded_l(px(0.0)),
        None => element,
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
    Large,
}

impl Size {
    fn height(self, layout: crate::theme::Layout) -> Pixels {
        match self {
            Size::XSmall => layout.control_xs,
            Size::Small => layout.control_sm,
            Size::Medium => layout.control,
            Size::Large => layout.control_lg,
        }
    }

    fn icon_size(self) -> Pixels {
        px(match self {
            Self::XSmall => 10.0,
            Self::Small => 12.0,
            Self::Medium => 14.0,
            Self::Large => 16.0,
        })
    }
}
pub(crate) type PressHandler = Rc<dyn Fn(&MouseDownEvent, &mut Window, &mut App)>;
pub(crate) type KeyHandler = Rc<dyn Fn(&KeyDownEvent, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    label: SharedString,
    icon: Option<Icon>,
    icon_size: Option<Pixels>,
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
    joined: Option<Joined>,
    style: StyleRefinement,
}

impl Button {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            icon: None,
            icon_size: None,
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
            joined: None,
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

    pub fn icon_size(mut self, size: Pixels) -> Self {
        self.icon_size = Some(size);
        self
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

    pub fn joined(mut self, joined: Joined) -> Self {
        self.joined = Some(joined);
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

    pub(crate) fn id(&self) -> &ElementId {
        &self.id
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
        let typography = cx.theme().typography;
        let focus = match self.focus.take() {
            Some(focus) => focus,
            None => nav::focus_handle((self.id.clone(), "focus"), window, cx),
        };
        let ring = !self.disabled && nav::keyboard_focused(&focus, window, cx);
        let theme = cx.theme().clone();
        let inert = self.disabled || self.busy;
        let lit = self.selected == Some(true) || self.expanded == Some(true);
        let clear = gpui::transparent_black();
        let (bg, hover, fg, border) = if self.role == Role::ComboBox {
            (
                theme.input_fill(),
                theme.input.opacity(0.5),
                theme.foreground,
                theme.input,
            )
        } else {
            match self.variant {
                Variant::Primary => (
                    theme.primary,
                    theme.primary.opacity(0.8),
                    theme.primary_foreground,
                    clear,
                ),
                Variant::Secondary => (
                    theme.secondary,
                    crate::theme::perceptual_mix(theme.secondary, theme.foreground, 0.05),
                    theme.secondary_foreground,
                    clear,
                ),
                Variant::Outline => (
                    if lit {
                        theme.muted
                    } else {
                        theme.outline_fill()
                    },
                    theme.input.opacity(0.5),
                    theme.foreground,
                    theme.border,
                ),
                Variant::Ghost => (
                    if lit { theme.muted } else { clear },
                    theme.ghost_hover(),
                    theme.foreground,
                    clear,
                ),
                Variant::Danger => (
                    theme.destructive_fill(),
                    theme
                        .destructive
                        .opacity(if theme.dark_mode { 0.3 } else { 0.2 }),
                    theme.destructive,
                    clear,
                ),
            }
        };
        let active = theme.pressed(theme.background, hover, fg);
        let radius = if self.size == Size::XSmall {
            theme.radius.small
        } else {
            theme.radius.control
        };
        let destructive = self.variant == Variant::Danger;
        let focus_border = if destructive {
            theme.destructive.opacity(0.4)
        } else {
            theme.ring
        };
        let halo = if destructive {
            theme.invalid_halo()
        } else {
            theme.ring.opacity(0.3)
        };
        let nudge = !inert && self.expanded.is_none() && !cx.reduce_motion();
        let has_start = self.start.is_some();
        let has_end = self.end.is_some();
        let height = self.size.height(theme.layout);
        let icon_only = self.icon.is_some() && self.children.is_empty();
        let icon_size = self.icon_size.unwrap_or(self.size.icon_size());
        let tip = self.tooltip.clone();
        let describe = tip.clone().filter(|tip| *tip != self.label);
        let content = h_flex()
            .id("content")
            .flex_1()
            .min_w_0()
            .gap(theme.space(if self.role == Role::ComboBox {
                1.5
            } else {
                1.0
            }))
            .justify_center()
            .relative()
            .when(nudge, |content| {
                content.group_active(FACE_GROUP, |style| style.top(px(1.0)))
            })
            .when(self.busy, |content| content.opacity(0.0))
            .when_some(self.start, |content, start| {
                content.child(h_flex().flex_none().child(start))
            })
            .map(|content| match (self.icon, self.children.is_empty()) {
                (Some(glyph), true) => content.child(icon(glyph, fg).size(icon_size)),
                (_, true) => content.child(div().truncate().child(self.label.clone())),
                (_, false) => content.children(self.children),
            })
            .when(self.selected.is_some() && self.icon.is_none(), |content| {
                content.child(
                    div()
                        .flex_none()
                        .w(icon_size)
                        .when(self.selected == Some(true), |mark| {
                            mark.child(icon(Icon::Check, fg).size(icon_size))
                        }),
                )
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
            .when(self.joined == Some(Joined::Previous), |button| {
                button.ml(px(-1.0))
            })
            .h(height)
            .map(|button| {
                if icon_only {
                    button.w(height)
                } else {
                    let padding = if self.size == Size::Large {
                        theme.space(2.5)
                    } else {
                        theme.space(2.0)
                    };
                    let icon_padding = padding - theme.space(0.5);
                    button
                        .pl(if has_start { icon_padding } else { padding })
                        .pr(if has_end { icon_padding } else { padding })
                }
            })
            .text_size(match self.size {
                Size::XSmall => typography.secondary,
                _ => typography.body,
            })
            .line_height(if self.size == Size::XSmall {
                typography.secondary_line
            } else {
                typography.body_line
            })
            .font_weight(FontWeight::MEDIUM)
            .text_color(fg)
            .group(FACE_GROUP)
            .when(self.disabled, |button| button.opacity(DISABLED_OPACITY))
            .child(
                corners(div(), radius, self.joined)
                    .id("face")
                    .absolute()
                    .inset_0()
                    .bg(bg)
                    .border_1()
                    .border_color(if ring { focus_border } else { border })
                    .when(!inert, |face| {
                        face.group_hover(FACE_GROUP, |style| style.bg(hover))
                            .group_active(FACE_GROUP, |style| {
                                if nudge {
                                    style.bg(active).top(px(1.0)).bottom(px(-1.0))
                                } else {
                                    style.bg(active)
                                }
                            })
                    }),
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
                button.child(corners(
                    theme.halo(halo, radius, Pixels::ZERO, px(2.0)),
                    radius + px(2.0),
                    self.joined,
                ))
            });
        button.style().refine(&self.style);
        match tip {
            Some(tip) => tooltip::attach(button, (self.id, "tooltip"), tip, ring, window, cx),
            None => button,
        }
    }
}
