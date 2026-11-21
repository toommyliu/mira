use std::rc::Rc;

use gpui::{
    App, ElementId, InteractiveElement as _, IntoElement, ParentElement as _, RenderOnce, Role,
    SharedString, StatefulInteractiveElement as _, Styled as _, Window,
    prelude::FluentBuilder as _, px,
};

use crate::{
    ActiveTheme as _, PointerCursor as _, StyledExt as _, h_flex, nav,
    primitives::{Icon, icon},
};

type ToggleHandler = Rc<dyn Fn(bool, &mut Window, &mut App)>;

#[derive(IntoElement)]
pub struct Disclosure {
    id: ElementId,
    label: SharedString,
    open: bool,
    caption: bool,
    on_toggle: Option<ToggleHandler>,
}

impl Disclosure {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>, open: bool) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            open,
            caption: false,
            on_toggle: None,
        }
    }

    pub fn caption(mut self) -> Self {
        self.caption = true;
        self
    }

    pub fn on_toggle(mut self, f: impl Fn(bool, &mut Window, &mut App) + 'static) -> Self {
        self.on_toggle = Some(Rc::new(f));
        self
    }
}

impl RenderOnce for Disclosure {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let focus = nav::focus_handle((self.id.clone(), "focus"), window, cx);
        let ring = nav::keyboard_focused(&focus, window, cx);
        let hovered = window.use_keyed_state((self.id.clone(), "hovered"), cx, |_, _| false);
        let theme = cx.theme().clone();
        let ink = if *hovered.read(cx) {
            theme.foreground
        } else {
            theme.muted_foreground
        };
        let open = self.open;
        let glyph = if open {
            Icon::ChevronDown
        } else {
            Icon::ChevronRight
        };
        let toggle = self.on_toggle;
        let chevron = if self.caption { px(12.0) } else { px(14.0) };
        let inset = theme.space(1.0);
        h_flex()
            .id(self.id)
            .role(Role::Button)
            .aria_label(self.label.clone())
            .aria_expanded(open)
            .track_focus(&focus.tab_stop(true))
            .relative()
            .flex_none()
            .gap(theme.space(1.0))
            .h(theme.layout.control_sm)
            .px(inset)
            .mx(-inset)
            .rounded(theme.radius.tag)
            .text_color(ink)
            .on_hover(move |inside, _, cx| {
                hovered.update(cx, |hovered, cx| {
                    *hovered = *inside;
                    cx.notify();
                })
            })
            .pointer_cursor(&theme)
            .when(self.caption, |row| row.text_caption(theme.typography))
            .child(self.label)
            .child(icon(glyph, ink).size(chevron))
            .when(ring, |row| {
                row.child(theme.halo(
                    theme.ring.opacity(0.5),
                    theme.radius.small,
                    px(0.0),
                    px(3.0),
                ))
            })
            .when_some(toggle, |row, toggle| {
                row.on_click(move |_, window, cx| toggle(!open, window, cx))
            })
    }
}
