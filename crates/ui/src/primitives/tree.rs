use std::{collections::BTreeSet, rc::Rc};

use gpui::{
    App, ElementId, InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _,
    RenderOnce, Role, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
    prelude::FluentBuilder as _, px,
};

use super::{Icon, icon};
use crate::{
    ActiveTheme as _, PointerCursor as _,
    nav::{self, Axis, Step},
    theme::{ROW, TEXT_BODY, TEXT_SMALL, tabular},
};

#[derive(Clone)]
pub struct TreeNode {
    label: SharedString,
    detail: Option<SharedString>,
    children: Vec<TreeNode>,
}

impl TreeNode {
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            detail: None,
            children: Vec::new(),
        }
    }

    pub fn detail(mut self, detail: impl Into<SharedString>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn children(mut self, children: Vec<TreeNode>) -> Self {
        self.children = children;
        self
    }
}

#[derive(Default)]
struct TreeState {
    expanded: BTreeSet<Vec<usize>>,
    selected: Option<Vec<usize>>,
}

struct Visible<'a> {
    path: Vec<usize>,
    node: &'a TreeNode,
    position: usize,
    siblings: usize,
}

fn flatten<'a>(
    nodes: &'a [TreeNode],
    expanded: &BTreeSet<Vec<usize>>,
    prefix: &[usize],
    out: &mut Vec<Visible<'a>>,
) {
    for (index, node) in nodes.iter().enumerate() {
        let mut path = prefix.to_vec();
        path.push(index);
        let open = expanded.contains(&path);
        out.push(Visible {
            path: path.clone(),
            node,
            position: index + 1,
            siblings: nodes.len(),
        });
        if open {
            flatten(&node.children, expanded, &path, out);
        }
    }
}

#[derive(IntoElement)]
pub struct Tree {
    id: ElementId,
    label: SharedString,
    nodes: Vec<TreeNode>,
}

impl Tree {
    pub fn new(
        id: impl Into<ElementId>,
        label: impl Into<SharedString>,
        nodes: Vec<TreeNode>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            nodes,
        }
    }
}

impl RenderOnce for Tree {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let focus = nav::focus_handle((self.id.clone(), "focus"), window, cx);
        let state =
            window.use_keyed_state((self.id.clone(), "tree"), cx, |_, _| TreeState::default());
        let ring = nav::keyboard_focused(&focus, window);
        let theme = cx.theme().clone();
        let nodes = Rc::new(self.nodes);
        let (expanded, selected) = {
            let state = state.read(cx);
            (state.expanded.clone(), state.selected.clone())
        };
        let mut visible = Vec::new();
        flatten(&nodes, &expanded, &[], &mut visible);
        let paths: Rc<[(Vec<usize>, bool)]> = visible
            .iter()
            .map(|row| (row.path.clone(), !row.node.children.is_empty()))
            .collect();
        let current = selected
            .as_ref()
            .and_then(|path| paths.iter().position(|(p, _)| p == path));
        let key_state = state.clone();
        let key_paths = paths.clone();
        div()
            .id(self.id.clone())
            .role(Role::Tree)
            .aria_label(self.label)
            .track_focus(&focus.clone().tab_stop(true))
            .on_key_down(move |event: &KeyDownEvent, _, cx| {
                let key = event.keystroke.key.as_str();
                let at = current.map(|index| key_paths[index].clone());
                let handled = key_state.update(cx, |tree, cx| {
                    let changed = match (key, at) {
                        ("right", Some((path, true))) if !tree.expanded.contains(&path) => {
                            tree.expanded.insert(path);
                            true
                        }
                        ("right", Some((path, true))) => {
                            let mut child = path;
                            child.push(0);
                            tree.selected = Some(child);
                            true
                        }
                        ("left", Some((path, _))) if tree.expanded.contains(&path) => {
                            tree.expanded.remove(&path);
                            true
                        }
                        ("left", Some((path, _))) if path.len() > 1 => {
                            tree.selected = Some(path[..path.len() - 1].to_vec());
                            true
                        }
                        ("enter" | "space", Some((path, true))) => {
                            if !tree.expanded.remove(&path) {
                                tree.expanded.insert(path);
                            }
                            true
                        }
                        _ => match Step::from_key(event, Axis::Vertical).and_then(|step| {
                            nav::step(current, key_paths.len(), step, false, |_| true)
                        }) {
                            Some(next) => {
                                tree.selected = Some(key_paths[next].0.clone());
                                true
                            }
                            None => false,
                        },
                    };
                    if changed {
                        cx.notify();
                    }
                    changed
                });
                if handled {
                    cx.stop_propagation();
                }
            })
            .flex()
            .flex_col()
            .p(px(4.0))
            .rounded(theme.radius.panel)
            .bg(theme.surface)
            .border_1()
            .border_color(theme.hairline)
            .children(visible.iter().enumerate().map(|(index, row)| {
                let depth = row.path.len() - 1;
                let branch = !row.node.children.is_empty();
                let open = expanded.contains(&row.path);
                let lit = current == Some(index);
                let path = row.path.clone();
                let click_state = state.clone();
                let focus = focus.clone();
                div()
                    .id(SharedString::from(
                        row.path
                            .iter()
                            .map(usize::to_string)
                            .collect::<Vec<_>>()
                            .join("."),
                    ))
                    .role(Role::TreeItem)
                    .aria_label(row.node.label.clone())
                    .aria_level(depth + 1)
                    .aria_position_in_set(row.position)
                    .aria_size_of_set(row.siblings)
                    .aria_selected(lit)
                    .when(branch, |item| item.aria_expanded(open))
                    .when(lit, |item| item.aria_active_descendant())
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .h(ROW)
                    .pl(px(6.0 + depth as f32 * 16.0))
                    .pr(px(10.0))
                    .rounded(theme.radius.item)
                    .pointer_cursor(&theme)
                    .when(lit, |item| item.bg(theme.selected))
                    .relative()
                    .when(lit && ring, |item| {
                        item.child(theme.focus_ring(theme.radius.item))
                    })
                    .when(!lit, |item| item.hover(|style| style.bg(theme.fill)))
                    .on_click(move |_, window, cx| {
                        window.focus(&focus, cx);
                        click_state.update(cx, |tree, cx| {
                            if branch && !tree.expanded.remove(&path) {
                                tree.expanded.insert(path.clone());
                            }
                            tree.selected = Some(path.clone());
                            cx.notify();
                        });
                    })
                    .child(div().flex_none().w(px(12.0)).when(branch, |slot| {
                        slot.child(icon(
                            if open {
                                Icon::ChevronDown
                            } else {
                                Icon::ChevronRight
                            },
                            theme.text_tertiary,
                        ))
                    }))
                    .child(
                        div()
                            .flex_1()
                            .truncate()
                            .text_size(TEXT_BODY)
                            .text_color(theme.text)
                            .child(row.node.label.clone()),
                    )
                    .when_some(row.node.detail.clone(), |item, detail| {
                        item.child(
                            div()
                                .flex_none()
                                .text_size(TEXT_SMALL)
                                .font_features(tabular())
                                .text_color(theme.text_tertiary)
                                .child(detail),
                        )
                    })
            }))
    }
}
