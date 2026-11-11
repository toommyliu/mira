use gpui::{
    AnyElement, App, FontWeight, IntoElement, ParentElement as _, RenderOnce, SharedString,
    Styled as _, Window, div, prelude::FluentBuilder as _, px,
};

use crate::{
    ActiveTheme as _,
    primitives::label::caption,
    theme::{LINE_BODY, LINE_SMALL, TEXT_BODY, TEXT_SMALL},
};

#[derive(IntoElement)]
pub struct SettingsGroup {
    title: Option<SharedString>,
    rows: Vec<AnyElement>,
}

impl SettingsGroup {
    pub fn new() -> Self {
        Self {
            title: None,
            rows: Vec::new(),
        }
    }

    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn row(
        mut self,
        label: impl Into<SharedString>,
        description: Option<SharedString>,
        control: impl IntoElement,
    ) -> Self {
        self.rows.push(
            SettingRow {
                label: label.into(),
                description,
                control: control.into_any_element(),
            }
            .into_any_element(),
        );
        self
    }
}

#[derive(IntoElement)]
struct SettingRow {
    label: SharedString,
    description: Option<SharedString>,
    control: AnyElement,
}

impl RenderOnce for SettingRow {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(24.0))
            .min_h(px(44.0))
            .py(px(8.0))
            .px(px(14.0))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(TEXT_BODY)
                    .line_height(LINE_BODY)
                    .text_color(theme.text)
                    .child(caption(self.label, self.description, cx)),
            )
            .child(div().flex_none().child(self.control))
    }
}

impl RenderOnce for SettingsGroup {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme().clone();
        div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .when_some(self.title, |group, title| {
                group.child(
                    div()
                        .px(px(14.0))
                        .text_size(TEXT_SMALL)
                        .line_height(LINE_SMALL)
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.text_secondary)
                        .child(title),
                )
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .rounded(theme.radius.panel)
                    .bg(theme.surface)
                    .border_1()
                    .border_color(theme.hairline)
                    .children(self.rows.into_iter().enumerate().map(|(index, row)| {
                        div()
                            .when(index > 0, |row| {
                                row.border_t_1().border_color(theme.hairline)
                            })
                            .child(row)
                    })),
            )
    }
}
