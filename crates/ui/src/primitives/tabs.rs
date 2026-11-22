use std::rc::Rc;

use gpui::{
    AnyElement, App, Div, ElementId, Entity, FontWeight, InteractiveElement as _, IntoElement,
    KeyDownEvent, ParentElement, RenderOnce, Role, ScrollHandle, SharedString, Stateful,
    StatefulInteractiveElement as _, Styled as _, Window, div, prelude::FluentBuilder as _, px,
};

use super::{
    Button, ContextMenu, DropdownMenu, Flow, HoverCard, Icon, MenuItem, Pose, Scrollbar, Sortable,
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
    detail: Option<AnyElement>,
    on_close: Option<Handler>,
    menu: Option<ItemsBuilder>,
    hover_card: Option<HoverCard>,
}

impl Tab {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            start: None,
            end: None,
            description: None,
            children: Vec::new(),
            detail: None,
            on_close: None,
            menu: None,
            hover_card: None,
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

    pub fn detail(mut self, element: impl IntoElement) -> Self {
        self.detail = Some(element.into_any_element());
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

    pub fn hover_card(mut self, card: HoverCard) -> Self {
        self.hover_card = Some(card);
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum TabLayout {
    Horizontal,
    Sidebar,
    PageSidebar,
}

#[derive(IntoElement)]
pub struct Tabs {
    id: ElementId,
    label: SharedString,
    count: usize,
    tab: TabBuilder,
    selected: usize,
    layout: TabLayout,
    line: bool,
    small: bool,
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
            layout: TabLayout::Horizontal,
            line: false,
            small: false,
            scroll: None,
            on_select: None,
            on_reorder: None,
        }
    }

    pub fn selected(mut self, selected: usize) -> Self {
        self.selected = selected;
        self
    }

    pub fn line(mut self) -> Self {
        self.line = true;
        self
    }

    pub fn small(mut self) -> Self {
        self.small = true;
        self
    }

    pub fn vertical(mut self) -> Self {
        self.layout = TabLayout::Sidebar;
        self
    }

    pub fn page_sidebar(mut self) -> Self {
        self.layout = TabLayout::PageSidebar;
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
    id: ElementId,
    tab: TabBuilder,
    selected: usize,
    count: usize,
    layout: TabLayout,
    line: bool,
    small: bool,
    ring: bool,
    focus: gpui::FocusHandle,
    menu: Entity<MenuState>,
    on_select: Option<IndexHandler>,
    theme: Theme,
}

impl Render {
    fn tab(&self, index: usize, pose: Pose, window: &mut Window, cx: &mut App) -> AnyElement {
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
        let more = menu
            .clone()
            .filter(|_| self.layout != TabLayout::Horizontal && pose == Pose::Rest)
            .map(|items| {
                let id = ElementId::from((self.id.clone(), format!("more-{index}")));
                let state = menu::state(&id, false, window, cx);
                let open = state.read(cx).open;
                let trigger = DropdownMenu::new(id)
                    .state(state)
                    .trigger(
                        Button::icon(("more", index), Icon::Ellipsis, format!("{label} actions"))
                            .size(Size::XSmall)
                            .hide_tooltip(),
                    )
                    .items(move |window, cx| items(window, cx));
                (trigger, open)
            });
        let context_menu = menu.map(|items| {
            let id = ElementId::from((self.id.clone(), format!("context-{index}")));
            let state = if current {
                self.menu.clone()
            } else {
                menu::state(&id, false, window, cx)
            };
            let open = state.read(cx).open;
            let context_menu = ContextMenu::new(id, label)
                .state(state)
                .items(move |window, cx| items(window, cx));
            (context_menu, open)
        });
        let menu_open = more.as_ref().is_some_and(|(_, open)| *open)
            || context_menu.as_ref().is_some_and(|(_, open)| *open);
        let card = tab
            .hover_card
            .take()
            .filter(|_| pose == Pose::Rest && !menu_open);
        let row = if self.layout != TabLayout::Horizontal {
            self.vertical_tab(base, tab, index, current, pose, more)
        } else {
            self.horizontal_tab(base, tab, current)
        }
        .when_some(card, |row, card| {
            card.attach(row, current && self.ring, window, cx)
        });
        match context_menu {
            Some((menu, _)) => menu.child(row).into_any_element(),
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
        more: Option<(DropdownMenu, bool)>,
    ) -> Stateful<Div> {
        let layout = self.theme.layout;
        let typography = self.theme.typography;
        let theme = &self.theme;
        let quiet = theme.muted_foreground;
        let page = self.layout == TabLayout::PageSidebar;
        let (height, padding_y) = if self.small {
            (layout.control, theme.space(1.0))
        } else {
            (layout.control_lg, theme.space(1.5))
        };
        let hover_fill = if page {
            theme.sidebar_accent.opacity(0.5)
        } else {
            theme.sidebar_accent
        };
        let line = div()
            .flex()
            .items_start()
            .gap(layout.space(2.0))
            .when_some(tab.start, |row, start| {
                row.child(h_flex().flex_none().h(typography.body_line).child(start))
            })
            .child(v_flex().flex_1().min_w_0().map(|body| {
                if tab.children.is_empty() {
                    body.child(div().truncate().child(tab.label)).when_some(
                        tab.description,
                        |body, description| {
                            body.child(
                                div()
                                    .truncate()
                                    .text_caption(typography)
                                    .text_color(quiet)
                                    .child(description),
                            )
                        },
                    )
                } else {
                    body.children(tab.children)
                }
            }))
            .when(tab.end.is_some() || more.is_some(), |row| {
                let shown = more
                    .as_ref()
                    .is_some_and(|(_, open)| *open || (current && self.ring));
                let reveals = more.is_some();
                row.child(
                    div()
                        .relative()
                        .flex_none()
                        .h(typography.body_line)
                        .min_w(layout.control_xs)
                        .flex()
                        .items_center()
                        .justify_end()
                        .when_some(tab.end, |slot, end| {
                            slot.child(
                                h_flex()
                                    .text_caption(typography)
                                    .text_color(quiet)
                                    .when(shown, |end| end.invisible())
                                    .when(reveals, |end| {
                                        end.group_hover(GROUP, |end| end.invisible())
                                    })
                                    .child(end),
                            )
                        })
                        .when_some(more, |slot, (trigger, _)| {
                            slot.child(
                                div()
                                    .absolute()
                                    .top_0()
                                    .right(px(-4.0))
                                    .when(!shown, |more| {
                                        more.invisible().group_hover(GROUP, |more| more.visible())
                                    })
                                    .child(trigger),
                            )
                        }),
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
            });
        base.flex()
            .flex_col()
            .flex_none()
            .px(layout.space(2.0))
            .min_h(height)
            .py(padding_y)
            .rounded(theme.radius.control)
            .group(GROUP)
            .text_body(typography)
            .text_color(theme.sidebar_foreground)
            .when(current, |tab| {
                let fill = if page {
                    crate::theme::perceptual_mix(
                        theme.sidebar_accent,
                        theme.sidebar_accent_foreground,
                        0.05,
                    )
                } else {
                    theme.sidebar_accent
                };
                tab.bg(fill)
                    .text_color(theme.sidebar_accent_foreground)
                    .when(!page, |row| row.font_weight(FontWeight::MEDIUM))
            })
            .when(!current && pose == Pose::Rest, |tab| {
                tab.hover(|style| style.bg(hover_fill))
            })
            .when(current && self.ring, |tab| {
                tab.child(
                    div()
                        .absolute()
                        .inset_0()
                        .rounded(theme.radius.control)
                        .border_2()
                        .border_color(theme.sidebar_ring)
                        .child(crate::nav::reveal_probe()),
                )
            })
            .child(line)
            .children(tab.detail)
    }

    fn horizontal_tab(&self, base: Stateful<Div>, tab: Tab, current: bool) -> Stateful<Div> {
        let layout = self.theme.layout;
        let typography = self.theme.typography;
        let theme = &self.theme;
        base.flex()
            .items_center()
            .gap(layout.space(1.5))
            .relative()
            .h(px(25.0))
            .px(theme.space(2.0))
            .rounded(theme.radius.control)
            .border_1()
            .border_color(gpui::transparent_black())
            .when(current && !self.line, |tab| {
                tab.bg(if theme.dark_mode {
                    theme.input.opacity(0.3)
                } else {
                    theme.background
                })
                .border_color(if theme.dark_mode {
                    theme.input
                } else {
                    gpui::transparent_black()
                })
            })
            .when(current && self.ring, |tab| {
                tab.border_color(theme.ring)
                    .child(theme.halo(
                        theme.ring.opacity(0.5),
                        theme.radius.control,
                        px(1.0),
                        px(3.0),
                    ))
                    .child(theme.halo(theme.ring, theme.radius.control, px(1.0), px(1.0)))
            })
            .text_body(typography)
            .font_weight(FontWeight::MEDIUM)
            .text_color(if current {
                theme.foreground
            } else {
                theme.muted_foreground
            })
            .when(!current, |tab| {
                tab.hover(|style| style.text_color(theme.foreground))
            })
            .when_some(tab.start, |row, start| row.child(start))
            .child(div().relative().px(layout.space(0.5)).map(|label| {
                if tab.children.is_empty() {
                    label.child(tab.label)
                } else {
                    label.children(tab.children)
                }
            }))
            .when_some(tab.end, |row, end| {
                row.child(
                    div()
                        .text_caption(typography)
                        .text_color(theme.muted_foreground)
                        .child(end),
                )
            })
            .when(current && self.line, |tab| {
                tab.child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .bottom(px(-5.0))
                        .h(px(2.0))
                        .rounded_full()
                        .bg(theme.foreground),
                )
            })
    }
}

impl RenderOnce for Tabs {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let layout = cx.theme().layout;
        let focus = nav::focus_handle((self.id.clone(), "focus"), window, cx);
        let menu = menu::state(
            &ElementId::from((self.id.clone(), "menu")),
            false,
            window,
            cx,
        );
        let render = Render {
            id: self.id.clone(),
            tab: self.tab.clone(),
            selected: self.selected,
            count: self.count,
            layout: self.layout,
            line: self.line,
            small: self.small,
            ring: nav::keyboard_focused(&focus, window, cx),
            focus: focus.clone(),
            menu: menu.clone(),
            on_select: self.on_select.clone(),
            theme: cx.theme().clone(),
        };
        let (selected, count) = (self.selected, self.count);
        let vertical = self.layout != TabLayout::Horizontal;
        let axis = if vertical {
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
                if vertical {
                    list.flex_col()
                        .gap(px(1.0))
                        .px(layout.space(2.0))
                        .py(layout.space(if self.small { 1.0 } else { 2.0 }))
                } else {
                    list.h(layout.control_lg)
                        .items_center()
                        .p(px(3.0))
                        .rounded(render.theme.radius.panel)
                        .map(|list| {
                            if self.line {
                                list.gap(render.theme.space(1.0))
                            } else {
                                list.bg(render.theme.muted)
                            }
                        })
                }
            });
        let tabs = match self.on_reorder {
            Some(reorder) => {
                let flow = if vertical { Flow::Column } else { Flow::Row };
                let sortable = Sortable::new(
                    list,
                    flow,
                    0..count,
                    move |(index, pose), window, cx| render.tab(index, pose, window, cx),
                    move |from, to, window, cx| reorder(from, to, window, cx),
                );
                match &self.scroll {
                    Some(scroll) => sortable.scrolls(scroll).into_any_element(),
                    None => sortable.into_any_element(),
                }
            }
            None => list
                .when_some(self.scroll.clone(), |list, scroll| {
                    list.overflow_y_scroll().track_scroll(&scroll)
                })
                .children((0..count).map(|index| render.tab(index, Pose::Rest, window, cx)))
                .into_any_element(),
        };
        match self.scroll {
            Some(scroll) => div()
                .relative()
                .flex()
                .flex_col()
                .min_h_0()
                .child(tabs)
                .child(Scrollbar::new((self.id, "scrollbar"), scroll).persistent())
                .into_any_element(),
            None => tabs,
        }
    }
}
