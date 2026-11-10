use gpui::{
    App, FontWeight, IntoElement, ParentElement as _, RenderOnce, SharedString, Styled as _,
    Window, div, prelude::FluentBuilder as _, px,
};

use super::{Icon, icon::small_icon};
use crate::{
    ActiveTheme as _,
    theme::{LINE_CAPTION, TEXT_CAPTION},
};

#[derive(IntoElement)]
pub struct Kbd {
    keys: SharedString,
}

impl Kbd {
    pub fn new(keys: impl Into<SharedString>) -> Self {
        Self { keys: keys.into() }
    }
}

impl RenderOnce for Kbd {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
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
                    .min_w(px(18.0))
                    .h(px(18.0))
                    .when(symbol.is_none(), |cap| cap.px(px(4.0)))
                    .rounded(theme.radius.tag)
                    .bg(theme.fill_pressed)
                    .text_size(TEXT_CAPTION)
                    .line_height(LINE_CAPTION)
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text_secondary)
                    .map(|cap| match symbol {
                        Some(symbol) => cap.child(small_icon(symbol, theme.text_secondary)),
                        None => cap.child(key),
                    })
            }))
    }
}
