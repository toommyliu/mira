use std::rc::Rc;

use gpui::{
    App, Div, ElementId, FocusHandle, InteractiveElement as _, IntoElement, KeyDownEvent,
    ParentElement as _, Pixels, RenderOnce, Role, ScrollEvent, ScrollHandle, ScrollStrategy,
    SharedString, Stateful, StatefulInteractiveElement as _, Styled as _, UniformListScrollHandle,
    Window, div, prelude::FluentBuilder as _, px, uniform_list,
};

use super::Scrollbar;
use crate::{
    ActiveTheme as _, IndexHandler, PointerCursor as _, Theme,
    nav::{self, Axis, Step},
    theme::{LINE_BODY, LINE_SMALL, ROW, TEXT_BODY, TEXT_SMALL, tabular},
};

#[derive(Clone)]
pub struct ListItem {
    title: SharedString,
    detail: Option<SharedString>,
    meta: Option<SharedString>,
    disabled: bool,
}

impl ListItem {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            detail: None,
            meta: None,
            disabled: false,
        }
    }

    pub fn detail(mut self, detail: impl Into<SharedString>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn meta(mut self, meta: impl Into<SharedString>) -> Self {
        self.meta = Some(meta.into());
        self
    }

    pub fn disabled(mut self) -> Self {
        self.disabled = true;
        self
    }
}

#[derive(IntoElement)]
pub struct List {
    id: ElementId,
    label: SharedString,
    items: Vec<ListItem>,
    selected: Option<usize>,
    height: Option<Pixels>,
    on_select: Option<IndexHandler>,
    on_activate: Option<IndexHandler>,
}

impl List {
    pub fn new(
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        items: Vec<ListItem>,
        selected: Option<usize>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            items,
            selected,
            height: None,
            on_select: None,
            on_activate: None,
        }
    }

    pub fn height(mut self, height: Pixels) -> Self {
        self.height = Some(height);
        self
    }

    pub fn on_select(mut self, f: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_select = Some(Rc::new(f));
        self
    }

    pub fn on_activate(mut self, f: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_activate = Some(Rc::new(f));
        self
    }
}

#[derive(Clone)]
struct Options {
    count: usize,
    selected: Option<usize>,
    ring: bool,
    focus: FocusHandle,
    on_select: Option<IndexHandler>,
    on_activate: Option<IndexHandler>,
    theme: Theme,
}

impl Options {
    fn keys(
        &self,
        item: impl Fn(usize) -> ListItem + 'static,
        reveal: impl Fn(usize) + 'static,
    ) -> impl Fn(&KeyDownEvent, &mut Window, &mut App) + 'static {
        let options = self.clone();
        move |event, window, cx| {
            let Options {
                count,
                selected,
                on_select,
                on_activate,
                ..
            } = &options;
            if event.keystroke.key == "enter" {
                if let (Some(index), Some(on_activate)) = (selected, on_activate) {
                    on_activate(*index, window, cx);
                    cx.stop_propagation();
                }
                return;
            }
            let next = Step::from_key(event, Axis::Vertical)
                .and_then(|step| nav::step(*selected, *count, step, false, |i| !item(i).disabled))
                .or_else(|| {
                    nav::typeahead(
                        &event.keystroke.key,
                        (0..*count)
                            .map(|i| (i, item(i)))
                            .filter(|(_, item)| !item.disabled)
                            .map(|(i, item)| (i, item.title.to_string())),
                        *selected,
                    )
                });
            let Some(next) = next else {
                return;
            };
            reveal(next);
            if let Some(on_select) = on_select {
                on_select(next, window, cx);
            }
            cx.stop_propagation();
        }
    }

    fn row(&self, index: usize, item: ListItem) -> Stateful<Div> {
        let theme = &self.theme;
        let current = self.selected == Some(index);
        let focus = self.focus.clone();
        let on_select = self.on_select.clone();
        let on_activate = self.on_activate.clone();
        let disabled = item.disabled;
        div()
            .id(index)
            .role(Role::ListBoxOption)
            .aria_label(item.title.clone())
            .when_some(item.detail.clone(), |row, detail| {
                row.aria_description(detail)
            })
            .aria_selected(current)
            .aria_position_in_set(index + 1)
            .aria_size_of_set(self.count)
            .when(current, |row| row.aria_active_descendant())
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(12.0))
            .min_h(ROW)
            .px(px(10.0))
            .py(px(5.0))
            .rounded(theme.radius.item)
            .when(current, |row| row.bg(theme.selected))
            .when(current && self.ring, |row| {
                row.child(theme.focus_ring(theme.radius.item))
            })
            .when(!current && !disabled, |row| {
                row.hover(|style| style.bg(theme.fill))
            })
            .when(!disabled, |row| {
                row.pointer_cursor(&theme)
                    .on_click(move |event, window, cx| {
                        window.focus(&focus, cx);
                        if event.click_count() >= 2
                            && let Some(on_activate) = &on_activate
                        {
                            on_activate(index, window, cx);
                        } else if let Some(on_select) = &on_select {
                            on_select(index, window, cx);
                        }
                    })
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .truncate()
                            .text_size(TEXT_BODY)
                            .line_height(LINE_BODY)
                            .text_color(if disabled {
                                theme.text_disabled
                            } else {
                                theme.text
                            })
                            .child(item.title),
                    )
                    .when_some(item.detail, |column, detail| {
                        column.child(
                            div()
                                .truncate()
                                .text_size(TEXT_SMALL)
                                .line_height(LINE_SMALL)
                                .text_color(theme.text_tertiary)
                                .child(detail),
                        )
                    }),
            )
            .when_some(item.meta, |row, meta| {
                row.child(
                    div()
                        .flex_none()
                        .text_size(TEXT_SMALL)
                        .font_features(tabular())
                        .text_color(theme.text_tertiary)
                        .child(meta),
                )
            })
    }
}

fn contain_scroll(_: &ScrollEvent, _: &mut Window, cx: &mut App) {
    cx.stop_propagation();
}

fn list_frame(id: ElementId, label: SharedString, options: &Options) -> Stateful<Div> {
    div()
        .id(id)
        .role(Role::ListBox)
        .aria_label(label)
        .track_focus(&options.focus.clone().tab_stop(true))
        .rounded(options.theme.radius.panel)
        .bg(options.theme.surface)
        .border_1()
        .border_color(options.theme.hairline)
}

impl RenderOnce for List {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let focus = nav::focus_handle((self.id.clone(), "focus"), window, cx);
        let scroll = window
            .use_keyed_state((self.id.clone(), "scroll"), cx, |_, _| ScrollHandle::new())
            .read(cx)
            .clone();
        let options = Options {
            count: self.items.len(),
            selected: self.selected,
            ring: nav::keyboard_focused(&focus, window),
            focus,
            on_select: self.on_select,
            on_activate: self.on_activate,
            theme: cx.theme().clone(),
        };
        let items: Rc<[ListItem]> = self.items.into();
        let reveal = scroll.clone();
        let lookup = items.clone();
        let bar = Scrollbar::new((self.id.clone(), "scrollbar"), scroll.clone());
        let list = list_frame(self.id, self.label, &options)
            .on_key_down(options.keys(
                move |index| lookup[index].clone(),
                move |index| reveal.scroll_to_item(index),
            ))
            .flex()
            .flex_col()
            .gap(px(1.0))
            .p(px(4.0))
            .when_some(self.height, |list, height| list.h(height))
            .overflow_y_scroll()
            .track_scroll(&scroll)
            .children(
                items
                    .iter()
                    .enumerate()
                    .map(|(index, item)| options.row(index, item.clone())),
            );
        div()
            .relative()
            .on_scroll(contain_scroll)
            .child(list)
            .child(bar)
    }
}

type RowSource = Rc<dyn Fn(usize) -> ListItem>;

#[derive(IntoElement)]
pub struct VirtualList {
    id: ElementId,
    label: SharedString,
    count: usize,
    row: RowSource,
    selected: Option<usize>,
    height: Pixels,
    on_select: Option<IndexHandler>,
    on_activate: Option<IndexHandler>,
}

impl VirtualList {
    pub fn new(
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        count: usize,
        row: impl Fn(usize) -> ListItem + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            count,
            row: Rc::new(row),
            selected: None,
            height: px(320.0),
            on_select: None,
            on_activate: None,
        }
    }

    pub fn selected(mut self, selected: Option<usize>) -> Self {
        self.selected = selected;
        self
    }

    pub fn height(mut self, height: Pixels) -> Self {
        self.height = height;
        self
    }

    pub fn on_select(mut self, f: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_select = Some(Rc::new(f));
        self
    }

    pub fn on_activate(mut self, f: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_activate = Some(Rc::new(f));
        self
    }
}

impl RenderOnce for VirtualList {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let focus = nav::focus_handle((self.id.clone(), "focus"), window, cx);
        let scroll = window
            .use_keyed_state((self.id.clone(), "scroll"), cx, |_, _| {
                UniformListScrollHandle::new()
            })
            .read(cx)
            .clone();
        let options = Options {
            count: self.count,
            selected: self.selected,
            ring: nav::keyboard_focused(&focus, window),
            focus,
            on_select: self.on_select,
            on_activate: self.on_activate,
            theme: cx.theme().clone(),
        };
        let reveal = scroll.clone();
        let lookup = self.row.clone();
        let render_options = options.clone();
        let source = self.row;
        let bar = Scrollbar::new(
            (self.id.clone(), "scrollbar"),
            scroll.0.borrow().base_handle.clone(),
        );
        list_frame(self.id.clone(), self.label, &options)
            .relative()
            .on_scroll(contain_scroll)
            .on_key_down(options.keys(
                move |index| lookup(index),
                move |index| reveal.scroll_to_item(index, ScrollStrategy::Nearest),
            ))
            .h(self.height)
            .child(
                uniform_list((self.id, "rows"), self.count, move |range, _, _| {
                    range
                        .map(|index| render_options.row(index, source(index)).mb(px(1.0)))
                        .collect()
                })
                .track_scroll(&scroll)
                .size_full()
                .p(px(4.0)),
            )
            .child(bar)
    }
}
