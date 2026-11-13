use std::rc::Rc;

use gpui::{
    App, ElementId, InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _, Pixels,
    RenderOnce, Role, ScrollStrategy, SharedString, StatefulInteractiveElement as _, Styled as _,
    UniformListScrollHandle, Window, div, point, prelude::FluentBuilder as _, px, uniform_list,
};

use crate::{
    ActiveTheme as _, IndexHandler, PointerCursor as _, Tone,
    nav::{self, Axis, Step},
    primitives::{ContextMenu, MenuItem, Scrollbar},
    theme::TEXT_CAPTION,
};

const ROW_HEIGHT: Pixels = px(20.0);
const FOLLOW_SLACK: Pixels = px(4.0);

#[derive(Clone)]
pub struct LogEntry {
    pub time: Option<SharedString>,
    pub tag: SharedString,
    pub tone: Tone,
    pub message: SharedString,
}

impl LogEntry {
    pub fn new(tag: impl Into<SharedString>, message: impl Into<SharedString>) -> Self {
        Self {
            time: None,
            tag: tag.into(),
            tone: Tone::Neutral,
            message: message.into(),
        }
    }

    pub fn time(mut self, time: impl Into<SharedString>) -> Self {
        self.time = Some(time.into());
        self
    }

    pub fn tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }
}

type RowSource = Rc<dyn Fn(usize) -> LogEntry>;
type MenuSource = Rc<dyn Fn(usize) -> Vec<MenuItem>>;

#[derive(IntoElement)]
pub struct LogView {
    id: ElementId,
    label: SharedString,
    count: usize,
    dropped: u64,
    row: RowSource,
    height: Option<Pixels>,
    selected: Option<usize>,
    on_select: Option<IndexHandler>,
    on_activate: Option<IndexHandler>,
    menu: Option<MenuSource>,
}

impl LogView {
    pub fn new(
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        count: usize,
        row: impl Fn(usize) -> LogEntry + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            count,
            dropped: 0,
            row: Rc::new(row),
            height: None,
            selected: None,
            on_select: None,
            on_activate: None,
            menu: None,
        }
    }

    pub fn height(mut self, height: Pixels) -> Self {
        self.height = Some(height);
        self
    }

    pub fn dropped(mut self, dropped: u64) -> Self {
        self.dropped = dropped;
        self
    }

    pub fn selected(mut self, selected: Option<usize>) -> Self {
        self.selected = selected;
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

    pub fn menu(mut self, items: impl Fn(usize) -> Vec<MenuItem> + 'static) -> Self {
        self.menu = Some(Rc::new(items));
        self
    }
}

#[derive(Clone, Copy, Default)]
struct Shown {
    dropped: u64,
    selected: Option<usize>,
}

impl RenderOnce for LogView {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let scroll = window
            .use_keyed_state((self.id.clone(), "scroll"), cx, |_, _| {
                UniformListScrollHandle::new()
            })
            .read(cx)
            .clone();
        let shown = window.use_keyed_state((self.id.clone(), "shown"), cx, |_, _| Shown {
            dropped: self.dropped,
            selected: None,
        });
        let before = *shown.read(cx);
        shown.update(cx, |shown, _| {
            *shown = Shown {
                dropped: self.dropped,
                selected: self.selected,
            }
        });
        let base = scroll.0.borrow().base_handle.clone();
        let offset = base.offset();
        let following = -offset.y >= base.max_offset().y - FOLLOW_SLACK;
        if following && self.count > 0 {
            scroll.scroll_to_bottom();
        } else if self.dropped > before.dropped {
            let dropped = (self.dropped - before.dropped) as f32;
            base.set_offset(point(
                offset.x,
                (offset.y + ROW_HEIGHT * dropped).min(px(0.0)),
            ));
        }
        if self.selected != before.selected
            && let Some(selected) = self.selected
        {
            scroll.scroll_to_item(selected, ScrollStrategy::Nearest);
        }

        let focus = nav::focus_handle((self.id.clone(), "focus"), window, cx);
        let ring = nav::keyboard_focused(&focus, window, cx);
        let theme = cx.theme().clone();
        let bar = Scrollbar::new((self.id.clone(), "scrollbar"), base);
        let keys = keys(
            self.count,
            self.selected,
            self.on_select.clone(),
            self.on_activate.clone(),
            scroll.clone(),
        );
        let row_source = self.row;
        let menu = self.menu;
        let selected = self.selected;
        let on_select = self.on_select;
        let on_activate = self.on_activate;
        let dropped = self.dropped;
        let count = self.count;
        let row_focus = focus.clone();
        let id = self.id.clone();
        let rows = uniform_list(
            (self.id.clone(), "rows"),
            self.count,
            move |range, _, cx| {
                let theme = cx.theme().clone();
                range
                    .map(|index| {
                        let entry = row_source(index);
                        let current = selected == Some(index);
                        let focus = row_focus.clone();
                        let on_select = on_select.clone();
                        let on_activate = on_activate.clone();
                        let (_, _, tag_color) = theme.tone(entry.tone);
                        let line = div()
                            .id(index)
                            .debug_selector(move || format!("line-{index}"))
                            .role(Role::ListBoxOption)
                            .aria_label(entry.message.clone())
                            .aria_selected(current)
                            .aria_position_in_set(index + 1)
                            .aria_size_of_set(count)
                            .when(current, |line| line.aria_active_descendant())
                            .flex()
                            .items_center()
                            .gap(px(12.0))
                            .h(ROW_HEIGHT)
                            .px(px(12.0))
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .when(entry.tone == Tone::Danger, |line| {
                                line.bg(theme.danger_soft)
                            })
                            .when(current, |line| line.bg(theme.selected))
                            .when(on_select.is_some() || on_activate.is_some(), |line| {
                                line.pointer_cursor(&theme)
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
                                    .w(px(48.0))
                                    .text_color(tag_color)
                                    .child(entry.tag),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_color(theme.text_secondary)
                                    .child(entry.message),
                            );
                        match &menu {
                            Some(menu) => ContextMenu::new(
                                (
                                    id.clone(),
                                    SharedString::from((dropped + index as u64).to_string()),
                                ),
                                "Line actions",
                                menu(index),
                                line,
                            )
                            .into_any_element(),
                            None => line.into_any_element(),
                        }
                    })
                    .collect()
            },
        )
        .track_scroll(&scroll)
        .size_full()
        .py(px(8.0))
        .pr(Scrollbar::GUTTER);
        let lines = div()
            .id(self.id)
            .role(Role::Log)
            .aria_label(self.label)
            .track_focus(&focus.tab_stop(true))
            .on_key_down(keys)
            .map(|lines| match self.height {
                Some(height) => lines.h(height),
                None => lines.size_full(),
            })
            .rounded(theme.radius.panel)
            .bg(theme.surface)
            .border_1()
            .border_color(theme.hairline)
            .font_family(crate::theme::SYSTEM_MONO)
            .text_size(TEXT_CAPTION)
            .line_height(px(17.0))
            .child(rows);

        div()
            .relative()
            .when(self.height.is_none(), |log| log.size_full())
            .on_scroll(|_, _, cx| cx.stop_propagation())
            .child(lines)
            .child(bar)
            .when(ring, |log| log.child(theme.focus_ring(theme.radius.panel)))
    }
}

fn keys(
    count: usize,
    selected: Option<usize>,
    on_select: Option<IndexHandler>,
    on_activate: Option<IndexHandler>,
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
