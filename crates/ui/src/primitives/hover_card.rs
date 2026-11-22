use gpui::{
    AnyElement, App, Div, ElementId, FontWeight, HighlightStyle, Hsla, InteractiveElement as _,
    IntoElement, ParentElement, Pixels, RenderOnce, ScrollHandle, SharedString, Stateful,
    StatefulInteractiveElement as _, Styled as _, StyledText, TextRun, Window, canvas, div,
    prelude::FluentBuilder as _, px,
};

const LABEL_MIN: Pixels = px(44.0);
const NOTES_MAX_HEIGHT: Pixels = px(240.0);

use super::{Scrollbar, tooltip};
use crate::Tone;

enum Value {
    Text(SharedString, Tone),
    Path(SharedString),
}

pub(super) struct Card {
    title: SharedString,
    rows: Vec<(SharedString, Value)>,
    body: Option<SharedString>,
    notes: Vec<(Option<SharedString>, SharedString)>,
    scroll: ScrollHandle,
    fit: bool,
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
                body: None,
                notes: Vec::new(),
                scroll: ScrollHandle::new(),
                fit: false,
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

    pub fn fit(mut self) -> Self {
        self.card.fit = true;
        self
    }

    pub fn body(mut self, body: impl Into<SharedString>) -> Self {
        self.card.body = Some(body.into());
        self
    }

    pub fn note(mut self, lead: Option<SharedString>, text: impl Into<SharedString>) -> Self {
        self.card.notes.push((lead, text.into()));
        self
    }

    pub(super) fn attach(
        mut self,
        trigger: Stateful<Div>,
        focused: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> Stateful<Div> {
        self.card.scroll = window
            .use_keyed_state((self.id.clone(), "notes"), cx, |_, _| ScrollHandle::new())
            .read(cx)
            .clone();
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
    let width = if card.fit {
        let line = |text: &SharedString, size: Pixels, weight: FontWeight| {
            let run = TextRun {
                len: text.len(),
                font: gpui::Font {
                    weight,
                    ..gpui::font(theme.font_family.clone())
                },
                color: Hsla::default(),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            window
                .text_system()
                .shape_line(text.clone(), size, &[run], None)
                .width
        };
        let title = line(&card.title, typography.body, FontWeight::MEDIUM);
        let body = card.body.as_ref().map_or(Pixels::ZERO, |body| {
            line(body, typography.secondary, FontWeight::NORMAL)
        });
        (title.max(body) + inset * 2.0)
            .ceil()
            .min(super::popover::POPOVER_WIDTH)
    } else {
        super::popover::POPOVER_WIDTH
    };
    div()
        .flex()
        .flex_col()
        .gap(layout.space(1.5))
        .w(width)
        .max_w_full()
        .p(inset)
        .rounded(theme.radius.panel)
        .bg(theme.popover)
        .shadow(theme.popup_shadow())
        .child(theme.surface_ring(theme.radius.panel))
        .child(
            div()
                .mb(layout.space(0.5))
                .line_clamp(2)
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
        .when(card.body.is_some() || !card.notes.is_empty(), |popup| {
            let scroll = card.scroll.clone();
            let overflows = scroll.max_offset().y > Pixels::ZERO;
            let measured = scroll.clone();
            popup.child(
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .min_h_0()
                    .mr(-inset)
                    .child(
                        div()
                            .id("hover-card-notes")
                            .flex()
                            .flex_col()
                            .gap(layout.space(1.5))
                            .max_h(NOTES_MAX_HEIGHT)
                            .pr(if overflows {
                                Scrollbar::GUTTER + px(2.0)
                            } else {
                                inset
                            })
                            .overflow_y_scroll()
                            .track_scroll(&scroll)
                            .text_size(typography.secondary)
                            .line_height(typography.secondary_line)
                            .text_color(theme.muted_foreground)
                            .children(
                                card.body
                                    .map(|body| div().mt(layout.space(0.5)).child(body)),
                            )
                            .children(card.notes.into_iter().map(|(lead, text)| {
                                let (line, bold) = match lead {
                                    Some(lead) => (format!("{lead} {text}"), lead.len()),
                                    None => (text.to_string(), 0),
                                };
                                let lead = HighlightStyle {
                                    font_weight: Some(FontWeight::MEDIUM),
                                    color: Some(theme.foreground),
                                    ..Default::default()
                                };
                                div()
                                    .flex()
                                    .flex_none()
                                    .gap(layout.space(2.0))
                                    .child(div().flex_none().child("•"))
                                    .child(div().flex_1().min_w_0().child(
                                        StyledText::new(line).with_highlights([(0..bold, lead)]),
                                    ))
                            })),
                    )
                    .child(
                        canvas(
                            move |_, window, _| {
                                if (measured.max_offset().y > Pixels::ZERO) != overflows {
                                    window.request_animation_frame();
                                }
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .size_0(),
                    )
                    .child(Scrollbar::new("hover-card-notes-scrollbar", scroll).persistent()),
            )
        })
}
