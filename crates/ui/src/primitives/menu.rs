use std::{cell::Cell, rc::Rc, time::Instant};

use gpui::{
    Anchor, AnyElement, App, Bounds, Context, Div, ElementId, Entity, FocusHandle, FontWeight,
    InteractiveElement as _, IntoElement, KeyDownEvent, MouseButton, ParentElement as _, Pixels,
    Point, RenderOnce, Role, SharedString, Stateful, StatefulInteractiveElement as _, Styled as _,
    Toggled, Window, anchored, canvas, deferred, div, point, prelude::FluentBuilder as _, px,
    relative,
};

use super::{
    Icon,
    button::{FACE_GROUP, Face},
    icon,
};
use crate::{
    ActiveTheme as _, FocusNext, FocusPrevious, PointerCursor as _, Theme,
    motion::{self, From},
    nav::{self, Axis, Step},
    theme::{CONTROL_MD, LINE_BODY, PANEL_INSET, TEXT_BODY, TEXT_CAPTION, TEXT_SMALL},
};

type Handler = Rc<dyn Fn(&mut Window, &mut App)>;

const MENU_MIN_WIDTH: Pixels = px(200.0);
const MENU_MAX_HEIGHT: Pixels = px(320.0);
const ITEM_HEIGHT: Pixels = px(26.0);
const GAP: Pixels = px(4.0);
const BORDER: Pixels = px(1.0);
const SUBMENU_OVERLAP: Pixels = px(2.0);
pub(crate) const WINDOW_MARGIN: Pixels = px(8.0);
const ROW_PADDING: Pixels = px(8.0);
const CHECK_GUTTER: Pixels = px(12.0);
const TRIGGER_PADDING: Pixels = px(10.0);

#[derive(Clone)]
pub enum MenuItem {
    Action {
        label: SharedString,
        shortcut: Option<SharedString>,
        checked: Option<bool>,
        disabled: bool,
        danger: bool,
        on_select: Handler,
    },
    Submenu {
        label: SharedString,
        items: Rc<[MenuItem]>,
    },
    Section(SharedString),
    Separator,
}

impl MenuItem {
    pub fn action(
        label: impl Into<SharedString>,
        f: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        MenuItem::Action {
            label: label.into(),
            shortcut: None,
            checked: None,
            disabled: false,
            danger: false,
            on_select: Rc::new(f),
        }
    }

    pub fn submenu(label: impl Into<SharedString>, items: Vec<MenuItem>) -> Self {
        MenuItem::Submenu {
            label: label.into(),
            items: items.into(),
        }
    }

    pub fn section(label: impl Into<SharedString>) -> Self {
        MenuItem::Section(label.into())
    }

    pub fn separator() -> Self {
        MenuItem::Separator
    }

    pub fn shortcut(mut self, keys: impl Into<SharedString>) -> Self {
        if let MenuItem::Action { shortcut, .. } = &mut self {
            *shortcut = Some(keys.into());
        }
        self
    }

    pub fn checked(mut self, on: bool) -> Self {
        if let MenuItem::Action { checked, .. } = &mut self {
            *checked = Some(on);
        }
        self
    }

    pub fn disabled(mut self, off: bool) -> Self {
        if let MenuItem::Action { disabled, .. } = &mut self {
            *disabled = off;
        }
        self
    }

    pub fn danger(mut self) -> Self {
        if let MenuItem::Action { danger, .. } = &mut self {
            *danger = true;
        }
        self
    }

    fn navigable(&self) -> bool {
        match self {
            MenuItem::Action { disabled, .. } => !disabled,
            MenuItem::Submenu { .. } => true,
            MenuItem::Section(_) | MenuItem::Separator => false,
        }
    }

    fn label(&self) -> Option<&SharedString> {
        match self {
            MenuItem::Action { label, .. } | MenuItem::Submenu { label, .. } => Some(label),
            MenuItem::Section(_) | MenuItem::Separator => None,
        }
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum Align {
    #[default]
    Start,
    Center,
    End,
}

pub(crate) fn attach(
    align: Align,
    state: &Entity<MenuState>,
    window: &mut Window,
    cx: &App,
    panel: impl FnOnce(From, &mut Window) -> AnyElement,
) -> Div {
    let menu = state.read(cx);
    let trigger = menu.trigger_bounds.get();
    let height = menu.panel_height.clone();
    let fits_below =
        trigger.bottom() + GAP + height.get() <= window.viewport_size().height - WINDOW_MARGIN;
    let fits_above = trigger.top() - GAP - height.get() >= WINDOW_MARGIN;
    let above = !fits_below && fits_above;
    let spot = div()
        .absolute()
        .top(if above { -GAP } else { CONTROL_MD + GAP });
    let (spot, anchor) = match (align, above) {
        (Align::Start, false) => (spot.left_0(), Anchor::TopLeft),
        (Align::Center, false) => (spot.left(relative(0.5)), Anchor::TopCenter),
        (Align::End, false) => (spot.right_0(), Anchor::TopRight),
        (Align::Start, true) => (spot.left_0(), Anchor::BottomLeft),
        (Align::Center, true) => (spot.left(relative(0.5)), Anchor::BottomCenter),
        (Align::End, true) => (spot.right_0(), Anchor::BottomRight),
    };
    spot.child(
        deferred(
            anchored()
                .anchor(anchor)
                .snap_to_window_with_margin(WINDOW_MARGIN)
                .child(
                    div()
                        .relative()
                        .child(panel(if above { From::Below } else { From::Above }, window))
                        .child(
                            canvas(
                                move |area, window, _| {
                                    if height.replace(area.size.height) != area.size.height {
                                        window.refresh();
                                    }
                                },
                                |_, _, _, _| {},
                            )
                            .absolute()
                            .inset_0(),
                        ),
                ),
        )
        .with_priority(super::layer::POPUP),
    )
}

pub(crate) fn item_over_trigger(
    index: usize,
    count: usize,
    state: &Entity<MenuState>,
    window: &Window,
    cx: &App,
) -> Option<Point<Pixels>> {
    let height = (BORDER + PANEL_INSET) * 2.0 + ITEM_HEIGHT * count as f32;
    if height > MENU_MAX_HEIGHT {
        return None;
    }
    let offset = point(
        BORDER + PANEL_INSET + ROW_PADDING + CHECK_GUTTER + ROW_PADDING - BORDER - TRIGGER_PADDING,
        BORDER + PANEL_INSET + ITEM_HEIGHT * index as f32 - (CONTROL_MD - ITEM_HEIGHT) / 2.0,
    );
    let top = state.read(cx).trigger_bounds.get().top() - offset.y;
    let fits =
        top >= WINDOW_MARGIN && top + height <= window.viewport_size().height - WINDOW_MARGIN;
    fits.then_some(offset)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Menu,
    ListBox,
}

pub(crate) struct MenuState {
    pub(crate) open: bool,
    highlighted: Option<usize>,
    submenu: Option<(usize, Option<usize>)>,
    pub(crate) position: Option<Point<Pixels>>,
    return_focus: Option<FocusHandle>,
    focus: FocusHandle,
    submenu_bounds: Rc<Cell<Bounds<Pixels>>>,
    trigger_bounds: Rc<Cell<Bounds<Pixels>>>,
    panel_height: Rc<Cell<Pixels>>,
    pub(crate) item_offset: Cell<Option<Point<Pixels>>>,
    closed_at: Option<Instant>,
    pointer: Option<Point<Pixels>>,
}

impl MenuState {
    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            open: false,
            highlighted: None,
            submenu: None,
            position: None,
            return_focus: None,
            focus: cx.focus_handle(),
            submenu_bounds: Rc::default(),
            trigger_bounds: Rc::default(),
            panel_height: Rc::default(),
            item_offset: Cell::new(None),
            closed_at: None,
            pointer: None,
        }
    }

    fn aiming_at_submenu(&self, from: Point<Pixels>, at: Point<Pixels>) -> bool {
        let panel = self.submenu_bounds.get();
        self.submenu.is_some()
            && panel.size.height > Pixels::ZERO
            && heading_toward(from, at, (panel.origin, panel.bottom_left()))
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn focus(&self) -> FocusHandle {
        self.focus.clone()
    }
}

pub(crate) fn presence(state: &Entity<MenuState>, window: &mut Window, cx: &App) -> Option<f32> {
    let menu = state.read(cx);
    if menu.open {
        return Some(1.0);
    }
    motion::leaving(menu.closed_at?, window, cx)
}

pub(crate) fn fading(panel: impl IntoElement, presence: f32) -> AnyElement {
    if presence >= 1.0 {
        return panel.into_any_element();
    }
    div()
        .relative()
        .opacity(presence)
        .child(panel)
        .child(div().absolute().inset_0().occlude())
        .into_any_element()
}

fn heading_toward(
    from: Point<Pixels>,
    at: Point<Pixels>,
    (top, bottom): (Point<Pixels>, Point<Pixels>),
) -> bool {
    let side = |a: Point<Pixels>, b: Point<Pixels>, p: Point<Pixels>| {
        f32::from(b.x - a.x) * f32::from(p.y - a.y) - f32::from(b.y - a.y) * f32::from(p.x - a.x)
    };
    let (d1, d2, d3) = (
        side(from, top, at),
        side(top, bottom, at),
        side(bottom, from, at),
    );
    let negative = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let positive = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(negative && positive)
}

pub(crate) fn trigger_probe(state: &Entity<MenuState>) -> impl IntoElement {
    let state = state.clone();
    canvas(
        move |area, _, cx| state.read(cx).trigger_bounds.set(area),
        |_, _, _, _| {},
    )
    .absolute()
    .inset_0()
}

pub(crate) fn outside(state: &Entity<MenuState>, at: Point<Pixels>, cx: &App) -> bool {
    let menu = state.read(cx);
    !menu.trigger_bounds.get().contains(&at)
        && !(menu.submenu.is_some() && menu.submenu_bounds.get().contains(&at))
}

pub(crate) fn state(id: &ElementId, window: &mut Window, cx: &mut App) -> Entity<MenuState> {
    window.use_keyed_state((id.clone(), "menu"), cx, |_, cx| MenuState::new(cx))
}

pub(crate) fn open(
    state: &Entity<MenuState>,
    highlighted: Option<usize>,
    position: Option<Point<Pixels>>,
    window: &mut Window,
    cx: &mut App,
) {
    let return_focus = window.focused(cx);
    let focus = state.update(cx, |menu, cx| {
        menu.open = true;
        menu.closed_at = None;
        menu.pointer = None;
        menu.highlighted = highlighted;
        menu.submenu = None;
        menu.position = position;
        menu.return_focus = return_focus;
        cx.notify();
        menu.focus.clone()
    });
    window.focus(&focus, cx);
}

pub(crate) fn close(state: &Entity<MenuState>, restore: bool, window: &mut Window, cx: &mut App) {
    let return_focus = state.update(cx, |menu, cx| {
        if menu.open {
            menu.closed_at = Some(Instant::now());
        }
        menu.open = false;
        menu.submenu = None;
        cx.notify();
        menu.return_focus.take()
    });
    if restore && let Some(handle) = return_focus {
        window.focus(&handle, cx);
    }
}

fn activate(
    items: &[MenuItem],
    index: usize,
    state: &Entity<MenuState>,
    keyboard: bool,
    window: &mut Window,
    cx: &mut App,
) {
    match &items[index] {
        MenuItem::Action {
            on_select,
            disabled: false,
            ..
        } => {
            close(state, true, window, cx);
            on_select(window, cx);
        }
        MenuItem::Submenu { items: sub, .. } => {
            let first = keyboard
                .then(|| nav::step(None, sub.len(), Step::First, false, |i| sub[i].navigable()))
                .flatten();
            state.update(cx, |menu, cx| {
                menu.highlighted = Some(index);
                menu.submenu = Some((index, first));
                cx.notify();
            });
        }
        _ => {}
    }
}

fn key_down(
    items: &Rc<[MenuItem]>,
    state: &Entity<MenuState>,
    event: &KeyDownEvent,
    window: &mut Window,
    cx: &mut App,
) {
    let key = event.keystroke.key.as_str();
    let (highlighted, submenu) = current(items, state.read(cx));
    if key == "tab" {
        close(state, true, window, cx);
        return;
    }
    if let Some((parent, sub_highlight)) = submenu
        && let MenuItem::Submenu { items: sub, .. } = &items[parent]
        && sub_highlight.is_some()
    {
        if key == "escape" || key == "left" {
            state.update(cx, |menu, cx| {
                menu.submenu = None;
                cx.notify();
            });
            cx.stop_propagation();
            return;
        }
        if key == "enter" || key == "space" {
            if let Some(index) = sub_highlight {
                activate(sub, index, state, true, window, cx);
            }
            cx.stop_propagation();
            return;
        }
        let next = Step::from_key(event, Axis::Vertical)
            .and_then(|step| {
                nav::step(sub_highlight, sub.len(), step, true, |i| sub[i].navigable())
            })
            .or_else(|| {
                nav::typeahead(
                    key,
                    sub.iter()
                        .enumerate()
                        .filter(|(_, item)| item.navigable())
                        .filter_map(|(i, item)| item.label().map(|l| (i, l.to_string()))),
                    sub_highlight,
                )
            });
        if let Some(next) = next {
            state.update(cx, |menu, cx| {
                menu.submenu = Some((parent, Some(next)));
                cx.notify();
            });
        }
        cx.stop_propagation();
        return;
    }
    match key {
        "escape" => close(state, true, window, cx),
        "enter" | "space" => {
            if let Some(index) = highlighted {
                activate(items, index, state, true, window, cx);
            }
        }
        "right" => {
            if let Some(index) = highlighted
                && matches!(items[index], MenuItem::Submenu { .. })
            {
                activate(items, index, state, true, window, cx);
            }
        }
        _ => {
            let next = Step::from_key(event, Axis::Vertical)
                .and_then(|step| {
                    nav::step(highlighted, items.len(), step, true, |i| {
                        items[i].navigable()
                    })
                })
                .or_else(|| {
                    nav::typeahead(
                        key,
                        items
                            .iter()
                            .enumerate()
                            .filter(|(_, item)| item.navigable())
                            .filter_map(|(i, item)| item.label().map(|l| (i, l.to_string()))),
                        highlighted,
                    )
                });
            let Some(next) = next else {
                return;
            };
            state.update(cx, |menu, cx| {
                menu.highlighted = Some(next);
                menu.submenu = None;
                cx.notify();
            });
        }
    }
    cx.stop_propagation();
}

fn current(
    items: &[MenuItem],
    menu: &MenuState,
) -> (Option<usize>, Option<(usize, Option<usize>)>) {
    let highlighted = menu.highlighted.filter(|&index| index < items.len());
    let submenu = menu
        .submenu
        .and_then(|(parent, sub_highlight)| match items.get(parent) {
            Some(MenuItem::Submenu { items: sub, .. }) => {
                Some((parent, sub_highlight.filter(|&index| index < sub.len())))
            }
            _ => None,
        });
    (highlighted, submenu)
}

pub(crate) fn first_navigable(items: &[MenuItem], from_end: bool) -> Option<usize> {
    nav::step(
        None,
        items.len(),
        if from_end { Step::Last } else { Step::First },
        false,
        |i| items[i].navigable(),
    )
}

fn panel_shell(id: impl Into<ElementId>, scrolls: bool, theme: &Theme) -> Stateful<Div> {
    div()
        .id(id)
        .occlude()
        .flex()
        .flex_col()
        .min_w(MENU_MIN_WIDTH)
        .when(scrolls, |panel| {
            panel.max_h(MENU_MAX_HEIGHT).overflow_y_scroll()
        })
        .p(PANEL_INSET)
        .rounded(theme.radius.panel)
        .bg(theme.overlay)
        .border_1()
        .border_color(theme.border)
        .shadow(theme.floating())
        .text_size(TEXT_BODY)
        .line_height(LINE_BODY)
        .text_color(theme.text)
}

fn rows(
    items: &Rc<[MenuItem]>,
    highlighted: Option<usize>,
    submenu: Option<(usize, Option<usize>)>,
    nested: bool,
    kind: Kind,
    state: &Entity<MenuState>,
    theme: &Theme,
) -> Vec<AnyElement> {
    let gutter = items.iter().any(|item| {
        matches!(
            item,
            MenuItem::Action {
                checked: Some(_),
                ..
            }
        )
    });
    let count = items.iter().filter(|item| item.navigable()).count();
    let mut position = 0;
    items
        .iter()
        .enumerate()
        .map(|(index, item)| match item {
            MenuItem::Separator => div()
                .flex_none()
                .h(px(1.0))
                .mx(px(8.0))
                .my(PANEL_INSET)
                .bg(theme.hairline)
                .into_any_element(),
            MenuItem::Section(label) => div()
                .flex_none()
                .px(px(8.0))
                .pt(px(6.0))
                .pb(px(2.0))
                .text_size(TEXT_CAPTION)
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.text_tertiary)
                .child(label.clone())
                .into_any_element(),
            MenuItem::Action {
                label,
                shortcut,
                checked,
                disabled,
                danger,
                ..
            } => {
                position += 1;
                let lit = highlighted == Some(index);
                let fg = match (*disabled, *danger, lit) {
                    (true, _, _) => theme.text_disabled,
                    (false, true, _) => theme.danger_text,
                    (false, false, _) => theme.text,
                };
                row(index, nested, items, state)
                    .rounded(theme.radius.item)
                    .role(match (kind, checked) {
                        (Kind::ListBox, _) => Role::ListBoxOption,
                        (Kind::Menu, Some(_)) => Role::MenuItemCheckBox,
                        (Kind::Menu, None) => Role::MenuItem,
                    })
                    .aria_label(label.clone())
                    .aria_position_in_set(position)
                    .aria_size_of_set(count)
                    .when_some(shortcut.clone(), |row, keys| row.aria_keyshortcuts(keys))
                    .when_some(*checked, |row, on| match kind {
                        Kind::ListBox => row.aria_selected(on),
                        Kind::Menu => {
                            row.aria_toggled(if on { Toggled::True } else { Toggled::False })
                        }
                    })
                    .when(lit, |row| {
                        row.aria_active_descendant().bg(if *danger {
                            theme.danger_soft
                        } else {
                            theme.selected
                        })
                    })
                    .text_color(fg)
                    .when(gutter, |row| {
                        row.child(
                            div()
                                .flex_none()
                                .w(CHECK_GUTTER)
                                .when(*checked == Some(true), |c| c.child(icon(Icon::Check, fg))),
                        )
                    })
                    .child(div().flex_1().truncate().child(label.clone()))
                    .when_some(shortcut.clone(), |row, keys| {
                        row.child(
                            div()
                                .flex_none()
                                .text_size(TEXT_SMALL)
                                .text_color(theme.text_tertiary)
                                .child(keys),
                        )
                    })
                    .into_any_element()
            }
            MenuItem::Submenu { label, items: sub } => {
                position += 1;
                let lit = highlighted == Some(index);
                let open = submenu.is_some_and(|(parent, _)| parent == index);
                row(index, nested, items, state)
                    .rounded(theme.radius.item)
                    .role(Role::MenuItem)
                    .aria_label(label.clone())
                    .aria_expanded(open)
                    .aria_position_in_set(position)
                    .aria_size_of_set(count)
                    .when(lit && submenu.and_then(|(_, h)| h).is_none(), |row| {
                        row.aria_active_descendant()
                    })
                    .when(lit || open, |row| row.bg(theme.selected))
                    .relative()
                    .when(gutter, |row| row.child(div().flex_none().w(CHECK_GUTTER)))
                    .child(div().flex_1().child(label.clone()))
                    .child(icon(Icon::ChevronRight, theme.text_tertiary))
                    .when(open, |row| {
                        let sub_highlight = submenu.and_then(|(_, h)| h);
                        row.child(
                            div()
                                .absolute()
                                .top(-PANEL_INSET - BORDER)
                                .right(-(PANEL_INSET + BORDER - SUBMENU_OVERLAP))
                                .child(
                                    anchored().snap_to_window_with_margin(px(8.0)).child(
                                        motion::fade(
                                            ("submenu", index),
                                            panel_shell(("submenu-panel", index), true, theme)
                                                .role(Role::Menu)
                                                .aria_label(label.clone())
                                                .child({
                                                    let state = state.clone();
                                                    canvas(
                                                        move |area, _, cx| {
                                                            state.read(cx).submenu_bounds.set(area)
                                                        },
                                                        |_, _, _, _| {},
                                                    )
                                                    .absolute()
                                                    .inset_0()
                                                })
                                                .children(rows(
                                                    sub,
                                                    sub_highlight,
                                                    None,
                                                    true,
                                                    Kind::Menu,
                                                    state,
                                                    theme,
                                                )),
                                        ),
                                    ),
                                ),
                        )
                    })
                    .into_any_element()
            }
        })
        .collect()
}

fn row(
    index: usize,
    nested: bool,
    items: &Rc<[MenuItem]>,
    state: &Entity<MenuState>,
) -> Stateful<Div> {
    let hover_state = state.clone();
    let click_state = state.clone();
    let click_items = items.clone();
    div()
        .id(("item", index))
        .flex()
        .flex_none()
        .items_center()
        .gap(ROW_PADDING)
        .h(ITEM_HEIGHT)
        .px(ROW_PADDING)
        .when(nested, |row| {
            row.on_hover(move |hovered, _, cx| {
                if !*hovered {
                    return;
                }
                hover_state.update(cx, |menu, cx| {
                    if let Some((parent, _)) = menu.submenu {
                        menu.submenu = Some((parent, Some(index)));
                        cx.notify();
                    }
                });
            })
        })
        .on_click(move |_, window, cx| {
            activate(&click_items, index, &click_state, false, window, cx);
        })
        .when(!nested, |row| {
            let open_state = state.clone();
            let open_items = items.clone();
            row.on_mouse_move(move |event, _, cx| {
                let opens = matches!(open_items[index], MenuItem::Submenu { .. });
                open_state.update(cx, |menu, cx| {
                    let at = event.position;
                    let from = menu.pointer.replace(at);
                    let current = menu.submenu.map(|(parent, _)| parent);
                    if current == Some(index) {
                        return;
                    }
                    if current.is_some()
                        && from.is_some_and(|from| menu.aiming_at_submenu(from, at))
                    {
                        return;
                    }
                    if menu.highlighted == Some(index) && current.is_none() && !opens {
                        return;
                    }
                    menu.highlighted = Some(index);
                    menu.submenu = opens.then_some((index, None));
                    cx.notify();
                });
            })
        })
}

pub(crate) fn popup(
    id: &ElementId,
    label: SharedString,
    items: Rc<[MenuItem]>,
    kind: Kind,
    from: From,
    state: &Entity<MenuState>,
    window: &mut Window,
    cx: &App,
) -> AnyElement {
    let shown = presence(state, window, cx).unwrap_or(0.0);
    let theme = &cx.theme().clone();
    let menu = state.read(cx);
    let (highlighted, submenu) = current(&items, menu);
    let focus = menu.focus.clone();
    let keys_state = state.clone();
    let keys_items = items.clone();
    let out_state = state.clone();
    let next_state = state.clone();
    let prev_state = state.clone();
    let panel = motion::arrive(
        (id.clone(), "popup"),
        panel_shell(
            (id.clone(), "panel"),
            !items
                .iter()
                .any(|item| matches!(item, MenuItem::Submenu { .. })),
            theme,
        )
        .role(match kind {
            Kind::Menu => Role::Menu,
            Kind::ListBox => Role::ListBox,
        })
        .aria_label(label)
        .track_focus(&focus)
        .on_key_down(move |event, window, cx| key_down(&keys_items, &keys_state, event, window, cx))
        .on_action(move |_: &FocusNext, window, cx| {
            close(&next_state, true, window, cx);
            cx.propagate();
        })
        .on_action(move |_: &FocusPrevious, window, cx| {
            close(&prev_state, true, window, cx);
            cx.propagate();
        })
        .on_mouse_down_out(move |event, window, cx| {
            if outside(&out_state, event.position, cx) {
                close(&out_state, false, window, cx)
            }
        })
        .children(rows(
            &items,
            highlighted,
            submenu,
            false,
            kind,
            state,
            theme,
        )),
        from,
    );
    fading(panel, shown)
}

pub(crate) fn trigger_keys(
    items: Rc<[MenuItem]>,
    state: Entity<MenuState>,
    initial: Option<usize>,
) -> impl Fn(&KeyDownEvent, &mut Window, &mut App) {
    move |event, window, cx| {
        let highlight = match event.keystroke.key.as_str() {
            "down" | "enter" | "space" => initial.or_else(|| first_navigable(&items, false)),
            "up" => initial.or_else(|| first_navigable(&items, true)),
            _ => return,
        };
        open(&state, highlight, None, window, cx);
        cx.stop_propagation();
    }
}

pub(crate) fn trigger_face(open: bool, theme: &Theme) -> Face {
    Face {
        radius: theme.radius.control,
        bg: if open { theme.fill_pressed } else { theme.fill },
        border: Some(theme.border),
        states: Some((theme.fill_hover, theme.fill_pressed)),
        edge: theme.edge(),
    }
}

pub(crate) fn trigger_style(
    base: Stateful<Div>,
    face: Stateful<Div>,
    theme: &Theme,
    ring: bool,
) -> Stateful<Div> {
    base.flex()
        .flex_none()
        .items_center()
        .gap(px(6.0))
        .h(CONTROL_MD)
        .pl(TRIGGER_PADDING)
        .pr(px(8.0))
        .relative()
        .group(FACE_GROUP)
        .child(face)
        .text_size(TEXT_BODY)
        .line_height(LINE_BODY)
        .text_color(theme.text)
        .pointer_cursor(&theme)
        .when(ring, |trigger| {
            trigger.child(theme.focus_ring(theme.radius.control))
        })
}

#[derive(IntoElement)]
pub struct Menu {
    id: ElementId,
    label: SharedString,
    items: Vec<MenuItem>,
    align: Align,
}

impl Menu {
    pub fn new(
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        items: Vec<MenuItem>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            items,
            align: Align::Start,
        }
    }

    pub fn align(mut self, align: Align) -> Self {
        self.align = align;
        self
    }
}

impl RenderOnce for Menu {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = state(&self.id, window, cx);
        let trigger_focus = nav::focus_handle((self.id.clone(), "trigger"), window, cx);
        let ring = nav::keyboard_focused(&trigger_focus, window);
        let is_open = state.read(cx).open;
        let shown = presence(&state, window, cx).is_some();
        let theme = cx.theme().clone();
        let items: Rc<[MenuItem]> = self.items.into();
        let click_state = state.clone();
        div()
            .relative()
            .flex_none()
            .child(
                trigger_style(
                    div()
                        .id(self.id.clone())
                        .role(Role::Button)
                        .aria_label(self.label.clone())
                        .aria_expanded(is_open)
                        .track_focus(&trigger_focus.tab_stop(true)),
                    trigger_face(is_open, &theme).render((self.id.clone(), "face"), window, cx),
                    &theme,
                    ring,
                )
                .on_key_down(trigger_keys(items.clone(), state.clone(), None))
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    if click_state.read(cx).open {
                        close(&click_state, true, window, cx);
                    } else {
                        open(&click_state, None, None, window, cx);
                    }
                    cx.stop_propagation();
                })
                .child(self.label.clone())
                .child(icon(Icon::ChevronDown, theme.text_tertiary))
                .child(trigger_probe(&state)),
            )
            .when(shown, |container| {
                let panel = |from, window: &mut Window| {
                    popup(
                        &self.id,
                        self.label.clone(),
                        items,
                        Kind::Menu,
                        from,
                        &state,
                        window,
                        cx,
                    )
                };
                container.child(attach(self.align, &state, window, cx, panel))
            })
    }
}
