use std::rc::Rc;

use gpui::{
    AnyElement, App, Div, ElementId, Entity, FontWeight, InteractiveElement as _, IntoElement,
    KeyDownEvent, ParentElement, RenderOnce, Role, ScrollHandle, SharedString, Stateful,
    StatefulInteractiveElement as _, Styled as _, Window, div, prelude::FluentBuilder as _, px,
};

use super::{
    Button, ContextMenu, Flow, Icon, MenuItem, Pose, Sortable,
    button::Size,
    menu::{self, Handler, ItemsBuilder, MenuState},
};
use crate::{
    ActiveTheme as _, IndexHandler, PointerCursor as _, StyledExt as _, Theme, h_flex,
    nav::{self, Axis, Step},
    v_flex,
};

type TabBuilder = Rc<dyn Fn(usize) -> Tab>;
type ReorderHandler = Rc<dyn Fn(usize, usize, &mut Window, &mut App)>;

const GROUP: &str = "tab";

pub struct Tab {
    label: SharedString,
    start: Option<AnyElement>,
    end: Option<AnyElement>,
    description: Option<SharedString>,
    children: Vec<AnyElement>,
    on_close: Option<Handler>,
    menu: Option<ItemsBuilder>,
}

impl Tab {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            start: None,
            end: None,
            description: None,
            children: Vec::new(),
            on_close: None,
            menu: None,
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

    pub fn on_close(mut self, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_close = Some(Rc::new(f));
        self
    }

    pub fn menu(mut self, f: impl Fn(&mut Window, &mut App) -> Vec<MenuItem> + 'static) -> Self {
        self.menu = Some(Rc::new(f));
        self
    }
}

impl ParentElement for Tab {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl From<SharedString> for Tab {
    fn from(label: SharedString) -> Self {
        Self::new(label)
    }
}

impl From<&'static str> for Tab {
    fn from(label: &'static str) -> Self {
        Self::new(label)
    }
}

#[derive(IntoElement)]
pub struct Tabs {
    id: ElementId,
    label: SharedString,
    count: usize,
    tab: TabBuilder,
    selected: usize,
    vertical: bool,
    scroll: Option<ScrollHandle>,
    on_select: Option<IndexHandler>,
    on_reorder: Option<ReorderHandler>,
}

impl Tabs {
    pub fn new(
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        count: usize,
        tab: impl Fn(usize) -> Tab + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            count,
            tab: Rc::new(tab),
            selected: 0,
            vertical: false,
            scroll: None,
            on_select: None,
            on_reorder: None,
        }
    }

    pub fn selected(mut self, selected: usize) -> Self {
        self.selected = selected;
        self
    }

    pub fn vertical(mut self) -> Self {
        self.vertical = true;
        self
    }

    pub fn track_scroll(mut self, handle: &ScrollHandle) -> Self {
        self.scroll = Some(handle.clone());
        self
    }

    pub fn on_select(mut self, f: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_select = Some(Rc::new(f));
        self
    }

    pub fn on_reorder(mut self, f: impl Fn(usize, usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_reorder = Some(Rc::new(f));
        self
    }
}

#[derive(Clone)]
struct Render {
    tab: TabBuilder,
    selected: usize,
    count: usize,
    vertical: bool,
    ring: bool,
    focus: gpui::FocusHandle,
    menu: Entity<MenuState>,
    on_select: Option<IndexHandler>,
    theme: Theme,
}

impl Render {
    fn tab(&self, index: usize, pose: Pose) -> AnyElement {
        let mut tab = (self.tab)(index);
        let current = index == self.selected && pose != Pose::Lifted;
        let theme = &self.theme;
        let focus = self.focus.clone();
        let on_select = self.on_select.clone();
        let base = div()
            .id(index)
            .role(Role::Tab)
            .aria_label(tab.label.clone())
            .when_some(tab.description.clone(), |tab, description| {
                tab.aria_description(description)
            })
            .aria_selected(current)
            .aria_position_in_set(index + 1)
            .aria_size_of_set(self.count)
            .when(current, |tab| tab.aria_active_descendant())
            .relative()
            .pointer_cursor(theme)
            .when_some(
                on_select.filter(|_| pose == Pose::Rest),
                |tab, on_select| {
                    tab.on_click(move |_, window, cx| {
                        window.focus(&focus, cx);
                        on_select(index, window, cx);
                    })
                },
            );
        let label = tab.label.clone();
        let menu = tab.menu.take().filter(|_| pose != Pose::Lifted);
        let row = if self.vertical {
            self.vertical_tab(base, tab, index, current, pose)
        } else {
            self.horizontal_tab(base, tab, current)
        };
        match menu {
            Some(items) => ContextMenu::new(("menu", index), label)
                .items(move |window, cx| items(window, cx))
                .when(current, |menu| menu.state(self.menu.clone()))
                .child(row)
                .into_any_element(),
            None => row.into_any_element(),
        }
    }

    fn vertical_tab(
        &self,
        base: Stateful<Div>,
        tab: Tab,
        index: usize,
        current: bool,
        pose: Pose,
    ) -> Stateful<Div> {
        let theme = &self.theme;
        let quiet = if current {
            theme.text_secondary
        } else {
            theme.text_tertiary
        };
        base.flex()
            .flex_none()
            .items_start()
            .gap(px(8.0))
            .px(px(8.0))
            .py(px(6.0))
            .rounded(theme.radius.item)
            .group(GROUP)
            .text_body()
            .text_color(if current {
                theme.text
            } else {
                theme.text_secondary
            })
            .when(current, |tab| tab.bg(theme.selected))
            .when(!current && pose == Pose::Rest, |tab| {
                tab.hover(|style| style.bg(theme.fill_hover))
            })
            .when(current && self.ring, |tab| {
                tab.child(theme.focus_ring(theme.radius.item))
            })
            .when_some(tab.start, |row, start| {
                row.child(h_flex().flex_none().h(crate::theme::LINE_BODY).child(start))
            })
            .child(v_flex().flex_1().min_w_0().map(|body| {
                if tab.children.is_empty() {
                    body.child(
                        div()
                            .truncate()
                            .when(current, |label| label.font_weight(FontWeight::MEDIUM))
                            .child(tab.label),
                    )
                    .when_some(tab.description, |body, description| {
                        body.child(
                            div()
                                .truncate()
                                .text_small()
                                .text_color(quiet)
                                .child(description),
                        )
                    })
                } else {
                    body.children(tab.children)
                }
            }))
            .when_some(tab.end, |row, end| {
                row.child(
                    h_flex()
                        .flex_none()
                        .h(crate::theme::LINE_BODY)
                        .text_small()
                        .text_color(quiet)
                        .child(end),
                )
            })
            .when_some(tab.on_close.filter(|_| pose == Pose::Rest), |row, close| {
                row.child(
                    div()
                        .flex_none()
                        .mt(px(-1.0))
                        .when(!current, |slot| {
                            slot.invisible().group_hover(GROUP, |slot| slot.visible())
                        })
                        .child(
                            Button::icon(("close", index), Icon::X, "Close")
                                .size(Size::XSmall)
                                .on_click(move |_, window, cx| close(window, cx)),
                        ),
                )
            })
    }

    fn horizontal_tab(&self, base: Stateful<Div>, tab: Tab, current: bool) -> Stateful<Div> {
        let theme = &self.theme;
        base.flex()
            .items_center()
            .gap(px(6.0))
            .h(px(30.0))
            .text_body()
            .font_weight(FontWeight::MEDIUM)
            .text_color(if current {
                theme.text
            } else {
                theme.text_tertiary
            })
            .when(!current, |tab| {
                tab.hover(|style| style.text_color(theme.text_secondary))
            })
            .when_some(tab.start, |row, start| row.child(start))
            .child(
                div()
                    .relative()
                    .px(px(2.0))
                    .map(|label| {
                        if tab.children.is_empty() {
                            label.child(tab.label)
                        } else {
                            label.children(tab.children)
                        }
                    })
                    .when(current && self.ring, |label| {
                        label.child(theme.focus_ring(px(4.0)))
                    }),
            )
            .when_some(tab.end, |row, end| {
                row.child(
                    div()
                        .text_small()
                        .text_color(theme.text_tertiary)
                        .child(end),
                )
            })
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
    }
}

impl RenderOnce for Tabs {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let focus = nav::focus_handle((self.id.clone(), "focus"), window, cx);
        let menu = menu::state(
            &ElementId::from((self.id.clone(), "menu")),
            false,
            window,
            cx,
        );
        let render = Render {
            tab: self.tab.clone(),
            selected: self.selected,
            count: self.count,
            vertical: self.vertical,
            ring: nav::keyboard_focused(&focus, window, cx),
            focus: focus.clone(),
            menu: menu.clone(),
            on_select: self.on_select.clone(),
            theme: cx.theme().clone(),
        };
        let (selected, count) = (self.selected, self.count);
        let axis = if self.vertical {
            Axis::Vertical
        } else {
            Axis::Horizontal
        };
        let on_key = self.on_select;
        let tab = self.tab;
        let list = div()
            .id(self.id.clone())
            .role(Role::TabList)
            .aria_label(self.label)
            .track_focus(&focus.tab_stop(true))
            .on_key_down(move |event: &KeyDownEvent, window, cx| {
                let stroke = &event.keystroke;
                if stroke.key == "f10" && stroke.modifiers.shift {
                    if selected < count
                        && let Some(items) = tab(selected).menu
                    {
                        let first = menu::first_navigable(&items(window, cx), false);
                        menu::open(&menu, first, None, window, cx);
                        cx.stop_propagation();
                    }
                    return;
                }
                let Some(step) = Step::from_key(event, axis) else {
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
            .map(|list| {
                if self.vertical {
                    list.flex_col().gap(px(2.0))
                } else {
                    list.gap(px(16.0))
                        .border_b_1()
                        .border_color(render.theme.hairline)
                }
            });
        match self.on_reorder {
            Some(reorder) => {
                let flow = if self.vertical {
                    Flow::Column
                } else {
                    Flow::Row
                };
                let sortable = Sortable::new(
                    list,
                    flow,
                    0..count,
                    move |(index, pose), _, _| render.tab(index, pose),
                    move |from, to, window, cx| reorder(from, to, window, cx),
                );
                match &self.scroll {
                    Some(scroll) => sortable.scrolls(scroll).into_any_element(),
                    None => sortable.into_any_element(),
                }
            }
            None => list
                .when_some(self.scroll, |list, scroll| {
                    list.overflow_y_scroll().track_scroll(&scroll)
                })
                .children((0..count).map(|index| render.tab(index, Pose::Rest)))
                .into_any_element(),
        }
    }
}
