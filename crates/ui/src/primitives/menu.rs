use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
    time::Instant,
};

use gpui::{
    Anchor, AnyElement, App, Bounds, Context, DispatchPhase, Div, ElementId, Entity, FocusHandle,
    FontWeight, HitboxBehavior, InteractiveElement as _, IntoElement, KeyDownEvent, MouseButton,
    MouseDownEvent, ParentElement, Pixels, Point, RenderOnce, Role, ScrollHandle, SharedString,
    Stateful, StatefulInteractiveElement as _, Styled as _, Toggled, Window, anchored, canvas,
    deferred, div, point, prelude::FluentBuilder as _, px,
};

use super::{
    Button, Icon, Scrollbar,
    button::{KeyHandler, PressHandler},
    small_icon,
};
use crate::{
    ActiveTheme as _, FocusNext, FocusPrevious, StyledExt as _, Theme, h_flex,
    motion::{self, From},
    nav::{self, Axis, Step},
    theme::{LINE_BODY, PANEL_INSET, TEXT_BODY},
    v_flex,
};

pub(crate) type Handler = Rc<dyn Fn(&mut Window, &mut App)>;
pub(crate) type ItemsBuilder = Rc<dyn Fn(&mut Window, &mut App) -> Vec<MenuItem>>;

pub(crate) const MENU_MAX_HEIGHT: Pixels = px(360.0);
const ITEM_HEIGHT: Pixels = px(24.0);
const GAP: Pixels = px(4.0);
const BORDER: Pixels = px(1.0);
const SUBMENU_OVERLAP: Pixels = px(2.0);
pub(crate) const WINDOW_MARGIN: Pixels = px(8.0);
const ROW_PADDING: Pixels = px(8.0);
const MARK_GUTTER: Pixels = px(12.0);
const ROW_GAP: Pixels = px(6.0);
const TRIGGER_PADDING: Pixels = px(10.0);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mark {
    Check,
    Radio,
    Switch,
}

enum Kind {
    Action(Handler),
    Submenu(Vec<MenuItem>),
    Header,
    Separator,
}

pub struct MenuItem {
    kind: Kind,
    label: SharedString,
    start: Option<AnyElement>,
    end: Option<AnyElement>,
    description: Option<SharedString>,
    children: Vec<AnyElement>,
    shortcut: Option<SharedString>,
    key: Option<SharedString>,
    checked: Option<bool>,
    mark: Mark,
    disabled: bool,
    danger: bool,
    keep_open: bool,
    pinned: bool,
    dismiss: Option<(SharedString, Handler)>,
}

impl MenuItem {
    fn with(kind: Kind, label: SharedString) -> Self {
        Self {
            kind,
            label,
            start: None,
            end: None,
            description: None,
            children: Vec::new(),
            shortcut: None,
            key: None,
            checked: None,
            mark: Mark::Check,
            disabled: false,
            danger: false,
            keep_open: false,
            pinned: false,
            dismiss: None,
        }
    }

    pub fn action(
        label: impl Into<SharedString>,
        f: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        Self::with(Kind::Action(Rc::new(f)), label.into())
    }

    pub fn switch(
        label: impl Into<SharedString>,
        on: bool,
        f: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        let mut item = Self::action(label, f).checked(on).keep_open();
        item.mark = Mark::Switch;
        item
    }

    pub fn radio(
        label: impl Into<SharedString>,
        checked: bool,
        f: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        let mut item = Self::action(label, f).checked(checked);
        item.mark = Mark::Radio;
        item
    }

    pub fn submenu(label: impl Into<SharedString>, items: Vec<MenuItem>) -> Self {
        Self::with(Kind::Submenu(items), label.into())
    }

    pub fn header(label: impl Into<SharedString>) -> Self {
        Self::with(Kind::Header, label.into())
    }

    pub fn separator() -> Self {
        Self::with(Kind::Separator, SharedString::default())
    }

    pub fn start_slot(mut self, element: impl IntoElement) -> Self {
        self.start = Some(element.into_any_element());
        self
    }

    pub fn end_slot(mut self, element: impl IntoElement) -> Self {
        self.end = Some(element.into_any_element());
        self
    }

    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn shortcut(mut self, keys: impl Into<SharedString>) -> Self {
        self.shortcut = Some(keys.into());
        self
    }

    pub fn key(mut self, key: impl Into<SharedString>) -> Self {
        self.key = Some(key.into());
        self
    }

    pub fn checked(mut self, on: bool) -> Self {
        self.checked = Some(on);
        self
    }

    pub fn disabled(mut self, off: bool) -> Self {
        self.disabled = off;
        self
    }

    pub fn danger(mut self) -> Self {
        self.danger = true;
        self
    }

    pub fn keep_open(mut self) -> Self {
        self.keep_open = true;
        self
    }

    pub fn pinned(mut self) -> Self {
        self.pinned = true;
        self
    }

    pub fn dismiss(
        mut self,
        label: impl Into<SharedString>,
        f: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.dismiss = Some((label.into(), Rc::new(f)));
        self
    }

    fn navigable(&self) -> bool {
        match self.kind {
            Kind::Action(_) | Kind::Submenu(_) => !self.disabled,
            Kind::Header | Kind::Separator => false,
        }
    }
}

impl ParentElement for MenuItem {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

pub(crate) struct Meta {
    label: SharedString,
    key: Option<SharedString>,
    navigable: bool,
    on_select: Option<Handler>,
    dismiss: Option<Handler>,
    keep_open: bool,
    submenu: Option<Rc<[Meta]>>,
}

fn metas(items: &[MenuItem]) -> Rc<[Meta]> {
    items
        .iter()
        .map(|item| Meta {
            label: item.label.clone(),
            key: item.key.clone(),
            navigable: item.navigable(),
            on_select: match &item.kind {
                Kind::Action(handler) if !item.disabled => Some(handler.clone()),
                _ => None,
            },
            dismiss: item.dismiss.as_ref().map(|(_, handler)| handler.clone()),
            keep_open: item.keep_open,
            submenu: match &item.kind {
                Kind::Submenu(items) => Some(metas(items)),
                _ => None,
            },
        })
        .collect()
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
    on_outside: PressHandler,
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
    let spot = div().absolute().top(if above {
        -GAP
    } else {
        trigger.size.height + GAP
    });
    let (spot, anchor) = match (align, above) {
        (Align::Start, false) => (spot.left_0(), Anchor::TopLeft),
        (Align::Center, false) => (spot.left(gpui::relative(0.5)), Anchor::TopCenter),
        (Align::End, false) => (spot.right_0(), Anchor::TopRight),
        (Align::Start, true) => (spot.left_0(), Anchor::BottomLeft),
        (Align::Center, true) => (spot.left(gpui::relative(0.5)), Anchor::BottomCenter),
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
                        .child(dismiss_probe(state, on_outside))
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
    let trigger = state.read(cx).trigger_bounds.get();
    let offset = point(
        BORDER + PANEL_INSET + ROW_PADDING + MARK_GUTTER + ROW_GAP - TRIGGER_PADDING,
        BORDER + PANEL_INSET + ITEM_HEIGHT * index as f32
            - (trigger.size.height - ITEM_HEIGHT) / 2.0,
    );
    let top = trigger.top() - offset.y;
    let fits =
        top >= WINDOW_MARGIN && top + height <= window.viewport_size().height - WINDOW_MARGIN;
    fits.then_some(offset)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PopupKind {
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
    submenu_rows: RefCell<HashMap<usize, Bounds<Pixels>>>,
    rest_scroll: ScrollHandle,
    trigger_bounds: Rc<Cell<Bounds<Pixels>>>,
    panel_height: Rc<Cell<Pixels>>,
    pub(crate) item_offset: Cell<Option<Point<Pixels>>>,
    closed_at: Option<Instant>,
    pointer: Option<Point<Pixels>>,
    started: bool,
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
            submenu_rows: RefCell::default(),
            rest_scroll: ScrollHandle::new(),
            trigger_bounds: Rc::default(),
            panel_height: Rc::default(),
            item_offset: Cell::new(None),
            closed_at: None,
            pointer: None,
            started: false,
        }
    }

    fn aiming_at_submenu(&self, from: Point<Pixels>, at: Point<Pixels>) -> bool {
        let panel = self.submenu_bounds.get();
        let edge = if panel.left() >= from.x {
            (panel.origin, panel.bottom_left())
        } else {
            (panel.top_right(), panel.bottom_right())
        };
        self.submenu.is_some() && panel.size.height > Pixels::ZERO && heading_toward(from, at, edge)
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn focus(&self) -> FocusHandle {
        self.focus.clone()
    }

    pub(crate) fn trigger_width(&self) -> Pixels {
        self.trigger_bounds.get().size.width
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

pub(crate) fn over_trigger(state: &Entity<MenuState>, at: Point<Pixels>, cx: &App) -> bool {
    state.read(cx).trigger_bounds.get().contains(&at)
}

pub(crate) fn dismiss(state: &Entity<MenuState>) -> PressHandler {
    let state = state.clone();
    Rc::new(move |event, window, cx| {
        if !over_trigger(&state, event.position, cx) {
            close(&state, false, window, cx)
        }
    })
}

pub(crate) fn dismiss_probe(
    state: &Entity<MenuState>,
    on_outside: PressHandler,
) -> impl IntoElement + use<> {
    let state = state.clone();
    canvas(
        |_, window, _| {
            window.insert_hitbox(
                Bounds::new(Point::default(), window.viewport_size()),
                HitboxBehavior::Normal,
            )
        },
        move |_, probe, window, _| {
            window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                if phase == DispatchPhase::Capture
                    && state.read(cx).open
                    && probe.is_hovered_at(event.position, window)
                {
                    on_outside(event, window, cx);
                }
            });
        },
    )
    .absolute()
}

pub(crate) fn state(
    id: &ElementId,
    default_open: bool,
    window: &mut Window,
    cx: &mut App,
) -> Entity<MenuState> {
    let state = window.use_keyed_state((id.clone(), "menu"), cx, |_, cx| MenuState::new(cx));
    if default_open && !state.read(cx).started {
        state.update(cx, |menu, _| {
            menu.started = true;
            menu.open = true;
        });
    }
    state
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
        menu.started = true;
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

pub(crate) fn hand_off(
    from: &Entity<MenuState>,
    to: &Entity<MenuState>,
    highlighted: Option<usize>,
    window: &mut Window,
    cx: &mut App,
) {
    let return_focus = from.update(cx, |menu, cx| {
        menu.open = false;
        menu.submenu = None;
        menu.closed_at = None;
        cx.notify();
        menu.return_focus.take()
    });
    open(to, highlighted, None, window, cx);
    to.update(cx, |menu, _| menu.return_focus = return_focus);
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
    metas: &Rc<[Meta]>,
    index: usize,
    state: &Entity<MenuState>,
    keyboard: bool,
    window: &mut Window,
    cx: &mut App,
) {
    let meta = &metas[index];
    if let Some(sub) = &meta.submenu {
        let first = keyboard
            .then(|| nav::step(None, sub.len(), Step::First, false, |i| sub[i].navigable))
            .flatten();
        state.update(cx, |menu, cx| {
            menu.highlighted = Some(index);
            menu.submenu = Some((index, first));
            cx.notify();
        });
        return;
    }
    let Some(on_select) = meta.on_select.clone() else {
        return;
    };
    if !meta.keep_open {
        close(state, true, window, cx);
    }
    on_select(window, cx);
}

fn typeahead(key: &str, metas: &[Meta], current: Option<usize>) -> Option<usize> {
    nav::typeahead(
        key,
        metas
            .iter()
            .enumerate()
            .filter(|(_, meta)| meta.navigable)
            .map(|(i, meta)| (i, meta.label.to_string())),
        current,
    )
}

fn keyed(event: &KeyDownEvent, metas: &[Meta]) -> Option<usize> {
    if event.keystroke.modifiers.modified() {
        return None;
    }
    let key = event.keystroke.key.as_str();
    metas
        .iter()
        .position(|meta| meta.navigable && meta.key.as_deref() == Some(key))
}

fn key_down(
    metas: &Rc<[Meta]>,
    kind: PopupKind,
    state: &Entity<MenuState>,
    event: &KeyDownEvent,
    window: &mut Window,
    cx: &mut App,
) {
    let key = event.keystroke.key.as_str();
    let (highlighted, submenu) = current(metas, state.read(cx));
    if key == "tab" {
        close(state, true, window, cx);
        return;
    }
    if let Some((parent, sub_highlight)) = submenu
        && let Some(sub) = &metas[parent].submenu
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
        if let Some(index) = keyed(event, sub) {
            activate(sub, index, state, true, window, cx);
            cx.stop_propagation();
            return;
        }
        let next = Step::from_key(event, Axis::Vertical)
            .and_then(|step| nav::step(sub_highlight, sub.len(), step, true, |i| sub[i].navigable))
            .or_else(|| typeahead(key, sub, sub_highlight));
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
        "backspace" | "delete" => {
            if let Some(dismiss) = highlighted.and_then(|index| metas[index].dismiss.clone()) {
                dismiss(window, cx);
            }
        }
        "enter" | "space" => {
            if let Some(index) = highlighted {
                activate(metas, index, state, true, window, cx);
            }
        }
        "left" | "right" => {
            let submenu =
                highlighted.filter(|&index| key == "right" && metas[index].submenu.is_some());
            match (submenu, kind) {
                (Some(index), _) => activate(metas, index, state, true, window, cx),
                (None, PopupKind::ListBox) => {}
                (None, PopupKind::Menu) => return,
            }
        }
        _ if let Some(index) = keyed(event, metas) => {
            activate(metas, index, state, true, window, cx);
        }
        _ => {
            let next = Step::from_key(event, Axis::Vertical)
                .and_then(|step| {
                    nav::step(highlighted, metas.len(), step, true, |i| metas[i].navigable)
                })
                .or_else(|| typeahead(key, metas, highlighted));
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

fn current(metas: &[Meta], menu: &MenuState) -> (Option<usize>, Option<(usize, Option<usize>)>) {
    let highlighted = menu.highlighted.filter(|&index| index < metas.len());
    let submenu = menu
        .submenu
        .and_then(|(parent, sub_highlight)| match metas.get(parent) {
            Some(Meta {
                submenu: Some(sub), ..
            }) => Some((parent, sub_highlight.filter(|&index| index < sub.len()))),
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

pub(crate) fn panel_shell(
    id: impl Into<ElementId>,
    max_height: Option<Pixels>,
    theme: &Theme,
) -> Stateful<Div> {
    div()
        .id(id)
        .occlude()
        .flex()
        .flex_col()
        .when_some(max_height, |panel, height| {
            panel.max_h(height).overflow_y_scroll()
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

fn switch_mark(on: bool, theme: &Theme) -> Div {
    div()
        .flex_none()
        .w(px(24.0))
        .h(px(14.0))
        .p(px(2.0))
        .rounded_full()
        .bg(if on {
            theme.accent
        } else {
            theme.border_strong
        })
        .child(
            div()
                .when(on, |thumb| thumb.ml(px(10.0)))
                .size(px(10.0))
                .rounded_full()
                .bg(theme.knob),
        )
}

type Cursor = (Option<usize>, Option<(usize, Option<usize>)>);

struct OpenSubmenu {
    index: usize,
    highlighted: Option<usize>,
    row: Bounds<Pixels>,
}

fn rows(
    items: Vec<MenuItem>,
    metas: &Rc<[Meta]>,
    ((highlighted, submenu), submenu_row): (Cursor, Option<Bounds<Pixels>>),
    nested: bool,
    kind: PopupKind,
    state: &Entity<MenuState>,
    theme: &Theme,
) -> Vec<AnyElement> {
    let gutter = items
        .iter()
        .any(|item| item.checked.is_some() && item.mark != Mark::Switch);
    let count = metas.iter().filter(|meta| meta.navigable).count();
    let mut position = 0;
    items
        .into_iter()
        .enumerate()
        .map(|(index, item)| {
            let MenuItem {
                kind: item_kind,
                label,
                start,
                end,
                description,
                children,
                shortcut,
                key,
                checked,
                mark,
                disabled,
                danger,
                dismiss,
                ..
            } = item;
            match item_kind {
                Kind::Separator => div()
                    .flex_none()
                    .h(px(1.0))
                    .mx(px(-4.0))
                    .my(PANEL_INSET)
                    .bg(theme.hairline)
                    .into_any_element(),
                Kind::Header => div()
                    .flex_none()
                    .px(ROW_PADDING)
                    .pt(px(6.0))
                    .pb(px(2.0))
                    .text_caption()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.text_tertiary)
                    .child(label)
                    .into_any_element(),
                Kind::Action(_) | Kind::Submenu(_) => {
                    position += 1;
                    let lit = highlighted == Some(index);
                    let open = submenu.is_some_and(|(parent, _)| parent == index);
                    let fg = match (disabled, danger) {
                        (true, _) => theme.text_disabled,
                        (false, true) => theme.danger_text,
                        (false, false) => theme.text,
                    };
                    let two_line = description.is_some();
                    let sub = match item_kind {
                        Kind::Submenu(sub) => Some(sub),
                        _ => None,
                    };
                    let role = match (kind, &sub, checked, mark) {
                        (PopupKind::ListBox, _, _, _) => Role::ListBoxOption,
                        (_, Some(_), _, _) => Role::MenuItem,
                        (_, None, Some(_), Mark::Radio) => Role::MenuItemRadio,
                        (_, None, Some(_), _) => Role::MenuItemCheckBox,
                        (_, None, None, _) => Role::MenuItem,
                    };
                    let body = v_flex()
                        .flex_1()
                        .min_w_0()
                        .map(|body| {
                            if children.is_empty() {
                                body.child(div().truncate().child(label.clone()))
                            } else {
                                body.children(children)
                            }
                        })
                        .when_some(description.clone(), |body, description| {
                            body.child(
                                div()
                                    .truncate()
                                    .text_small()
                                    .text_color(if lit {
                                        theme.text_secondary
                                    } else {
                                        theme.text_tertiary
                                    })
                                    .child(description),
                            )
                        });
                    let trailing = |content: AnyElement| {
                        h_flex()
                            .flex_none()
                            .gap(px(4.0))
                            .text_small()
                            .text_color(if lit {
                                theme.text_secondary
                            } else {
                                theme.text_tertiary
                            })
                            .child(content)
                    };
                    row(index, nested, metas, state)
                        .rounded(theme.radius.item)
                        .role(role)
                        .aria_label(label.clone())
                        .when_some(description, |row, description| {
                            row.aria_description(description)
                        })
                        .aria_position_in_set(position)
                        .aria_size_of_set(count)
                        .when_some(shortcut.clone().or(key), |row, keys| {
                            row.aria_keyshortcuts(keys)
                        })
                        .when_some(checked, |row, on| match kind {
                            PopupKind::ListBox => row.aria_selected(on),
                            PopupKind::Menu => {
                                row.aria_toggled(if on { Toggled::True } else { Toggled::False })
                            }
                        })
                        .when(sub.is_some(), |row| row.aria_expanded(open))
                        .when(
                            lit && !(open && submenu.and_then(|(_, h)| h).is_some()),
                            |row| row.aria_active_descendant(),
                        )
                        .when(lit || open, |row| {
                            row.bg(if danger {
                                theme.danger_soft
                            } else {
                                theme.selected
                            })
                        })
                        .when(!two_line, |row| row.h(ITEM_HEIGHT))
                        .when(two_line, |row| row.py(px(4.0)))
                        .text_color(fg)
                        .when(gutter, |row| {
                            row.child(
                                div().flex_none().w(MARK_GUTTER).when(
                                    checked == Some(true) && mark != Mark::Switch,
                                    |gutter| gutter.child(Icon::Check),
                                ),
                            )
                        })
                        .when_some(start, |row, start| {
                            row.child(h_flex().flex_none().child(start))
                        })
                        .child(body)
                        .when_some(end, |row, end| row.child(trailing(end)))
                        .when_some(shortcut, |row, keys| {
                            row.child(trailing(keys.into_any_element()))
                        })
                        .when(mark == Mark::Switch, |row| {
                            row.child(switch_mark(checked == Some(true), theme))
                        })
                        .when_some(dismiss, |row, (label, dismiss)| {
                            row.child(
                                div()
                                    .id(("dismiss", index))
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .size(px(18.0))
                                    .rounded(theme.radius.item)
                                    .when(lit, |button| {
                                        button
                                            .role(Role::Button)
                                            .aria_label(label)
                                            .text_color(theme.text_secondary)
                                            .hover(|button| button.bg(theme.fill_hover))
                                            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                                cx.stop_propagation()
                                            })
                                            .on_click(move |_, window, cx| {
                                                cx.stop_propagation();
                                                dismiss(window, cx);
                                            })
                                            .child(small_icon(Icon::X, theme.text_secondary))
                                    }),
                            )
                        })
                        .when_some(sub, |row, sub| {
                            let sub_metas = metas[index].submenu.clone().unwrap_or_default();
                            let measured = state.clone();
                            row.relative()
                                .child(
                                    canvas(
                                        move |area, window, cx| {
                                            let previous = measured
                                                .read(cx)
                                                .submenu_rows
                                                .borrow_mut()
                                                .insert(index, area);
                                            if previous != Some(area) {
                                                window.refresh();
                                            }
                                        },
                                        |_, _, _, _| {},
                                    )
                                    .absolute()
                                    .inset_0(),
                                )
                                .child(trailing(Icon::ChevronRight.into_any_element()))
                                .when_some(submenu_row.filter(|_| open), |row, target| {
                                    let sub_highlight = submenu.and_then(|(_, h)| h);
                                    row.child(submenu_panel(
                                        OpenSubmenu {
                                            index,
                                            highlighted: sub_highlight,
                                            row: target,
                                        },
                                        &label,
                                        sub,
                                        &sub_metas,
                                        state,
                                        theme,
                                    ))
                                })
                        })
                        .into_any_element()
                }
            }
        })
        .collect()
}

fn submenu_panel(
    OpenSubmenu {
        index,
        highlighted,
        row,
    }: OpenSubmenu,
    label: &SharedString,
    items: Vec<MenuItem>,
    metas: &Rc<[Meta]>,
    state: &Entity<MenuState>,
    theme: &Theme,
) -> impl IntoElement {
    deferred(
        anchored()
            .attach(row, Anchor::TopRight)
            .offset(point(
                PANEL_INSET + BORDER - SUBMENU_OVERLAP,
                -(PANEL_INSET + BORDER),
            ))
            .snap_to_window_with_margin(WINDOW_MARGIN)
            .child(motion::fade(
                ("submenu", index),
                panel_shell(("submenu-panel", index), Some(MENU_MAX_HEIGHT), theme)
                    .role(Role::Menu)
                    .aria_label(label.clone())
                    .child({
                        let state = state.clone();
                        canvas(
                            move |area, _, cx| state.read(cx).submenu_bounds.set(area),
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .inset_0()
                    })
                    .children(rows(
                        items,
                        metas,
                        ((highlighted, None), None),
                        true,
                        PopupKind::Menu,
                        state,
                        theme,
                    )),
            )),
    )
    .with_priority(super::layer::POPUP)
}

fn row(index: usize, nested: bool, metas: &Rc<[Meta]>, state: &Entity<MenuState>) -> Stateful<Div> {
    let hover_state = state.clone();
    let click_state = state.clone();
    let click_metas = metas.clone();
    h_flex()
        .id(("item", index))
        .flex_none()
        .gap(ROW_GAP)
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
            activate(&click_metas, index, &click_state, false, window, cx);
        })
        .when(!nested, |row| {
            let open_state = state.clone();
            let opens = metas[index].submenu.is_some();
            let navigable = metas[index].navigable;
            row.on_mouse_move(move |event, _, cx| {
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
                    let highlight = navigable.then_some(index);
                    if menu.highlighted == highlight && current.is_none() && !opens {
                        return;
                    }
                    menu.highlighted = highlight;
                    menu.submenu = (opens && navigable).then_some((index, None));
                    cx.notify();
                });
            })
        })
}

pub(crate) fn popup(
    id: &ElementId,
    label: SharedString,
    items: Vec<MenuItem>,
    kind: PopupKind,
    max_height: Pixels,
    from: From,
    state: &Entity<MenuState>,
    window: &mut Window,
    cx: &App,
) -> AnyElement {
    let shown = presence(state, window, cx).unwrap_or(0.0);
    let theme = &cx.theme().clone();
    let metas = metas(&items);
    let menu = state.read(cx);
    let (highlighted, submenu) = current(&metas, menu);
    let submenu_row =
        submenu.and_then(|(parent, _)| menu.submenu_rows.borrow().get(&parent).copied());
    let focus = menu.focus.clone();
    let keys_state = state.clone();
    let keys_metas = metas.clone();
    let next_state = state.clone();
    let prev_state = state.clone();
    let scrolls = metas.iter().all(|meta| meta.submenu.is_none());
    let rest_scroll = menu.rest_scroll.clone();
    let count = items.iter().take_while(|item| item.pinned).count();
    let mut pinned = rows(
        items,
        &metas,
        ((highlighted, submenu), submenu_row),
        false,
        kind,
        state,
        theme,
    );
    let rest = pinned.split_off(count);
    let split = scrolls && count > 0;
    let panel = motion::arrive(
        (id.clone(), "popup"),
        panel_shell(
            (id.clone(), "panel"),
            (scrolls && !split).then_some(max_height),
            theme,
        )
        .role(match kind {
            PopupKind::Menu => Role::Menu,
            PopupKind::ListBox => Role::ListBox,
        })
        .aria_label(label)
        .track_focus(&focus)
        .on_key_down(move |event, window, cx| {
            key_down(&keys_metas, kind, &keys_state, event, window, cx)
        })
        .on_action(move |_: &FocusNext, window, cx| {
            close(&next_state, true, window, cx);
            cx.propagate();
        })
        .on_action(move |_: &FocusPrevious, window, cx| {
            close(&prev_state, true, window, cx);
            cx.propagate();
        })
        .children(pinned)
        .map(|panel| {
            if split {
                panel.child(scrolling(
                    ElementId::from((id.clone(), "rest")),
                    max_height,
                    false,
                    rest,
                    &rest_scroll,
                ))
            } else {
                panel.children(rest)
            }
        }),
        from,
    );
    fading(panel, shown)
}

fn scrolling(
    id: ElementId,
    height: Pixels,
    fixed: bool,
    rows: Vec<AnyElement>,
    handle: &ScrollHandle,
) -> Div {
    let overflows = handle.max_offset().y > Pixels::ZERO;
    let measured = handle.clone();
    div()
        .relative()
        .child(
            div()
                .id(id.clone())
                .flex()
                .flex_col()
                .max_h(height)
                .when(fixed, |rest| rest.h(height))
                .when(overflows, |rest| rest.pr(Scrollbar::GUTTER))
                .overflow_y_scroll()
                .track_scroll(handle)
                .children(rows),
        )
        .child(
            canvas(
                move |_, window, _| {
                    if (measured.max_offset().y > Pixels::ZERO) != overflows {
                        window.refresh();
                    }
                },
                |_, _, _, _| {},
            )
            .absolute()
            .size_0(),
        )
        .child(Scrollbar::new((id, "scrollbar"), handle.clone()))
}

#[derive(IntoElement)]
pub struct MenuList {
    id: ElementId,
    label: SharedString,
    items: Vec<MenuItem>,
    height: Pixels,
}

impl MenuList {
    pub fn new(
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        items: Vec<MenuItem>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            items,
            height: MENU_MAX_HEIGHT,
        }
    }

    pub fn height(mut self, height: Pixels) -> Self {
        self.height = height;
        self
    }
}

impl RenderOnce for MenuList {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = state(&self.id, false, window, cx);
        let theme = cx.theme().clone();
        let metas = metas(&self.items);
        let (highlighted, _) = current(&metas, state.read(cx));
        let focus = state.read(cx).focus.clone();
        let rest_scroll = state.read(cx).rest_scroll.clone();
        let keys_state = state.clone();
        let keys_metas = metas.clone();
        let leave_state = state.clone();
        let count = self.items.iter().take_while(|item| item.pinned).count();
        let mut pinned = rows(
            self.items,
            &metas,
            ((highlighted, None), None),
            false,
            PopupKind::Menu,
            &state,
            &theme,
        );
        let rest = pinned.split_off(count);
        div()
            .id(self.id.clone())
            .role(Role::Menu)
            .aria_label(self.label)
            .track_focus(&focus.tab_stop(true))
            .on_key_down(move |event, window, cx| {
                key_down(&keys_metas, PopupKind::Menu, &keys_state, event, window, cx)
            })
            .on_hover(move |hovered, _, cx| {
                if !hovered {
                    leave_state.update(cx, |menu, cx| {
                        menu.highlighted = None;
                        cx.notify();
                    });
                }
            })
            .flex()
            .flex_col()
            .p(PANEL_INSET)
            .rounded(theme.radius.panel)
            .bg(theme.surface)
            .border_1()
            .border_color(theme.hairline)
            .text_size(TEXT_BODY)
            .line_height(LINE_BODY)
            .text_color(theme.text)
            .children(pinned)
            .child(scrolling(
                ElementId::from((self.id, "rest")),
                self.height,
                true,
                rest,
                &rest_scroll,
            ))
    }
}

pub(crate) fn trigger_handlers(
    state: &Entity<MenuState>,
    items: Option<ItemsBuilder>,
    initial: Option<usize>,
) -> (PressHandler, KeyHandler) {
    let press_state = state.clone();
    let key_state = state.clone();
    let press = Rc::new(
        move |_: &gpui::MouseDownEvent, window: &mut Window, cx: &mut App| {
            if press_state.read(cx).open {
                close(&press_state, true, window, cx);
            } else {
                open(&press_state, initial, None, window, cx);
            }
            cx.stop_propagation();
        },
    );
    let keys = Rc::new(
        move |event: &KeyDownEvent, window: &mut Window, cx: &mut App| {
            let from_end = match event.keystroke.key.as_str() {
                "down" | "enter" | "space" => false,
                "up" => true,
                _ => return,
            };
            let highlight = initial.or_else(|| {
                items
                    .as_ref()
                    .and_then(|items| first_navigable(&items(window, cx), from_end))
            });
            open(&key_state, highlight, None, window, cx);
            cx.stop_propagation();
        },
    );
    (press, keys)
}

#[derive(IntoElement)]
pub struct DropdownMenu {
    id: ElementId,
    trigger: Option<Button>,
    items: ItemsBuilder,
    align: Align,
    width: Option<Pixels>,
    max_height: Pixels,
    default_open: bool,
}

impl DropdownMenu {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            trigger: None,
            items: Rc::new(|_, _| Vec::new()),
            align: Align::Start,
            width: None,
            max_height: MENU_MAX_HEIGHT,
            default_open: false,
        }
    }

    pub fn trigger(mut self, trigger: Button) -> Self {
        self.trigger = Some(trigger);
        self
    }

    pub fn items(mut self, f: impl Fn(&mut Window, &mut App) -> Vec<MenuItem> + 'static) -> Self {
        self.items = Rc::new(f);
        self
    }

    pub fn align(mut self, align: Align) -> Self {
        self.align = align;
        self
    }

    pub fn width(mut self, width: Pixels) -> Self {
        self.width = Some(width);
        self
    }

    pub fn max_height(mut self, height: Pixels) -> Self {
        self.max_height = height;
        self
    }

    pub fn default_open(mut self, open: bool) -> Self {
        self.default_open = open;
        self
    }
}

impl RenderOnce for DropdownMenu {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = state(&self.id, self.default_open, window, cx);
        let is_open = state.read(cx).open;
        let shown = presence(&state, window, cx).is_some();
        let trigger = self
            .trigger
            .unwrap_or_else(|| Button::new((self.id.clone(), "trigger"), "Menu"));
        let label = trigger.label().clone();
        let (press, keys) = trigger_handlers(&state, Some(self.items.clone()), None);
        let mut trigger = trigger
            .trigger(is_open, press, keys)
            .attach(trigger_probe(&state).into_any_element());
        if shown {
            let items = (self.items)(window, cx);
            let id = self.id.clone();
            let (width, max_height) = (self.width, self.max_height);
            let on_outside = dismiss(&state);
            trigger = trigger.attach(
                attach(
                    self.align,
                    &state,
                    on_outside,
                    window,
                    cx,
                    |from, window| {
                        let panel = popup(
                            &id,
                            label,
                            items,
                            PopupKind::Menu,
                            max_height,
                            from,
                            &state,
                            window,
                            cx,
                        );
                        match width {
                            Some(width) => div().w(width).child(panel).into_any_element(),
                            None => panel,
                        }
                    },
                )
                .into_any_element(),
            );
        }
        trigger
    }
}
