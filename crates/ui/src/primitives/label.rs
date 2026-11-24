use std::rc::Rc;

use gpui::{
    AnyElement, App, ElementId, FocusHandle, InteractiveElement as _, IntoElement, MouseButton,
    ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
    prelude::FluentBuilder as _,
};

use crate::{ActiveTheme as _, PointerCursor as _, nav, theme::DISABLED_OPACITY};

pub(crate) type ToggleHandler = Rc<dyn Fn(bool, &mut Window, &mut App)>;

pub(crate) fn labelled(
    id: ElementId,
    label: SharedString,
    description: Option<SharedString>,
    disabled: bool,
    focus: Option<FocusHandle>,
    window: &mut Window,
    cx: &mut App,
) -> (gpui::Stateful<gpui::Div>, bool) {
    let layout = cx.theme().layout;
    let typography = cx.theme().typography;
    let focus = focus.unwrap_or_else(|| nav::focus_handle((id.clone(), "focus"), window, cx));
    let ring = !disabled && nav::keyboard_focused(&focus, window, cx);
    let theme = cx.theme();
    let row = div()
        .id(id)
        .aria_label(label)
        .when_some(description, |row, description| {
            row.aria_description(description)
        })
        .when(!disabled, |row| {
            row.track_focus(&focus.tab_stop(true)).pointer_cursor(theme)
        })
        .flex()
        .items_start()
        .gap(layout.space(2.0))
        .min_h(layout.control)
        .py(theme.space(1.0))
        .text_size(typography.body)
        .line_height(typography.body_line)
        .text_color(theme.foreground)
        .when(disabled, |row| row.opacity(DISABLED_OPACITY));
    (row, ring)
}

pub(crate) fn caption(
    label: SharedString,
    description: Option<SharedString>,
    cx: &App,
) -> impl IntoElement {
    caption_with(label, description, false, None, cx)
}

pub(crate) fn caption_with(
    label: SharedString,
    description: Option<SharedString>,
    invalid: bool,
    accessory: Option<AnyElement>,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    let layout = theme.layout;
    let typography = theme.typography;
    let text = div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .font_weight(gpui::FontWeight::MEDIUM)
        .gap(theme.space(0.5))
        .child(label)
        .when_some(description, |column, description| {
            column.child(
                div()
                    .font_weight(gpui::FontWeight::NORMAL)
                    .text_size(typography.secondary)
                    .line_height(typography.secondary_line)
                    .text_color(if invalid {
                        theme.destructive
                    } else {
                        theme.muted_foreground
                    })
                    .child(description),
            )
        });
    div()
        .flex()
        .flex_1()
        .min_w_0()
        .items_center()
        .gap(layout.space(2.0))
        .child(text)
        .when_some(accessory, |row, accessory| {
            row.child(
                div()
                    .flex_none()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(accessory),
            )
        })
}
