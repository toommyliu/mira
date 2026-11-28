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
    lit: bool,
}

impl Kbd {
    pub fn new(keys: impl Into<SharedString>) -> Self {
        Self {
            keys: keys.into(),
            plain: false,
            lit: false,
        }
    }

    pub fn lit(mut self, lit: bool) -> Self {
        self.lit = lit;
        self
    }

    pub fn hint(keys: impl Into<SharedString>) -> Self {
        Self {
            keys: keys.into(),
            plain: true,
            lit: false,
        }
    }
}

const GLYPH_GAP: f32 = 2.0;
const MODIFIERS: [char; 4] = ['⌃', '⌥', '⇧', '⌘'];

fn glyphs(cap: &str) -> Vec<&str> {
    let mut glyphs = Vec::new();
    let mut rest = cap;
    while let Some(modifier) = rest.chars().next().filter(|c| MODIFIERS.contains(c)) {
        let (glyph, tail) = rest.split_at(modifier.len_utf8());
        glyphs.push(glyph);
        rest = tail;
    }
    if !rest.is_empty() {
        glyphs.push(rest);
    }
    glyphs
}

impl RenderOnce for Kbd {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let layout = theme.layout;
        let typography = theme.typography;
        if self.plain {
            return div()
                .flex()
                .flex_none()
                .gap(px(GLYPH_GAP))
                .text_size(typography.secondary)
                .line_height(typography.secondary_line)
                .font_weight(FontWeight::MEDIUM)
                .opacity(0.6)
                .children(glyphs(&self.keys).into_iter().map(|glyph| glyph.to_owned()));
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
                    .gap(px(GLYPH_GAP))
                    .min_w(layout.control_xs)
                    .h(layout.control_xs)
                    .when(symbol.is_none(), |cap| cap.px(layout.space(1.0)))
                    .rounded(theme.radius.tag)
                    .bg(if self.lit {
                        theme.background
                    } else {
                        theme.muted
                    })
                    .text_size(typography.secondary)
                    .line_height(typography.secondary_line)
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.muted_foreground)
                    .map(|cap| match symbol {
                        Some(symbol) => cap.child(small_icon(symbol, theme.muted_foreground)),
                        None => {
                            cap.children(glyphs(&key).into_iter().map(|glyph| match Icon::for_key(
                                glyph,
                            ) {
                                Some(symbol) => {
                                    small_icon(symbol, theme.muted_foreground).into_any_element()
                                }
                                None => glyph.to_owned().into_any_element(),
                            }))
                        }
                    })
            }))
    }
}
