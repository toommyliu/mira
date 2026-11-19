use gpui::{
    App, FontWeight, IntoElement, ParentElement as _, RenderOnce, SharedString, Styled as _,
    Window, div, prelude::FluentBuilder as _, px,
};

use super::{Icon, icon::small_icon};
use crate::ActiveTheme as _;

#[derive(IntoElement)]
pub struct Kbd {
    keys: SharedString,
    plain: bool,
}

impl Kbd {
    pub fn new(keys: impl Into<SharedString>) -> Self {
        Self {
            keys: keys.into(),
            plain: false,
        }
    }

    pub fn hint(keys: impl Into<SharedString>) -> Self {
        Self {
            keys: keys.into(),
            plain: true,
        }
    }
}

impl RenderOnce for Kbd {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let layout = theme.layout;
        let typography = theme.typography;
        if self.plain {
            return div()
                .flex_none()
                .text_size(typography.secondary)
                .line_height(typography.secondary_line)
                .font_weight(FontWeight::MEDIUM)
                .opacity(0.6)
                .child(self.keys);
        }
        let caps: Vec<String> = self.keys.split_whitespace().map(str::to_owned).collect();
        div()
            .flex()
            .flex_none()
            .gap(px(3.0))
            .children(caps.into_iter().map(|key| {
                let symbol = Icon::for_key(&key);
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .min_w(layout.control_xs)
                    .h(layout.control_xs)
                    .when(symbol.is_none(), |cap| cap.px(layout.space(1.0)))
                    .rounded(theme.radius.xs)
                    .bg(theme.muted)
                    .text_size(typography.secondary)
                    .line_height(typography.secondary_line)
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.muted_foreground)
                    .map(|cap| match symbol {
                        Some(symbol) => cap.child(small_icon(symbol, theme.muted_foreground)),
                        None => cap.child(key),
                    })
            }))
    }
}
