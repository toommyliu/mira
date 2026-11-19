use gpui::{
    App, FontWeight, IntoElement, ParentElement as _, RenderOnce, SharedString, Styled as _, Window,
};

use crate::{ActiveTheme as _, Tone, theme::tabular};

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
        let layout = theme.layout;
        let typography = theme.typography;
        let (_, soft, text) = theme.tone(self.tone);
        crate::h_flex()
            .flex_none()
            .h(layout.control_xs)
            .px(theme.space(2.0))
            .gap(theme.space(1.0))
            .rounded_full()
            .border_1()
            .border_color(if matches!(self.tone, Tone::Accent | Tone::Warning) {
                theme.border
            } else {
                gpui::transparent_black()
            })
            .bg(soft)
            .text_size(typography.secondary)
            .line_height(typography.secondary_line)
            .font_weight(FontWeight::MEDIUM)
            .font_features(tabular())
            .text_color(text)
            .child(self.text)
    }
}
