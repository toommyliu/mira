use gpui::{
    AnyElement, App, FontWeight, IntoElement, ParentElement, RenderOnce, SharedString, Styled as _,
    Window, div, prelude::FluentBuilder as _, px,
};

use crate::{ActiveTheme as _, StyledExt as _, h_flex, primitives::label::caption, v_flex};

#[derive(IntoElement, Default)]
pub struct Group {
    title: Option<SharedString>,
    children: Vec<AnyElement>,
}

impl Group {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn row(
        self,
        label: impl Into<SharedString>,
        description: Option<SharedString>,
        control: impl IntoElement,
    ) -> Self {
        let row = FieldRow::new(label).child(control);
        self.child(match description {
            Some(description) => row.description(description),
            None => row,
        })
    }
}

impl ParentElement for Group {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Group {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme().clone();
        v_flex()
            .gap(px(6.0))
            .when_some(self.title, |group, title| {
                group.child(
                    div()
                        .px(px(12.0))
                        .text_small()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.text_secondary)
                        .child(title),
                )
            })
            .child(
                v_flex()
                    .rounded(theme.radius.panel)
                    .bg(theme.surface)
                    .border_1()
                    .border_color(theme.hairline)
                    .children(self.children.into_iter().enumerate().map(|(index, row)| {
                        div()
                            .when(index > 0, |row| {
                                row.border_t_1().border_color(theme.hairline)
                            })
                            .child(row)
                    })),
            )
    }
}

#[derive(IntoElement)]
pub struct FieldRow {
    label: SharedString,
    description: Option<SharedString>,
    children: Vec<AnyElement>,
}

impl FieldRow {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            description: None,
            children: Vec::new(),
        }
    }

    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }
}

impl ParentElement for FieldRow {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for FieldRow {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        h_flex()
            .justify_between()
            .gap(px(16.0))
            .min_h(px(34.0))
            .py(px(6.0))
            .px(px(12.0))
            .text_body()
            .text_color(theme.text)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(caption(self.label, self.description, cx)),
            )
            .child(h_flex().flex_none().gap(px(6.0)).children(self.children))
    }
}
