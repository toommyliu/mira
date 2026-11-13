use std::rc::Rc;

use gpui::{
    AnyElement, App, ElementId, InteractiveElement as _, IntoElement, MouseButton, ParentElement,
    RenderOnce, SharedString, StatefulInteractiveElement as _, Styled as _, Window, anchored,
    deferred, div, point, prelude::FluentBuilder as _, px,
};

use super::menu::{
    ItemsBuilder, MenuItem, PopupKind, WINDOW_MARGIN, first_navigable, open, popup, presence, state,
};
use crate::motion::From;

#[derive(IntoElement)]
pub struct ContextMenu {
    id: ElementId,
    label: SharedString,
    items: ItemsBuilder,
    children: Vec<AnyElement>,
}

impl ContextMenu {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            items: Rc::new(|_, _| Vec::new()),
            children: Vec::new(),
        }
    }

    pub fn items(mut self, f: impl Fn(&mut Window, &mut App) -> Vec<MenuItem> + 'static) -> Self {
        self.items = Rc::new(f);
        self
    }
}

impl ParentElement for ContextMenu {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for ContextMenu {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = state(&self.id, false, window, cx);
        let position = state.read(cx).position;
        let shown = presence(&state, window, cx).is_some();
        let area = window
            .use_keyed_state((self.id.clone(), "area"), cx, |_, _| {
                Rc::new(std::cell::Cell::new(gpui::Bounds::default()))
            })
            .read(cx)
            .clone();
        let mouse_state = state.clone();
        let key_area = area.clone();
        let key_state = state.clone();
        let key_items = self.items.clone();
        div()
            .id(self.id.clone())
            .aria_keyshortcuts("Shift+F10")
            .on_mouse_down(MouseButton::Right, move |event, window, cx| {
                open(&mouse_state, None, Some(event.position), window, cx);
                cx.stop_propagation();
            })
            .capture_key_down(move |event, window, cx| {
                let stroke = &event.keystroke;
                if stroke.key == "f10" && stroke.modifiers.shift && !key_state.read(cx).open {
                    let at = key_area.get().origin + point(px(12.0), px(12.0));
                    let first = first_navigable(&key_items(window, cx), false);
                    open(&key_state, first, Some(at), window, cx);
                    cx.stop_propagation();
                }
            })
            .relative()
            .child(
                gpui::canvas(move |bounds, _, _| area.set(bounds), |_, _, _, _| {})
                    .absolute()
                    .inset_0(),
            )
            .children(self.children)
            .when_some(position.filter(|_| shown), |area, position| {
                let items = (self.items)(window, cx);
                area.child(
                    deferred(
                        anchored()
                            .position(position)
                            .snap_to_window_with_margin(WINDOW_MARGIN)
                            .child(popup(
                                &self.id,
                                self.label.clone(),
                                items,
                                PopupKind::Menu,
                                From::Above,
                                &state,
                                window,
                                cx,
                            )),
                    )
                    .with_priority(super::layer::POPUP),
                )
            })
    }
}
