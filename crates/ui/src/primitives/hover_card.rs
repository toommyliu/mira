use gpui::{
    AnyElement, App, Div, ElementId, FontWeight, Hsla, InteractiveElement as _, IntoElement,
    ParentElement, Pixels, RenderOnce, SharedString, Stateful, Styled as _, TextRun, Window, div,
    px,
};

const LABEL_MIN: Pixels = px(44.0);

use super::tooltip;
use crate::Tone;

enum Value {
    Text(SharedString, Tone),
    Path(SharedString),
}

pub(super) struct Card {
    title: SharedString,
    rows: Vec<(SharedString, Value)>,
}

#[derive(IntoElement)]
pub struct HoverCard {
    id: ElementId,
    card: Card,
    children: Vec<AnyElement>,
    placement: super::Placement,
}

impl HoverCard {
    pub fn new(id: impl Into<ElementId>, title: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            card: Card {
                title: title.into(),
                rows: Vec::new(),
            },
            children: Vec::new(),
            placement: super::Placement::new(super::Side::Bottom, super::Align::Center),
        }
    }

    pub fn placement(mut self, placement: super::Placement) -> Self {
        self.placement = placement;
        self
    }

    pub fn row(
        mut self,
        label: impl Into<SharedString>,
        text: impl Into<SharedString>,
        tone: Tone,
    ) -> Self {
        self.card
            .rows
            .push((label.into(), Value::Text(text.into(), tone)));
        self
    }

    pub fn path_row(
        mut self,
        label: impl Into<SharedString>,
        path: impl Into<SharedString>,
    ) -> Self {
        self.card
            .rows
            .push((label.into(), Value::Path(path.into())));
        self
    }

    pub(super) fn attach(
        self,
        trigger: Stateful<Div>,
        focused: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> Stateful<Div> {
        tooltip::hover(
            trigger.children(self.children),
            (self.id, "card").into(),
            tooltip::Help::Card(self.card),
            focused,
            self.placement,
            window,
            cx,
        )
    }
}

impl ParentElement for HoverCard {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for HoverCard {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let trigger = div().id(self.id.clone()).flex().flex_1().min_w_0();
        self.attach(trigger, false, window, cx)
    }
}

pub(super) fn popup(card: Card, theme: &crate::Theme, window: &Window) -> gpui::Div {
    let layout = theme.layout;
    let typography = theme.typography;
    let inset = theme.space(2.5);
    let label_width = card
        .rows
        .iter()
        .map(|(label, _)| {
            let run = TextRun {
                len: label.len(),
                font: gpui::font(theme.font_family.clone()),
                color: Hsla::default(),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            window
                .text_system()
                .shape_line(label.clone(), typography.secondary, &[run], None)
                .width
        })
        .fold(LABEL_MIN, Pixels::max)
        .min((super::popover::POPOVER_WIDTH - inset * 2.0) / 2.0)
        .ceil();
    div()
        .flex()
        .flex_col()
        .gap(layout.space(1.5))
        .w(super::popover::POPOVER_WIDTH)
        .max_w_full()
        .p(inset)
        .rounded(theme.radius.panel)
        .bg(theme.popover)
        .shadow(theme.popup_shadow())
        .child(theme.surface_ring(theme.radius.panel))
        .child(
            div()
                .mb(layout.space(0.5))
                .truncate()
                .text_size(typography.body)
                .line_height(typography.body_line)
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.foreground)
                .child(card.title),
        )
        .children(card.rows.into_iter().map(|(label, value)| {
            let cell = div().flex_1().min_w_0().text_color(theme.foreground);
            let cell = match value {
                Value::Path(path) => cell.child(path),
                Value::Text(text, tone) => cell
                    .line_clamp(3)
                    .text_ellipsis()
                    .text_color(match tone {
                        Tone::Neutral => theme.foreground,
                        tone => theme.tone(tone).2,
                    })
                    .child(text),
            };
            div()
                .flex()
                .gap(layout.space(3.0))
                .text_size(typography.secondary)
                .line_height(typography.secondary_line)
                .child(
                    div()
                        .flex_none()
                        .w(label_width)
                        .text_color(theme.muted_foreground)
                        .child(label),
                )
                .child(cell)
        }))
}
