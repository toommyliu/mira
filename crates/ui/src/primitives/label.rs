use std::rc::Rc;

use gpui::{
    App, ElementId, InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window, div, prelude::FluentBuilder as _, px,
};

use crate::{
    ActiveTheme as _, PointerCursor as _, nav,
    theme::{DISABLED_OPACITY, LINE_BODY, LINE_SMALL, TEXT_BODY, TEXT_SMALL},
};

pub(crate) type ToggleHandler = Rc<dyn Fn(bool, &mut Window, &mut App)>;

pub(crate) fn labelled(
    id: ElementId,
    label: SharedString,
    description: Option<SharedString>,
    disabled: bool,
    window: &mut Window,
    cx: &mut App,
) -> (gpui::Stateful<gpui::Div>, bool) {
    let focus = nav::focus_handle((id.clone(), "focus"), window, cx);
    let ring = !disabled && nav::keyboard_focused(&focus, window, cx);
    let theme = cx.theme();
    let row = div()
        .id(id)
        .aria_label(label)
        .when_some(description, |row, description| {
            row.aria_description(description)
        })
        .when(!disabled, |row| {
            row.track_focus(&focus.tab_stop(true))
                .pointer_cursor(&theme)
        })
        .flex()
        .items_start()
        .gap(px(8.0))
        .py(px(3.0))
        .text_size(TEXT_BODY)
        .line_height(LINE_BODY)
        .text_color(theme.text)
        .when(disabled, |row| row.opacity(DISABLED_OPACITY));
    (row, ring)
}

pub(crate) fn caption(
    label: SharedString,
    description: Option<SharedString>,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    div()
        .flex()
        .flex_col()
        .child(label)
        .when_some(description, |column, description| {
            column.child(
                div()
                    .text_size(TEXT_SMALL)
                    .line_height(LINE_SMALL)
                    .text_color(theme.text_tertiary)
                    .child(description),
            )
        })
}
