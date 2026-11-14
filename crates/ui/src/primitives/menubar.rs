use std::rc::Rc;

use gpui::{
    AnyElement, App, Div, ElementId, Entity, FocusHandle, InteractiveElement as _, IntoElement,
    KeyDownEvent, MouseDownEvent, ParentElement, Pixels, Point, Refineable as _, RenderOnce, Role,
    SharedString, Stateful, StatefulInteractiveElement as _, StyleRefinement, Styled, Window, div,
};

use super::{
    Button, MenuItem, PopoverClose,
    button::PressHandler,
    menu::{self, Align, ItemsBuilder, MENU_MAX_HEIGHT, MenuState, PopupKind},
    popover::{self, Content},
};
use crate::{
    FOCUS_SCOPE, FocusNext, FocusPrevious, h_flex, motion,
    nav::{self, Step},
};

enum Panel {
    Menu(ItemsBuilder),
    Popover(Content),
}

struct Entry {
    trigger: Button,
    panel: Option<Panel>,
}

enum Slot {
    Entry(Box<Entry>),
    Element(AnyElement),
}

#[derive(IntoElement)]
pub struct Menubar {
    id: ElementId,
    label: SharedString,
    slots: Vec<Slot>,
    default_open: Option<usize>,
    on_dismiss: Option<PressHandler>,
    style: StyleRefinement,
}

impl Menubar {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            slots: Vec::new(),
            default_open: None,
            on_dismiss: None,
            style: StyleRefinement::default(),
        }
    }

    pub fn menu(
        mut self,
        trigger: Button,
        items: impl Fn(&mut Window, &mut App) -> Vec<MenuItem> + 'static,
    ) -> Self {
        self.slots.push(Slot::Entry(Box::new(Entry {
            trigger,
            panel: Some(Panel::Menu(Rc::new(items))),
        })));
        self
    }

    pub fn popover(
        mut self,
        trigger: Button,
        content: impl Fn(&PopoverClose, &mut Window, &mut App) -> AnyElement + 'static,
    ) -> Self {
        self.slots.push(Slot::Entry(Box::new(Entry {
            trigger,
            panel: Some(Panel::Popover(Rc::new(content))),
        })));
        self
    }

    pub fn button(mut self, button: Button) -> Self {
        self.slots.push(Slot::Entry(Box::new(Entry {
            trigger: button,
            panel: None,
        })));
        self
    }

    pub fn on_dismiss(
        mut self,
        f: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_dismiss = Some(Rc::new(f));
        self
    }

    pub fn default_open(mut self, entry: usize) -> Self {
        self.default_open = Some(entry);
        self
    }
}

impl ParentElement for Menubar {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.slots.extend(elements.into_iter().map(Slot::Element));
    }
}

impl Styled for Menubar {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

struct Bar {
    states: Vec<Entity<MenuState>>,
    triggers: Vec<FocusHandle>,
    enabled: Vec<bool>,
    panels: Vec<bool>,
    menus: Vec<Option<ItemsBuilder>>,
}

impl Bar {
    fn open_entry(&self, cx: &App) -> Option<usize> {
        self.states
            .iter()
            .position(|state| state.read(cx).is_open())
    }

    fn adjacent(&self, from: usize, backwards: bool, panels_only: bool) -> Option<usize> {
        let step = if backwards { Step::Prev } else { Step::Next };
        nav::step(Some(from), self.states.len(), step, true, |index| {
            self.enabled[index] && (self.panels[index] || !panels_only)
        })
        .filter(|&index| index != from)
    }

    fn over_trigger(&self, at: Point<Pixels>, cx: &App) -> bool {
        self.states
            .iter()
            .any(|state| menu::over_trigger(state, at, cx))
    }

    fn show(
        &self,
        index: usize,
        keyboard: bool,
        from_end: bool,
        window: &mut Window,
        cx: &mut App,
    ) {
        let highlight = match &self.menus[index] {
            Some(items) if keyboard => menu::first_navigable(&items(window, cx), from_end),
            _ => None,
        };
        match self.open_entry(cx) {
            Some(open) if open == index => {}
            Some(open) => menu::hand_off(
                &self.states[open],
                &self.states[index],
                highlight,
                window,
                cx,
            ),
            None => menu::open(&self.states[index], highlight, None, window, cx),
        }
    }

    fn key_down(&self, event: &KeyDownEvent, window: &mut Window, cx: &mut App) {
        let stroke = &event.keystroke;
        let backwards = match stroke.key.as_str() {
            "left" => true,
            "right" => false,
            _ => return,
        };
        if stroke.modifiers.modified() {
            return;
        }
        if let Some(open) = self.open_entry(cx) {
            if let Some(next) = self.adjacent(open, backwards, true) {
                self.show(next, true, false, window, cx);
            }
        } else if let Some(at) = self
            .triggers
            .iter()
            .position(|trigger| trigger.is_focused(window))
        {
            if let Some(next) = self.adjacent(at, backwards, false) {
                window.focus(&self.triggers[next], cx);
            }
        } else {
            return;
        }
        cx.stop_propagation();
    }
}

impl RenderOnce for Menubar {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        window.with_id(self.id.clone(), |window| self.render_bar(window, cx))
    }
}

impl Menubar {
    fn render_bar(self, window: &mut Window, cx: &mut App) -> Stateful<Div> {
        let mut bar = Bar {
            states: Vec::new(),
            triggers: Vec::new(),
            enabled: Vec::new(),
            panels: Vec::new(),
            menus: Vec::new(),
        };
        let entries = self.slots.iter().filter_map(|slot| match slot {
            Slot::Entry(entry) => Some(entry),
            Slot::Element(_) => None,
        });
        for (index, entry) in entries.enumerate() {
            bar.states.push(menu::state(
                &entry_id(index),
                self.default_open == Some(index),
                window,
                cx,
            ));
            bar.triggers
                .push(nav::focus_handle((entry_id(index), "trigger"), window, cx));
            bar.enabled.push(!entry.trigger.is_disabled());
            bar.panels.push(entry.panel.is_some());
            bar.menus.push(match &entry.panel {
                Some(Panel::Menu(items)) => Some(items.clone()),
                Some(Panel::Popover(_)) | None => None,
            });
        }
        let bar = Rc::new(bar);
        let mut index = 0;
        let mut children = Vec::with_capacity(self.slots.len());
        for slot in self.slots {
            children.push(match slot {
                Slot::Element(element) => element,
                Slot::Entry(entry) => {
                    let on_dismiss = self.on_dismiss.clone();
                    let element = render_entry(index, *entry, &bar, on_dismiss, window, cx);
                    index += 1;
                    element
                }
            });
        }
        let keys = bar.clone();
        let mut menubar = h_flex()
            .id(self.id)
            .role(Role::Toolbar)
            .aria_label(self.label)
            .key_context(FOCUS_SCOPE)
            .on_key_down(move |event, window, cx| keys.key_down(event, window, cx))
            .children(children);
        menubar.style().refine(&self.style);
        menubar
    }
}

fn entry_id(index: usize) -> ElementId {
    ("entry", index).into()
}

fn render_entry(
    index: usize,
    entry: Entry,
    bar: &Rc<Bar>,
    on_dismiss: Option<PressHandler>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let Some(panel) = entry.panel else {
        return entry
            .trigger
            .focus(bar.triggers[index].clone())
            .into_any_element();
    };
    let id = entry_id(index);
    let state = bar.states[index].clone();
    let label = entry.trigger.label().clone();
    let is_open = state.read(cx).is_open();
    let press = {
        let bar = bar.clone();
        Rc::new(
            move |_: &MouseDownEvent, window: &mut Window, cx: &mut App| {
                if bar.states[index].read(cx).is_open() {
                    menu::close(&bar.states[index], true, window, cx);
                } else {
                    bar.show(index, false, false, window, cx);
                }
                cx.stop_propagation();
            },
        )
    };
    let keys = {
        let bar = bar.clone();
        Rc::new(
            move |event: &KeyDownEvent, window: &mut Window, cx: &mut App| {
                let from_end = match event.keystroke.key.as_str() {
                    "down" | "enter" | "space" => false,
                    "up" => true,
                    _ => return,
                };
                bar.show(index, true, from_end, window, cx);
                cx.stop_propagation();
            },
        )
    };
    let hover = {
        let bar = bar.clone();
        div()
            .id((id.clone(), "hover"))
            .absolute()
            .inset_0()
            .on_mouse_move(move |_, window, cx| {
                if bar.enabled[index] && bar.open_entry(cx).is_some_and(|open| open != index) {
                    bar.show(index, false, false, window, cx);
                }
            })
    };
    let mut trigger = entry
        .trigger
        .focus(bar.triggers[index].clone())
        .trigger(is_open, press, keys)
        .attach(menu::trigger_probe(&state).into_any_element())
        .attach(hover.into_any_element());
    if let Panel::Popover(_) = panel {
        popover::focus_first_on_open(&id, &state, window, cx);
    }
    let Some(shown) = menu::presence(&state, window, cx) else {
        return trigger.into_any_element();
    };
    let on_outside: PressHandler = {
        let bar = bar.clone();
        Rc::new(move |event, window, cx| {
            if bar.over_trigger(event.position, cx) {
                return;
            }
            menu::close(&bar.states[index], false, window, cx);
            if let Some(on_dismiss) = &on_dismiss {
                on_dismiss(event, window, cx);
            }
        })
    };
    let panel = match panel {
        Panel::Menu(items) => {
            let items = items(window, cx);
            menu::attach(
                Align::Start,
                &state,
                on_outside,
                window,
                cx,
                |from, window| {
                    menu::popup(
                        &id,
                        label,
                        items,
                        PopupKind::Menu,
                        MENU_MAX_HEIGHT,
                        from,
                        &state,
                        window,
                        cx,
                    )
                },
            )
        }
        Panel::Popover(content) => {
            let close = PopoverClose::returning_focus(state.clone());
            let content = content(&close, window, cx);
            let focus = state.read(cx).focus();
            let (next, previous) = (focus.clone(), focus);
            let panel = popover::panel(&id, label, close, None, cx)
                .key_context(FOCUS_SCOPE)
                .on_action(move |_: &FocusNext, window, cx| nav::trap_tab(&next, false, window, cx))
                .on_action(move |_: &FocusPrevious, window, cx| {
                    nav::trap_tab(&previous, true, window, cx)
                })
                .child(content);
            menu::attach(Align::Start, &state, on_outside, window, cx, |from, _| {
                menu::fading(motion::arrive((id.clone(), "popover"), panel, from), shown)
            })
        }
    };
    trigger = trigger.attach(panel.into_any_element());
    trigger.into_any_element()
}
