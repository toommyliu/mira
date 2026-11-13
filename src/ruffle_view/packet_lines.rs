use std::rc::Rc;

use gpui::{
    AnyElement, App, ElementId, InteractiveElement as _, IntoElement, KeyDownEvent, MouseButton,
    ParentElement as _, Pixels, RenderOnce, Role, ScrollStrategy, SharedString,
    StatefulInteractiveElement as _, Styled as _, UniformListScrollHandle, Window, div, point,
    prelude::FluentBuilder as _, px, uniform_list,
};
use mira_ui::{
    ActiveTheme as _, PointerCursor as _, StyledExt as _, Theme, Tone,
    nav::{self, Axis, Step},
    primitives::{ContextMenu, Icon, MenuItem, Scrollbar, icon},
    theme::{SYSTEM_MONO, TEXT_CAPTION},
};

const ID: &str = "packet-lines";

fn key(part: impl Into<SharedString>) -> ElementId {
    ElementId::from((ElementId::from(ID), part.into()))
}
const ROW_HEIGHT: Pixels = px(20.0);
const FOLLOW_SLACK: Pixels = px(4.0);

pub(super) struct Row {
    pub(super) time: Option<SharedString>,
    pub(super) tag: &'static str,
    pub(super) tone: Tone,
    pub(super) text: SharedString,
    pub(super) actions: bool,
}

type Rows = Rc<dyn Fn(usize, &mut App) -> Row>;
type Menu = Rc<dyn Fn(usize, &mut App) -> Vec<MenuItem>>;
type Handler = Rc<dyn Fn(usize, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub(super) struct PacketLines {
    count: usize,
    dropped: u64,
    selected: Option<usize>,
    row: Rows,
    menu: Menu,
    on_select: Option<Handler>,
    on_activate: Option<Handler>,
}

impl PacketLines {
    pub(super) fn new(
        count: usize,
        row: impl Fn(usize, &mut App) -> Row + 'static,
        menu: impl Fn(usize, &mut App) -> Vec<MenuItem> + 'static,
    ) -> Self {
        Self {
            count,
            dropped: 0,
            selected: None,
            row: Rc::new(row),
            menu: Rc::new(menu),
            on_select: None,
            on_activate: None,
        }
    }

    pub(super) fn dropped(mut self, dropped: u64) -> Self {
        self.dropped = dropped;
        self
    }

    pub(super) fn selected(mut self, selected: Option<usize>) -> Self {
        self.selected = selected;
        self
    }

    pub(super) fn on_select(mut self, f: impl Fn(usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_select = Some(Rc::new(f));
        self
    }

    pub(super) fn on_activate(
        mut self,
        f: impl Fn(usize, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_activate = Some(Rc::new(f));
        self
    }
}

#[derive(Clone, Copy, Default)]
struct Shown {
    dropped: u64,
    selected: Option<u64>,
    left_at: Option<u64>,
}

impl RenderOnce for PacketLines {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let scroll = window
            .use_keyed_state(key("scroll"), cx, |_, _| UniformListScrollHandle::new())
            .read(cx)
            .clone();
        let shown = window.use_keyed_state(key("shown"), cx, |_, _| Shown {
            dropped: self.dropped,
            selected: None,
            left_at: None,
        });
        let before = *shown.read(cx);
        let selected_line = self.selected.map(|index| self.dropped + index as u64);
        let base = scroll.0.borrow().base_handle.clone();
        let offset = base.offset();
        let following = -offset.y >= base.max_offset().y - FOLLOW_SLACK;
        let total = self.dropped + self.count as u64;
        let left_at =
            (!following).then(|| before.left_at.filter(|at| *at <= total).unwrap_or(total));
        shown.update(cx, |shown, _| {
            *shown = Shown {
                dropped: self.dropped,
                selected: selected_line,
                left_at,
            }
        });
        if following && self.count > 0 {
            scroll.scroll_to_bottom();
        } else if self.dropped > before.dropped {
            let dropped = (self.dropped - before.dropped) as f32;
            base.set_offset(point(
                offset.x,
                (offset.y + ROW_HEIGHT * dropped).min(px(0.0)),
            ));
        }
        if selected_line != before.selected
            && let Some(selected) = self.selected
        {
            scroll.scroll_to_item(selected, ScrollStrategy::Nearest);
        }

        let focus = nav::focus_handle(key("focus"), window, cx);
        let ring = nav::keyboard_focused(&focus, window, cx);
        let theme = cx.theme().clone();
        let bar = Scrollbar::new(key("scrollbar"), base);
        let keys = keys(
            self.count,
            self.selected,
            self.on_select.clone(),
            self.on_activate.clone(),
            scroll.clone(),
        );
        let PacketLines {
            count,
            dropped,
            selected,
            row,
            menu,
            on_select,
            on_activate,
        } = self;
        let row_focus = focus.clone();
        let jump = left_at.map(|at| jump(&theme, total - at, scroll.clone()));
        let rows = uniform_list(key("rows"), count, move |range, _, cx| {
            let theme = cx.theme().clone();
            range
                .map(|index| {
                    let entry = row(index, cx);
                    let current = selected == Some(index);
                    let focus = row_focus.clone();
                    let picks = on_select.clone();
                    let on_select = on_select.clone();
                    let on_activate = on_activate.clone();
                    let (_, _, tag_color) = theme.tone(entry.tone);
                    let line = div()
                        .id(index)
                        .debug_selector(move || format!("line-{index}"))
                        .role(Role::ListBoxOption)
                        .aria_label(entry.text.clone())
                        .aria_selected(current)
                        .aria_position_in_set(index + 1)
                        .aria_size_of_set(count)
                        .when(current, |line| line.aria_active_descendant())
                        .flex()
                        .items_center()
                        .gap(px(12.0))
                        .h(ROW_HEIGHT)
                        .px(px(12.0))
                        .font_family(SYSTEM_MONO)
                        .text_size(TEXT_CAPTION)
                        .line_height(px(17.0))
                        .whitespace_nowrap()
                        .overflow_hidden()
                        .when(current, |line| line.bg(theme.selected))
                        .pointer_cursor(&theme)
                        .when(entry.actions, |line| {
                            line.on_mouse_down(MouseButton::Right, move |_, window, cx| {
                                if let Some(pick) = &picks {
                                    pick(index, window, cx);
                                }
                            })
                        })
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
                        .when_some(entry.time, |line, time| {
                            line.child(
                                div()
                                    .flex_none()
                                    .text_color(theme.text_tertiary)
                                    .child(time),
                            )
                        })
                        .child(
                            div()
                                .flex_none()
                                .w(px(32.0))
                                .text_color(tag_color)
                                .child(entry.tag),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_color(theme.text_secondary)
                                .child(entry.text),
                        );
                    if !entry.actions {
                        return div().child(line).into_any_element();
                    }
                    let menu = menu.clone();
                    ContextMenu::new(key((dropped + index as u64).to_string()), "Packet actions")
                        .items(move |_, cx| menu(index, cx))
                        .child(line)
                        .into_any_element()
                })
                .collect()
        })
        .track_scroll(&scroll)
        .size_full()
        .py(px(4.0))
        .pr(Scrollbar::GUTTER);
        let lines = div()
            .id(ID)
            .role(Role::Log)
            .aria_label("Packets")
            .track_focus(&focus.tab_stop(true))
            .on_key_down(keys)
            .size_full()
            .child(rows);

        div()
            .relative()
            .size_full()
            .bg(theme.surface)
            .on_scroll(|_, _, cx| cx.stop_propagation())
            .child(lines)
            .child(bar)
            .children(jump)
            .when(ring, |log| {
                log.child(
                    div()
                        .absolute()
                        .inset_0()
                        .border_2()
                        .border_color(theme.focus),
                )
            })
    }
}

fn jump(theme: &Theme, unseen: u64, scroll: UniformListScrollHandle) -> AnyElement {
    let label: SharedString = match unseen {
        0 => "Jump to latest".into(),
        1 => "1 new packet".into(),
        n => format!("{n} new packets").into(),
    };
    let button = div()
        .id(key("jump"))
        .role(Role::Button)
        .aria_label(label.clone())
        .flex()
        .items_center()
        .justify_center()
        .gap(px(6.0))
        .h(px(28.0))
        .rounded_full()
        .bg(theme.overlay)
        .border_1()
        .border_color(theme.border)
        .shadow(theme.raised())
        .text_small()
        .text_color(theme.text_secondary)
        .pointer_cursor(theme)
        .hover(|style| style.text_color(theme.text))
        .on_click(move |_, window, cx| {
            scroll.scroll_to_bottom();
            window.refresh();
            cx.stop_propagation();
        });
    div()
        .absolute()
        .bottom(px(12.0))
        .left_0()
        .right_0()
        .flex()
        .justify_center()
        .child(
            button
                .pl(px(10.0))
                .pr(px(12.0))
                .child(icon(Icon::ArrowDownToLine, theme.text_secondary))
                .child(label),
        )
        .into_any_element()
}

fn keys(
    count: usize,
    selected: Option<usize>,
    on_select: Option<Handler>,
    on_activate: Option<Handler>,
    scroll: UniformListScrollHandle,
) -> impl Fn(&KeyDownEvent, &mut Window, &mut App) + 'static {
    move |event, window, cx| {
        if event.keystroke.key == "enter" {
            if let (Some(index), Some(on_activate)) = (selected, &on_activate) {
                on_activate(index, window, cx);
                cx.stop_propagation();
            }
            return;
        }
        let Some(on_select) = &on_select else {
            return;
        };
        let Some(next) = Step::from_key(event, Axis::Vertical)
            .and_then(|step| nav::step(selected, count, step, false, |_| true))
        else {
            return;
        };
        scroll.scroll_to_item(next, ScrollStrategy::Nearest);
        on_select(next, window, cx);
        cx.stop_propagation();
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use gpui::{
        Context, Render, ScrollDelta, ScrollEvent, TestAppContext, VisualTestContext, point,
    };

    use super::*;

    struct Feed {
        dropped: u64,
        lines: Vec<String>,
        selected: Option<usize>,
    }

    struct Lines(Rc<RefCell<Feed>>);

    impl Render for Lines {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let feed = self.0.borrow();
            let lines = feed.lines.clone();
            div().w(px(300.0)).h(px(120.0)).child(
                PacketLines::new(
                    lines.len(),
                    move |index, _| Row {
                        time: None,
                        tag: "in",
                        tone: Tone::Neutral,
                        text: lines[index].clone().into(),
                        actions: false,
                    },
                    |_, _| Vec::new(),
                )
                .dropped(feed.dropped)
                .selected(feed.selected),
            )
        }
    }

    #[gpui::test]
    fn the_log_follows_new_lines_until_scrolled_up_then_keeps_its_place_as_old_lines_drop(
        cx: &mut TestAppContext,
    ) {
        cx.update(mira_ui::init);
        let feed = Rc::new(RefCell::new(Feed {
            dropped: 0,
            lines: (0..40).map(|line| format!("line {line}")).collect(),
            selected: Some(39),
        }));
        let (view, cx) = cx.add_window_view(|_, _| Lines(Rc::clone(&feed)));
        let log = |change: &dyn Fn(&mut Feed), cx: &mut VisualTestContext| {
            change(&mut feed.borrow_mut());
            view.update(cx, |_, cx| cx.notify());
            cx.run_until_parked();
        };
        let shown = |cx: &mut VisualTestContext, index: usize| {
            cx.debug_bounds(Box::leak(format!("line-{index}").into_boxed_str()))
        };
        cx.run_until_parked();
        assert!(shown(cx, 39).is_some());

        log(&|feed| feed.lines.push("line 40".into()), cx);
        assert!(shown(cx, 40).is_some());

        cx.simulate_event(ScrollEvent {
            position: point(px(100.0), px(60.0)),
            delta: ScrollDelta::Pixels(point(px(0.0), px(200.0))),
            ..Default::default()
        });
        cx.run_until_parked();
        let top = (0..=40).find(|&index| shown(cx, index).is_some()).unwrap();
        let place = shown(cx, top + 2);
        assert!(top > 3 && shown(cx, 40).is_none());

        log(
            &|feed| {
                feed.lines.drain(..3);
                feed.dropped += 3;
                feed.selected = Some(36);
                feed.lines
                    .extend((41..44).map(|line| format!("line {line}")));
            },
            cx,
        );
        assert_eq!(shown(cx, top - 1), place);
        assert!(shown(cx, 40).is_none());
    }
}
