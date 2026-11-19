use std::rc::Rc;

use gpui::{
    AnyElement, App, ElementId, FontWeight, InteractiveElement as _, IntoElement, KeyDownEvent,
    ParentElement as _, Pixels, RenderOnce, Role, SharedString, StatefulInteractiveElement as _,
    Styled as _, Window, div, prelude::FluentBuilder as _, px,
};

use super::{Icon, icon};
use crate::{
    ActiveTheme as _, IndexHandler, PointerCursor as _,
    nav::{self, Axis, Step},
    theme::{LINE_BODY, MIRA_CONTROL, ROW, SYSTEM_MONO, TEXT_BODY, tabular},
};

#[derive(Clone)]
pub struct Column {
    title: SharedString,
    width: Option<Pixels>,
    numeric: bool,
    monospace: bool,
}

impl Column {
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            width: None,
            numeric: false,
            monospace: false,
        }
    }

    pub fn width(mut self, width: Pixels) -> Self {
        self.width = Some(width);
        self
    }

    pub fn numeric(mut self) -> Self {
        self.numeric = true;
        self
    }

    pub fn monospace(mut self) -> Self {
        self.monospace = true;
        self
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Sort {
    pub column: usize,
    pub ascending: bool,
}

#[derive(IntoElement)]
pub struct Table {
    id: ElementId,
    label: SharedString,
    columns: Vec<Column>,
    rows: Vec<Vec<SharedString>>,
    sort: Option<Sort>,
    selected: Option<usize>,
    on_sort: Option<IndexHandler>,
    on_select: Option<IndexHandler>,
    leading: Option<Rc<dyn Fn(Option<usize>) -> AnyElement>>,
}

impl Table {
    pub fn new(
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        columns: Vec<Column>,
        rows: Vec<Vec<SharedString>>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            columns,
            rows,
            sort: None,
            selected: None,
            on_sort: None,
            on_select: None,
            leading: None,
        }
    }

    pub fn sort(mut self, sort: Option<Sort>) -> Self {
        self.sort = sort;
        self
    }

    pub fn selected(mut self, selected: Option<usize>) -> Self {
        self.selected = selected;
        self
    }

    pub fn on_sort(mut self, f: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_sort = Some(Rc::new(f));
        self
    }

    pub fn on_select(mut self, f: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_select = Some(Rc::new(f));
        self
    }

    pub fn leading(mut self, cell: impl Fn(Option<usize>) -> AnyElement + 'static) -> Self {
        self.leading = Some(Rc::new(cell));
        self
    }
}

impl RenderOnce for Table {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let focus = nav::focus_handle((self.id.clone(), "focus"), window, cx);
        let ring = nav::keyboard_focused(&focus, window, cx);
        let theme = cx.theme().clone();
        let mira = theme.palette().is_some();
        let leading = self.leading;
        let shift = usize::from(leading.is_some());
        let leading_cell = |content, header| {
            div()
                .id("leading")
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .w(px(40.0))
                .role(if header {
                    Role::ColumnHeader
                } else {
                    Role::Cell
                })
                .aria_column_index(1)
                .child(content)
        };
        let selected = self.selected;
        let count = self.rows.len();
        let columns = self.columns.clone();
        let on_key = self.on_select.clone();
        let header_focus: Vec<_> = (0..columns.len())
            .map(|index| {
                nav::focus_handle(
                    (self.id.clone(), SharedString::from(format!("sort-{index}"))),
                    window,
                    cx,
                )
            })
            .collect();
        let cell = move |column: &Column, header: bool| {
            div()
                .flex()
                .items_center()
                .gap(px(4.0))
                .px(px(10.0))
                .map(|cell| match column.width {
                    Some(width) => cell.flex_none().w(width),
                    None => cell.flex_1().min_w_0(),
                })
                .when(column.numeric, |cell| {
                    cell.justify_end().font_features(tabular())
                })
                .when(header, |cell| cell.h(px(if mira { 24.0 } else { 30.0 })))
                .when(!header && column.monospace, |cell| {
                    cell.font_family(SYSTEM_MONO)
                })
        };
        div()
            .id(self.id.clone())
            .role(Role::Table)
            .aria_label(self.label)
            .aria_row_count(count + 1)
            .aria_column_count(columns.len() + shift)
            .flex()
            .flex_col()
            .when(!mira, |table| {
                table
                    .rounded(theme.radius.panel)
                    .bg(theme.surface)
                    .border_1()
                    .border_color(theme.hairline)
                    .overflow_hidden()
            })
            .text_size(TEXT_BODY)
            .line_height(LINE_BODY)
            .child(
                div()
                    .id("header")
                    .role(Role::Row)
                    .aria_row_index(1)
                    .flex()
                    .border_b_1()
                    .border_color(theme.hairline)
                    .children(
                        leading
                            .as_ref()
                            .map(|leading| leading_cell(leading(None), true)),
                    )
                    .children(columns.iter().enumerate().map(|(index, column)| {
                        let sorted = self.sort.filter(|sort| sort.column == index);
                        let on_sort = self.on_sort.clone();
                        cell(column, true)
                            .id(("column", index))
                            .role(Role::ColumnHeader)
                            .aria_label(column.title.clone())
                            .aria_column_index(index + shift + 1)
                            .when_some(sorted, |header, sort| {
                                header.aria_description(if sort.ascending {
                                    "Sorted ascending"
                                } else {
                                    "Sorted descending"
                                })
                            })
                            .text_size(theme.secondary_text_size())
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(if sorted.is_some() {
                                theme.text
                            } else {
                                theme.text_tertiary
                            })
                            .when_some(on_sort, |header, on_sort| {
                                let focus = header_focus[index].clone();
                                let ring = nav::keyboard_focused(&focus, window, cx);
                                header
                                    .track_focus(&focus.tab_stop(true))
                                    .relative()
                                    .pointer_cursor(&theme)
                                    .hover(|style| style.text_color(theme.text))
                                    .when(ring, |header| header.child(theme.focus_ring(px(4.0))))
                                    .on_click(move |_, window, cx| on_sort(index, window, cx))
                            })
                            .child(column.title.clone())
                            .when_some(sorted, |header, sort| {
                                header.child(icon(
                                    if sort.ascending {
                                        Icon::ChevronUp
                                    } else {
                                        Icon::ChevronDown
                                    },
                                    theme.text_secondary,
                                ))
                            })
                    })),
            )
            .child(
                div()
                    .id("body")
                    .role(Role::RowGroup)
                    .track_focus(&focus.clone().tab_stop(true))
                    .on_key_down(move |event: &KeyDownEvent, window, cx| {
                        let Some(step) = Step::from_key(event, Axis::Vertical) else {
                            return;
                        };
                        if let (Some(next), Some(on_select)) =
                            (nav::step(selected, count, step, false, |_| true), &on_key)
                        {
                            on_select(next, window, cx);
                        }
                        cx.stop_propagation();
                    })
                    .flex()
                    .flex_col()
                    .children(self.rows.into_iter().enumerate().map(|(index, row)| {
                        let current = selected == Some(index);
                        let on_select = self.on_select.clone();
                        let focus = focus.clone();
                        let label = row.first().cloned().unwrap_or_default();
                        div()
                            .id(("row", index))
                            .role(Role::Row)
                            .aria_label(label)
                            .aria_row_index(index + 2)
                            .aria_selected(current)
                            .when(current, |row| row.aria_active_descendant())
                            .flex()
                            .h(if mira { MIRA_CONTROL } else { ROW })
                            .items_center()
                            .when(index > 0 && !mira, |row| {
                                row.border_t_1().border_color(theme.hairline)
                            })
                            .when(current, |row| row.bg(theme.selected))
                            .when(mira && (current || on_select.is_some()), |row| {
                                row.rounded(theme.radius.item)
                            })
                            .when(mira && on_select.is_some(), |row| {
                                let (hover, press) = theme.mira_item_colors(current);
                                row.hover(move |style| style.bg(hover))
                                    .active(move |style| style.bg(press))
                            })
                            .when(!mira && !current, |row| {
                                row.hover(|style| style.bg(theme.fill))
                            })
                            .when(current && ring && mira, |row| {
                                row.child(theme.focus_ring(theme.radius.item))
                            })
                            .when(current && ring && !mira, |row| {
                                row.child(
                                    div()
                                        .absolute()
                                        .left_0()
                                        .top_0()
                                        .bottom_0()
                                        .w(px(2.0))
                                        .bg(theme.focus),
                                )
                            })
                            .relative()
                            .when_some(on_select, |row, on_select| {
                                row.pointer_cursor(&theme).on_click(move |_, window, cx| {
                                    window.focus(&focus, cx);
                                    on_select(index, window, cx);
                                })
                            })
                            .children(
                                leading
                                    .as_ref()
                                    .map(|leading| leading_cell(leading(Some(index)), false)),
                            )
                            .children(row.into_iter().zip(columns.iter()).enumerate().map(
                                |(column, (text, spec))| {
                                    cell(spec, false)
                                        .id(("cell", column))
                                        .role(Role::Cell)
                                        .aria_label(text.clone())
                                        .aria_column_index(column + shift + 1)
                                        .text_color(if column == 0 {
                                            theme.text
                                        } else {
                                            theme.text_secondary
                                        })
                                        .child(div().truncate().child(text))
                                },
                            ))
                    })),
            )
    }
}
