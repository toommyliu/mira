use gpui::{
    App, FontWeight, IntoElement, ParentElement as _, RenderOnce, SharedString, Styled as _,
    Window, div, px,
};

use crate::{
    ActiveTheme as _, Tone,
    theme::{TEXT_CAPTION, tabular},
};

#[derive(IntoElement)]
pub struct Badge {
    text: SharedString,
    tone: Tone,
}

impl Badge {
    pub fn new(text: impl Into<SharedString>) -> Self {
        Self {
            text: text.into(),
            tone: Tone::Neutral,
        }
    }

    pub fn tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }
}

impl RenderOnce for Badge {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let (_, soft, text) = theme.tone(self.tone);
        div()
            .flex_none()
            .px(px(6.0))
            .rounded(theme.radius.tag)
            .bg(soft)
            .text_size(TEXT_CAPTION)
            .line_height(px(18.0))
            .font_weight(FontWeight::MEDIUM)
            .font_features(tabular())
            .text_color(text)
            .child(self.text)
    }
}
