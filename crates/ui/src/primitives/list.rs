use std::rc::Rc;

use gpui::{
    AnyElement, App, Div, ElementId, FocusHandle, FontWeight, InteractiveElement as _, IntoElement,
    KeyDownEvent, ParentElement, Pixels, RenderOnce, Role, ScrollEvent, ScrollHandle,
    ScrollStrategy, SharedString, Stateful, StatefulInteractiveElement as _, Styled as _,
    UniformListScrollHandle, Window, div, prelude::FluentBuilder as _, px, uniform_list,
};

use super::{Flow, Pose, Scrollbar, Sortable};
use crate::{
    ActiveTheme as _, IndexHandler, PointerCursor as _, StyledExt as _, Theme, h_flex,
    nav::{self, Axis, Step},
    theme::{ROW, tabular},
    v_flex,
};

type ReorderHandler = Rc<dyn Fn(usize, usize, &mut Window, &mut App)>;
type RowBuilder = Rc<dyn Fn(usize) -> ListItem>;

const LIST_INSET: Pixels = px(4.0);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Row,
    Header,
}

pub struct ListItem {
    kind: Kind,
    label: SharedString,
    start: Option<AnyElement>,
    end: Option<AnyElement>,
    description: Option<SharedString>,
    children: Vec<AnyElement>,
    disabled: bool,
}

impl ListItem {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            kind: Kind::Row,
            label: label.into(),
            start: None,
            end: None,
            description: None,
            children: Vec::new(),
            disabled: false,
        }
    }

    pub fn header(label: impl Into<SharedString>) -> Self {
        Self {
            kind: Kind::Header,
            ..Self::new(label)
        }
    }

    pub fn start_slot(mut self, element: impl IntoElement) -> Self {
        self.start = Some(element.into_any_element());
        self
    }

    pub fn end_slot(mut self, element: impl IntoElement) -> Self {
        self.end = Some(element.into_any_element());
        self
    }

    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn disabled(mut self) -> Self {
        self.disabled = true;
        self
    }

    fn selectable(&self) -> bool {
        self.kind == Kind::Row && !self.disabled
    }
}

impl ParentElement for ListItem {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

#[derive(Clone)]
struct Options {
    count: usize,
    row: RowBuilder,
    selected: Option<usize>,
    ring: bool,
    focus: FocusHandle,
    on_select: Option<IndexHandler>,
    on_activate: Option<IndexHandler>,
    uniform: bool,
    theme: Theme,
}

impl Options {
    fn keys(
        &self,
        reveal: impl Fn(usize) + 'static,
    ) -> impl Fn(&KeyDownEvent, &mut Window, &mut App) + 'static {
        let options = self.clone();
        move |event, window, cx| {
            let Options {
                count,
                row,
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
                .and_then(|step| nav::step(*selected, *count, step, false, |i| row(i).selectable()))
                .or_else(|| {
                    nav::typeahead(
                        &event.keystroke.key,
                        (0..*count)
                            .map(|i| (i, row(i)))
                            .filter(|(_, item)| item.selectable())
                            .map(|(i, item)| (i, item.label.to_string())),
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

    fn render(&self, index: usize, pose: Pose) -> Stateful<Div> {
        let item = (self.row)(index);
        match item.kind {
            Kind::Header => self.header(index, item),
            Kind::Row => self.item(index, item, pose),
        }
    }

    fn header(&self, index: usize, item: ListItem) -> Stateful<Div> {
        h_flex()
            .id(index)
            .role(Role::Heading)
            .aria_label(item.label.clone())
            .flex_none()
            .items_end()
            .gap(px(6.0))
            .px(px(8.0))
            .pb(px(2.0))
            .map(|header| {
                if self.uniform {
                    header.h(ROW)
                } else {
                    header.pt(px(if index == 0 { 4.0 } else { 10.0 }))
                }
            })
            .text_caption()
            .font_weight(FontWeight::MEDIUM)
            .text_color(self.theme.text_tertiary)
            .when_some(item.start, |header, start| header.child(start))
            .child(div().flex_1().min_w_0().truncate().child(item.label))
            .when_some(item.end, |header, end| header.child(end))
    }

    fn item(&self, index: usize, item: ListItem, pose: Pose) -> Stateful<Div> {
        let theme = &self.theme;
        let current = self.selected == Some(index) && pose != Pose::Lifted;
        let focus = self.focus.clone();
        let on_select = self.on_select.clone();
        let on_activate = self.on_activate.clone();
        let disabled = item.disabled;
        let quiet = if current {
            theme.text_secondary
        } else {
            theme.text_tertiary
        };
        h_flex()
            .id(index)
            .debug_selector(move || format!("row-{index}"))
            .role(Role::ListBoxOption)
            .aria_label(item.label.clone())
            .when_some(item.description.clone(), |row, description| {
                row.aria_description(description)
            })
            .aria_selected(current)
            .aria_position_in_set(index + 1)
            .aria_size_of_set(self.count)
            .when(current, |row| row.aria_active_descendant())
            .relative()
            .flex_none()
            .gap(px(8.0))
            .min_h(ROW)
            .px(px(8.0))
            .py(px(4.0))
            .rounded(theme.radius.item)
            .text_body()
            .text_color(if disabled {
                theme.text_disabled
            } else {
                theme.text
            })
            .when(current, |row| row.bg(theme.selected))
            .when(current && self.ring, |row| {
                row.child(theme.focus_ring(theme.radius.item))
            })
            .when(!current && !disabled && pose == Pose::Rest, |row| {
                row.hover(|style| style.bg(theme.fill))
            })
            .when(!disabled, |row| {
                row.pointer_cursor(theme)
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
            .when_some(item.start, |row, start| {
                row.child(h_flex().flex_none().child(start))
            })
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .map(|body| {
                        if item.children.is_empty() {
                            body.child(div().truncate().child(item.label))
                        } else {
                            body.children(item.children)
                        }
                    })
                    .when_some(item.description, |body, description| {
                        body.child(
                            div()
                                .truncate()
                                .text_small()
                                .text_color(quiet)
                                .child(description),
                        )
                    }),
            )
            .when_some(item.end, |row, end| {
                row.child(
                    h_flex()
                        .flex_none()
                        .gap(px(4.0))
                        .text_small()
                        .font_features(tabular())
                        .text_color(quiet)
                        .child(end),
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

#[derive(Clone, Copy)]
enum Height {
    Content,
    Fixed(Pixels),
    Fill,
}

#[derive(IntoElement)]
pub struct List {
    id: ElementId,
    label: SharedString,
    count: usize,
    row: RowBuilder,
    selected: Option<usize>,
    height: Height,
    on_select: Option<IndexHandler>,
    on_activate: Option<IndexHandler>,
    on_reorder: Option<ReorderHandler>,
    pinned: usize,
}

impl List {
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
            height: Height::Content,
            on_select: None,
            on_activate: None,
            on_reorder: None,
            pinned: 0,
        }
    }

    pub fn selected(mut self, selected: Option<usize>) -> Self {
        self.selected = selected;
        self
    }

    pub fn height(mut self, height: Pixels) -> Self {
        self.height = Height::Fixed(height);
        self
    }

    pub fn fill(mut self) -> Self {
        self.height = Height::Fill;
        self
    }

    pub fn on_reorder(mut self, f: impl Fn(usize, usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_reorder = Some(Rc::new(f));
        self
    }

    pub fn pin_first(mut self, count: usize) -> Self {
        self.pinned = count;
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

impl RenderOnce for List {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let focus = nav::focus_handle((self.id.clone(), "focus"), window, cx);
        let scroll = window
            .use_keyed_state((self.id.clone(), "scroll"), cx, |_, _| ScrollHandle::new())
            .read(cx)
            .clone();
        let options = Options {
            count: self.count,
            row: self.row,
            selected: self.selected,
            ring: nav::keyboard_focused(&focus, window, cx),
            focus,
            on_select: self.on_select,
            on_activate: self.on_activate,
            uniform: false,
            theme: cx.theme().clone(),
        };
        let reveal = scroll.clone();
        let bar = Scrollbar::new((self.id.clone(), "scrollbar"), scroll.clone());
        let list = list_frame(self.id, self.label, &options)
            .on_key_down(options.keys(move |index| reveal.scroll_to_item(index)))
            .flex()
            .flex_col()
            .gap(px(1.0))
            .p(LIST_INSET)
            .map(|list| match self.height {
                Height::Content => list,
                Height::Fixed(height) => list.h(height).pr(Scrollbar::GUTTER),
                Height::Fill => list.size_full().pr(Scrollbar::GUTTER),
            });
        let count = self.count;
        let list = match self.on_reorder {
            Some(reorder) => Sortable::new(
                list,
                Flow::Column,
                0..count,
                move |(index, pose), _, _| options.render(index, pose).into_any_element(),
                move |from, to, window, cx| reorder(from, to, window, cx),
            )
            .scrolls(&scroll)
            .pin_first(self.pinned)
            .into_any_element(),
            None => list
                .overflow_y_scroll()
                .track_scroll(&scroll)
                .children((0..count).map(|index| options.render(index, Pose::Rest)))
                .into_any_element(),
        };
        div()
            .relative()
            .when(matches!(self.height, Height::Fill), |root| {
                root.flex_1().min_h_0()
            })
            .on_scroll(contain_scroll)
            .child(list)
            .child(bar)
    }
}

#[derive(IntoElement)]
pub struct VirtualList {
    id: ElementId,
    label: SharedString,
    count: usize,
    row: RowBuilder,
    selected: Option<usize>,
    height: Height,
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
            height: Height::Fixed(px(320.0)),
            on_select: None,
            on_activate: None,
        }
    }

    pub fn selected(mut self, selected: Option<usize>) -> Self {
        self.selected = selected;
        self
    }

    pub fn height(mut self, height: Pixels) -> Self {
        self.height = Height::Fixed(height);
        self
    }

    pub fn fill(mut self) -> Self {
        self.height = Height::Fill;
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
            row: self.row,
            selected: self.selected,
            ring: nav::keyboard_focused(&focus, window, cx),
            focus,
            on_select: self.on_select,
            on_activate: self.on_activate,
            uniform: true,
            theme: cx.theme().clone(),
        };
        let shown = window.use_keyed_state((self.id.clone(), "shown"), cx, |_, _| None);
        if self.selected.is_some() && *shown.read(cx) != self.selected {
            shown.update(cx, |shown, _| *shown = self.selected);
            if let Some(selected) = self.selected {
                scroll.scroll_to_item(selected, ScrollStrategy::Nearest);
            }
        }
        let reveal = scroll.clone();
        let render_options = options.clone();
        let bar = Scrollbar::new(
            (self.id.clone(), "scrollbar"),
            scroll.0.borrow().base_handle.clone(),
        );
        list_frame(self.id.clone(), self.label, &options)
            .relative()
            .on_scroll(contain_scroll)
            .on_key_down(
                options.keys(move |index| reveal.scroll_to_item(index, ScrollStrategy::Nearest)),
            )
            .map(|list| match self.height {
                Height::Fixed(height) => list.h(height),
                Height::Content | Height::Fill => list.flex_1().min_h_0(),
            })
            .child(
                uniform_list((self.id, "rows"), self.count, move |range, _, _| {
                    range
                        .map(|index| {
                            div()
                                .pb(px(1.0))
                                .child(render_options.render(index, Pose::Rest))
                        })
                        .collect()
                })
                .track_scroll(&scroll)
                .size_full()
                .p(LIST_INSET)
                .pr(Scrollbar::GUTTER),
            )
            .child(bar)
    }
}
