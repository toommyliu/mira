use std::rc::Rc;

use gpui::{
    App, ElementId, FontWeight, InteractiveElement as _, IntoElement, KeyDownEvent,
    ParentElement as _, RenderOnce, Role, SharedString, StatefulInteractiveElement as _,
    Styled as _, Window, div, prelude::FluentBuilder as _, px,
};

use crate::{
    ActiveTheme as _, IndexHandler, PointerCursor as _,
    nav::{self, Axis, Step},
    theme::TEXT_BODY,
};

#[derive(IntoElement)]
pub struct Tabs {
    id: ElementId,
    label: SharedString,
    tabs: Vec<SharedString>,
    selected: usize,
    on_select: Option<IndexHandler>,
}

impl Tabs {
    pub fn new(
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        tabs: Vec<SharedString>,
        selected: usize,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            tabs,
            selected,
            on_select: None,
        }
    }

    pub fn on_select(mut self, f: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_select = Some(Rc::new(f));
        self
    }
}

impl RenderOnce for Tabs {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let focus = nav::focus_handle((self.id.clone(), "focus"), window, cx);
        let ring = nav::keyboard_focused(&focus, window, cx);
        let theme = cx.theme().clone();
        let (selected, count) = (self.selected, self.tabs.len());
        let on_key = self.on_select.clone();
        div()
            .id(self.id.clone())
            .role(Role::TabList)
            .aria_label(self.label)
            .track_focus(&focus.clone().tab_stop(true))
            .on_key_down(move |event: &KeyDownEvent, window, cx| {
                let Some(step) = Step::from_key(event, Axis::Horizontal) else {
                    return;
                };
                if let (Some(next), Some(on_select)) = (
                    nav::step(Some(selected), count, step, true, |_| true),
                    &on_key,
                ) {
                    on_select(next, window, cx);
                }
                cx.stop_propagation();
            })
            .flex()
            .gap(px(20.0))
            .border_b_1()
            .border_color(theme.hairline)
            .children(self.tabs.into_iter().enumerate().map(|(index, tab)| {
                let current = index == selected;
                let on_select = self.on_select.clone();
                let focus = focus.clone();
                div()
                    .id(index)
                    .role(Role::Tab)
                    .aria_label(tab.clone())
                    .aria_selected(current)
                    .aria_position_in_set(index + 1)
                    .aria_size_of_set(count)
                    .when(current, |tab| tab.aria_active_descendant())
                    .relative()
                    .flex()
                    .items_center()
                    .h(px(34.0))
                    .text_size(TEXT_BODY)
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(if current {
                        theme.text
                    } else {
                        theme.text_tertiary
                    })
                    .pointer_cursor(&theme)
                    .when(!current, |tab| {
                        tab.hover(|style| style.text_color(theme.text_secondary))
                    })
                    .when_some(on_select, |tab, on_select| {
                        tab.on_click(move |_, window, cx| {
                            window.focus(&focus, cx);
                            on_select(index, window, cx);
                        })
                    })
                    .child(
                        div()
                            .relative()
                            .px(px(2.0))
                            .child(tab)
                            .when(current && ring, |label| {
                                label.child(theme.focus_ring(px(4.0)))
                            }),
                    )
                    .when(current, |tab| {
                        tab.child(
                            div()
                                .absolute()
                                .left_0()
                                .right_0()
                                .bottom(px(-1.0))
                                .h(px(2.0))
                                .rounded_full()
                                .bg(theme.text),
                        )
                    })
            }))
    }
}
