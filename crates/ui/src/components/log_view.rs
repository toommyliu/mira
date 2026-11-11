use gpui::{
    App, ElementId, InteractiveElement as _, IntoElement, ParentElement as _, Pixels, RenderOnce,
    Role, ScrollHandle, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
    prelude::FluentBuilder as _, px,
};

use crate::primitives::Scrollbar;
use crate::{ActiveTheme as _, nav, theme::TEXT_CAPTION};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Clone)]
pub struct LogEntry {
    pub time: SharedString,
    pub level: LogLevel,
    pub message: SharedString,
}

#[derive(IntoElement)]
pub struct LogView {
    id: ElementId,
    label: SharedString,
    entries: Vec<LogEntry>,
    height: Pixels,
}

impl LogView {
    pub fn new(
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        entries: Vec<LogEntry>,
        height: Pixels,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            entries,
            height,
        }
    }
}

impl RenderOnce for LogView {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let scroll = window
            .use_keyed_state((self.id.clone(), "scroll"), cx, |_, _| ScrollHandle::new())
            .read(cx)
            .clone();
        let seen = window.use_keyed_state((self.id.clone(), "seen"), cx, |_, _| 0usize);
        let count = self.entries.len();
        let previous = seen.update(cx, |seen, _| std::mem::replace(seen, count));
        let pinned = scroll.offset().y <= -(scroll.max_offset().y - px(4.0));
        if count > previous && (previous == 0 || pinned) {
            scroll.scroll_to_bottom();
        }
        let focus = nav::focus_handle((self.id.clone(), "focus"), window, cx);
        let ring = nav::keyboard_focused(&focus, window);
        let theme = cx.theme().clone();
        let bar = Scrollbar::new((self.id.clone(), "scrollbar"), scroll.clone());
        let lines = div()
            .id(self.id.clone())
            .role(Role::Log)
            .aria_label(self.label)
            .track_focus(&focus.tab_stop(true))
            .h(self.height)
            .overflow_y_scroll()
            .track_scroll(&scroll)
            .py(px(8.0))
            .rounded(theme.radius.panel)
            .bg(theme.surface)
            .border_1()
            .border_color(theme.hairline)
            .font_family(crate::theme::SYSTEM_MONO)
            .text_size(TEXT_CAPTION)
            .line_height(px(17.0))
            .children(self.entries.into_iter().enumerate().map(|(index, entry)| {
                let (word, color) = match entry.level {
                    LogLevel::Debug => ("debug", theme.text_tertiary),
                    LogLevel::Info => ("info", theme.text_tertiary),
                    LogLevel::Warn => ("warn", theme.warning_text),
                    LogLevel::Error => ("error", theme.danger_text),
                };
                div()
                    .id(index)
                    .flex()
                    .gap(px(12.0))
                    .px(px(12.0))
                    .when(entry.level == LogLevel::Error, |line| {
                        line.bg(theme.danger_soft)
                    })
                    .child(
                        div()
                            .flex_none()
                            .text_color(theme.text_tertiary)
                            .child(entry.time),
                    )
                    .child(div().flex_none().w(px(40.0)).text_color(color).child(word))
                    .child(
                        div()
                            .flex_1()
                            .text_color(if entry.level == LogLevel::Debug {
                                theme.text_tertiary
                            } else {
                                theme.text_secondary
                            })
                            .child(entry.message),
                    )
            }));

        div()
            .relative()
            .on_scroll(|_, _, cx| cx.stop_propagation())
            .child(lines)
            .child(bar)
            .when(ring, |log| log.child(theme.focus_ring(theme.radius.panel)))
    }
}
