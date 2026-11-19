use gpui::{
    AnyElement, App, FontWeight, IntoElement, ParentElement, RenderOnce, SharedString, Styled as _,
    Window, div, prelude::FluentBuilder as _, px,
};

use crate::{ActiveTheme as _, StyledExt as _, h_flex, primitives::label::caption, v_flex};

#[derive(IntoElement, Default)]
pub struct Group {
    title: Option<SharedString>,
    header_end: Option<AnyElement>,
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

    pub fn header_end(mut self, element: impl IntoElement) -> Self {
        self.header_end = Some(element.into_any_element());
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
        let layout = theme.layout;
        let typography = theme.typography;
        v_flex()
            .gap(theme.space(2.0))
            .when(self.title.is_some() || self.header_end.is_some(), |group| {
                group.child(
                    h_flex()
                        .justify_between()
                        .gap(theme.space(2.0))
                        .px(layout.space(3.0))
                        .child(
                            div()
                                .text_title(typography)
                                .text_color(theme.foreground)
                                .children(self.title),
                        )
                        .children(self.header_end.map(|end| {
                            div()
                                .text_caption(typography)
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.muted_foreground)
                                .child(end)
                        })),
                )
            })
            .child(
                v_flex()
                    .rounded(theme.radius.panel)
                    .bg(theme.card)
                    .child(theme.surface_ring(theme.radius.panel))
                    .children(self.children.into_iter().enumerate().map(|(index, row)| {
                        div()
                            .when(index > 0, |row| row.border_t_1().border_color(theme.border))
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
        let layout = theme.layout;
        let typography = theme.typography;
        let caption = caption(self.label, self.description, cx);
        h_flex()
            .justify_between()
            .flex_wrap()
            .gap(theme.space(2.0))
            .min_h(theme.layout.toolbar)
            .py(layout.space(1.5))
            .px(layout.space(3.0))
            .text_body(typography)
            .text_color(theme.foreground)
            .child(div().flex_1().min_w(px(120.0)).child(caption))
            .child(
                h_flex()
                    .flex_none()
                    .ml_auto()
                    .gap(theme.space(2.0))
                    .children(self.children),
            )
    }
}
