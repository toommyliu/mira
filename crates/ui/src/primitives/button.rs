use std::{
    cell::Cell,
    rc::Rc,
    time::{Duration, Instant},
};

use gpui::{
    App, BoxShadow, ClickEvent, Div, ElementId, FontWeight, Hsla, InteractiveElement as _,
    IntoElement, MouseButton, ParentElement as _, Pixels, RenderOnce, Role, SharedString, Stateful,
    StatefulInteractiveElement as _, StyleRefinement, Styled as _, Window, div,
    prelude::FluentBuilder as _, px,
};

use super::{Icon, Spinner, icon, tooltip};
use crate::{
    ActiveTheme as _, PointerCursor as _, nav,
    theme::{CONTROL_MD, CONTROL_SM, LINE_BODY, TEXT_BODY, TEXT_SMALL},
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
    Ghost,
    Danger,
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Size {
    Small,
    #[default]
    Medium,
}

enum Content {
    Label(SharedString),
    Icon(Icon, SharedString),
}

#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    content: Content,
    variant: Variant,
    size: Size,
    disabled: bool,
    busy: bool,
    selected: Option<bool>,
    tooltip: Option<SharedString>,
    on_click: Option<ClickHandler>,
}

impl Button {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self::with(id, Content::Label(label.into()))
    }

    pub fn icon(id: impl Into<ElementId>, icon: Icon, label: impl Into<SharedString>) -> Self {
        let label = label.into();
        let mut button = Self::with(id, Content::Icon(icon, label.clone()));
        button.variant = Variant::Ghost;
        button.tooltip = (icon != Icon::X).then_some(label);
        button
    }

    fn with(id: impl Into<ElementId>, content: Content) -> Self {
        Self {
            id: id.into(),
            content,
            variant: Variant::default(),
            size: Size::default(),
            disabled: false,
            busy: false,
            selected: None,
            tooltip: None,
            on_click: None,
        }
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

    pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(f));
        self
    }
}

impl RenderOnce for Button {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let focus = nav::focus_handle((self.id.clone(), "focus"), window, cx);
        let ring = !self.disabled && nav::keyboard_focused(&focus, window, cx);
        let theme = cx.theme().clone();
        let inert = self.disabled || self.busy;
        let pressed = self.selected == Some(true);
        let (bg, hover, active, fg, border) = match self.variant {
            Variant::Primary => (
                theme.inverse,
                theme.inverse_hover,
                theme.inverse_hover,
                theme.on_inverse,
                None,
            ),
            Variant::Secondary => (
                theme.fill,
                theme.fill_hover,
                theme.fill_pressed,
                theme.text,
                Some(theme.border),
            ),
            Variant::Ghost => (
                if pressed {
                    theme.selected
                } else {
                    gpui::transparent_black()
                },
                theme.fill_hover,
                theme.fill_pressed,
                if pressed {
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
        let height = match self.size {
            Size::Small => CONTROL_SM,
            Size::Medium => CONTROL_MD,
        };
        let (label, is_icon) = match &self.content {
            Content::Label(label) => (label.clone(), false),
            Content::Icon(_, label) => (label.clone(), true),
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
        let tip = self.tooltip.clone();
        let describe = tip.clone().filter(|tip| *tip != label);

        let button = div()
            .id(self.id.clone())
            .role(Role::Button)
            .when_some(describe, |button, tip| button.aria_description(tip))
            .aria_label(label.clone())
            .when_some(self.selected, |button, pressed| {
                button.aria_toggled(if pressed {
                    gpui::Toggled::True
                } else {
                    gpui::Toggled::False
                })
            })
            .when(self.busy, |button| button.aria_description("Working"))
            .when(!self.disabled, |button| {
                button.track_focus(&focus.tab_stop(true))
            })
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .h(height)
            .map(|button| {
                if is_icon {
                    button.w(height)
                } else {
                    button.px(match self.size {
                        Size::Small => px(8.0),
                        Size::Medium => px(12.0),
                    })
                }
            })
            .text_size(match self.size {
                Size::Small => TEXT_SMALL,
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
                    edge: if self.variant == Variant::Ghost || self.disabled {
                        Vec::new()
                    } else {
                        theme.edge()
                    },
                }
                .render((self.id.clone(), "face"), window, cx),
            )
            .when(!inert, |button| button.pointer_cursor(&theme))
            .when_some(self.on_click.filter(|_| !inert), |button, on_click| {
                button.on_click(move |event, window, cx| on_click(event, window, cx))
            })
            .child(
                div()
                    .flex()
                    .when(self.busy, |content| content.opacity(0.0))
                    .map(|content| match self.content {
                        Content::Label(label) => content.child(label),
                        Content::Icon(g, _) => content.child(icon(g, fg)),
                    }),
            )
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
        match tip {
            Some(tip) => tooltip::attach(button, (self.id, "tooltip"), tip, ring, window, cx),
            None => button,
        }
    }
}
