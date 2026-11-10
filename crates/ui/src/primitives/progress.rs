use gpui::{
    App, ElementId, InteractiveElement as _, IntoElement, ParentElement as _, RenderOnce, Role,
    SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
    prelude::FluentBuilder as _, px, relative,
};

use crate::{
    ActiveTheme as _, motion,
    theme::{LINE_SMALL, TEXT_SMALL, tabular},
};

#[derive(IntoElement)]
pub struct Progress {
    id: ElementId,
    label: SharedString,
    value: Option<f32>,
    detail: Option<SharedString>,
}

impl Progress {
    pub fn new(
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        value: Option<f32>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            value,
            detail: None,
        }
    }

    pub fn detail(mut self, detail: impl Into<SharedString>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

impl RenderOnce for Progress {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let shown = self.value.map(|value| {
            motion::travel((self.id.clone(), "fill"), value.clamp(0.0, 1.0), window, cx)
        });
        let reduce = cx.reduce_motion();
        let theme = cx.theme().clone();
        let track = div()
            .relative()
            .w_full()
            .h(px(4.0))
            .rounded_full()
            .overflow_hidden()
            .bg(theme.border);
        div()
            .id(self.id.clone())
            .role(Role::ProgressIndicator)
            .aria_label(self.label.clone())
            .when_some(self.value, |bar, value| {
                bar.aria_numeric_value(f64::from(value * 100.0))
                    .aria_min_numeric_value(0.0)
                    .aria_max_numeric_value(100.0)
            })
            .when_some(self.detail.clone(), |bar, detail| bar.aria_value(detail))
            .flex()
            .flex_col()
            .gap(px(6.0))
            .w_full()
            .child(
                div()
                    .flex()
                    .justify_between()
                    .text_size(TEXT_SMALL)
                    .line_height(LINE_SMALL)
                    .child(div().text_color(theme.text).child(self.label))
                    .when_some(self.detail, |row, detail| {
                        row.child(
                            div()
                                .font_features(tabular())
                                .text_color(theme.text_tertiary)
                                .child(detail),
                        )
                    }),
            )
            .child(match shown {
                Some(fraction) => track
                    .child(
                        div()
                            .h_full()
                            .w(relative(fraction))
                            .rounded_full()
                            .bg(theme.accent),
                    )
                    .into_any_element(),
                None if reduce => track
                    .child(div().h_full().w_full().bg(theme.accent.opacity(0.5)))
                    .into_any_element(),
                None => track
                    .child(motion::sweep(
                        (self.id, "sweep"),
                        div()
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .w(relative(0.3))
                            .rounded_full()
                            .bg(theme.accent),
                        |bar, t| bar.left(relative(-0.3 + 1.3 * t)),
                    ))
                    .into_any_element(),
            })
    }
}
