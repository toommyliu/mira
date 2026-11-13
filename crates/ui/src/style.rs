use gpui::{Div, FontWeight, Styled, div};

use crate::theme::{
    LINE_BODY, LINE_CAPTION, LINE_DISPLAY, LINE_SMALL, LINE_TITLE, TEXT_BODY, TEXT_CAPTION,
    TEXT_DISPLAY, TEXT_SMALL, TEXT_TITLE,
};

pub fn h_flex() -> Div {
    div().flex().flex_row().items_center()
}

pub fn v_flex() -> Div {
    div().flex().flex_col()
}

pub trait StyledExt: Styled + Sized {
    fn text_caption(self) -> Self {
        self.text_size(TEXT_CAPTION).line_height(LINE_CAPTION)
    }

    fn text_small(self) -> Self {
        self.text_size(TEXT_SMALL).line_height(LINE_SMALL)
    }

    fn text_body(self) -> Self {
        self.text_size(TEXT_BODY).line_height(LINE_BODY)
    }

    fn text_title(self) -> Self {
        self.text_size(TEXT_TITLE)
            .line_height(LINE_TITLE)
            .font_weight(FontWeight::SEMIBOLD)
    }

    fn text_display(self) -> Self {
        self.text_size(TEXT_DISPLAY)
            .line_height(LINE_DISPLAY)
            .font_weight(FontWeight::SEMIBOLD)
    }
}

impl<T: Styled> StyledExt for T {}
