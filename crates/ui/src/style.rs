use crate::Theme;
use gpui::{Div, FontWeight, IntoElement, ParentElement as _, Pixels, Styled, div, px};

pub fn h_flex() -> Div {
    div().flex().flex_row().items_center()
}

pub fn v_flex() -> Div {
    div().flex().flex_col()
}

pub trait StyledExt: Styled + Sized {
    fn window_surface(self, theme: &Theme) -> Self {
        self.window_text(theme).bg(theme.background)
    }

    fn window_text(self, theme: &Theme) -> Self {
        self.font_family(theme.font_family.clone())
            .text_body(theme.typography)
            .text_color(theme.foreground)
    }

    fn text_caption(self, typography: crate::theme::Typography) -> Self {
        self.text_size(typography.secondary)
            .line_height(typography.secondary_line)
    }

    fn text_body(self, typography: crate::theme::Typography) -> Self {
        self.text_size(typography.body)
            .line_height(typography.body_line)
    }

    fn text_title(self, typography: crate::theme::Typography) -> Self {
        self.text_size(typography.body)
            .line_height(typography.body_line)
            .font_weight(FontWeight::MEDIUM)
    }

    fn text_heading(self, typography: crate::theme::Typography) -> Self {
        self.text_size(typography.heading)
            .line_height(typography.heading_line)
            .font_weight(FontWeight::MEDIUM)
    }
}

impl<T: Styled> StyledExt for T {}

impl Theme {
    pub fn toolbar(&self) -> Div {
        h_flex()
            .flex_none()
            .min_h(self.layout.toolbar)
            .px(self.space(3.0))
            .py(self.space(1.0))
            .gap(self.space(2.0))
            .border_b_1()
            .border_color(self.border)
    }

    pub fn footer(&self) -> Div {
        h_flex()
            .flex_none()
            .min_h(self.layout.toolbar)
            .px(self.space(4.0))
            .py(self.space(1.5))
            .gap(self.space(2.0))
            .border_t_1()
            .border_color(self.border)
    }

    pub fn floating_bar(&self) -> Div {
        h_flex()
            .flex_none()
            .relative()
            .gap(px(2.0))
            .pl(self.space(3.0))
            .pr(self.space(1.0))
            .py(self.space(1.0))
            .rounded(self.radius.panel + px(2.0))
            .bg(self.popover)
            .text_color(self.popover_foreground)
            .shadow(self.large_shadow())
            .child(self.surface_ring(self.radius.panel + px(2.0)))
    }

    pub fn sidebar_pane(&self) -> Div {
        v_flex()
            .flex_none()
            .w(self.layout.sidebar)
            .min_h_0()
            .bg(self.sidebar)
            .text_color(self.sidebar_foreground)
            .border_r_1()
            .border_color(self.sidebar_border)
    }

    pub fn inspector_pane(&self) -> Div {
        v_flex()
            .flex_none()
            .min_h_0()
            .w(self.layout.inspector)
            .bg(self.background)
            .border_l_1()
            .border_color(self.border)
    }

    pub fn form_columns(
        &self,
        width: Pixels,
        first: impl IntoElement,
        second: impl IntoElement,
    ) -> Div {
        let inset = self.space(4.0);
        if width >= self.layout.split_forms {
            h_flex()
                .items_start()
                .min_h_full()
                .child(div().flex_1().min_w_0().p(inset).child(first))
                .child(div().self_stretch().w(px(1.0)).bg(self.border))
                .child(div().flex_1().min_w_0().p(inset).child(second))
        } else {
            v_flex()
                .p(inset)
                .gap(inset)
                .child(first)
                .child(div().h(px(1.0)).bg(self.border))
                .child(second)
        }
    }

    pub fn fill_columns(&self, first: impl IntoElement, second: impl IntoElement) -> Div {
        h_flex()
            .flex_1()
            .min_h_0()
            .items_stretch()
            .child(v_flex().flex_1().min_w_0().child(first))
            .child(div().w(px(1.0)).bg(self.border))
            .child(v_flex().flex_1().min_w_0().child(second))
    }

    pub fn account_columns(&self, pane: Pixels) -> usize {
        let gap = self.space(2.0);
        let inner = pane - self.space(3.0) * 2.0;
        (f32::from(inner + gap) / f32::from(self.layout.card_min + gap))
            .floor()
            .clamp(1.0, 6.0) as usize
    }
}
