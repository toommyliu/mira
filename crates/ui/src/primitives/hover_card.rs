use gpui::{
    Anchor, AnyElement, App, Bounds, ElementId, FontWeight, InteractiveElement as _, IntoElement,
    ParentElement, Pixels, RenderOnce, SharedString, Styled as _, Window, anchored, deferred, div,
    point, px,
};

use super::tooltip;
use crate::{
    ActiveTheme as _, Tone,
    theme::{LINE_BODY, LINE_SMALL, TEXT_BODY, TEXT_SMALL},
};

const GAP: Pixels = px(6.0);
const MARGIN: Pixels = px(8.0);
const MAX_LINES: usize = 3;

struct Card {
    title: SharedString,
    rows: Vec<(SharedString, SharedString, Tone)>,
}

#[derive(IntoElement)]
pub struct HoverCard {
    id: ElementId,
    card: Card,
    children: Vec<AnyElement>,
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
        }
    }

    pub fn row(
        mut self,
        label: impl Into<SharedString>,
        text: impl Into<SharedString>,
        tone: Tone,
    ) -> Self {
        self.card.rows.push((label.into(), text.into(), tone));
        self
    }
}

impl ParentElement for HoverCard {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for HoverCard {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let trigger = div()
            .id(self.id.clone())
            .flex()
            .flex_1()
            .min_w_0()
            .children(self.children);
        let card = self.card;
        tooltip::hover(
            trigger,
            (self.id, "card").into(),
            false,
            window,
            cx,
            move |at, _, cx| popup(card, at, cx),
        )
    }
}

fn popup(card: Card, trigger: Bounds<Pixels>, cx: &App) -> AnyElement {
    let theme = cx.theme();
    deferred(
        anchored()
            .position(point(trigger.right() + GAP, trigger.center().y))
            .anchor(Anchor::LeftCenter)
            .snap_to_window_with_margin(MARGIN)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.0))
                    .w(px(260.0))
                    .px(px(12.0))
                    .py(px(10.0))
                    .rounded(theme.radius.panel)
                    .bg(theme.overlay)
                    .border_1()
                    .border_color(theme.border)
                    .shadow(theme.floating())
                    .child(
                        div()
                            .mb(px(2.0))
                            .truncate()
                            .text_size(TEXT_BODY)
                            .line_height(LINE_BODY)
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.text)
                            .child(card.title),
                    )
                    .children(card.rows.into_iter().map(|(label, text, tone)| {
                        div()
                            .flex()
                            .gap(px(12.0))
                            .text_size(TEXT_SMALL)
                            .line_height(LINE_SMALL)
                            .child(
                                div()
                                    .flex_none()
                                    .w(px(44.0))
                                    .text_color(theme.text_tertiary)
                                    .child(label),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .line_clamp(MAX_LINES)
                                    .text_ellipsis()
                                    .text_color(match tone {
                                        Tone::Neutral => theme.text,
                                        tone => theme.tone(tone).2,
                                    })
                                    .child(text),
                            )
                    })),
            ),
    )
    .with_priority(super::layer::TOOLTIP)
    .into_any_element()
}
