use std::{io, path::PathBuf};

use gpui::{
    AnyView, App, Context, IntoElement, ParentElement as _, Pixels, Render, Size, Styled as _,
    Window, div,
};
use mira_ui::ActiveTheme as _;

pub(crate) struct Story {
    pub(crate) name: &'static str,
    pub(crate) build: fn(&mut Window, &mut App) -> AnyView,
}

pub(crate) struct Board {
    pub(crate) name: &'static str,
    #[cfg_attr(not(feature = "storyboard"), allow(dead_code))]
    pub(crate) size: Size<Pixels>,
    pub(crate) min_size: Size<Pixels>,
    pub(crate) stories: Vec<Story>,
}

fn boards() -> Vec<Board> {
    Vec::new()
}

pub(crate) fn scratch(story: &str) -> PathBuf {
    let folder = std::env::temp_dir()
        .join(format!("mira-storyboard-{}", std::process::id()))
        .join(slug(story));
    if let Err(error) = std::fs::remove_dir_all(&folder)
        && error.kind() != io::ErrorKind::NotFound
    {
        panic!("couldn't clear {}: {error}", folder.display());
    }
    std::fs::create_dir_all(&folder).expect("create the story's folder");
    folder
}

fn slug(name: &str) -> String {
    name.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

fn globals(cx: &mut App) {
    let theme = cx.theme().clone();
    let folder = scratch("globals");
    crate::settings::init(folder.join("settings.json"), |_| {}, cx);
    crate::script_inputs::init(folder.join("script-inputs.json"), cx);
    crate::packages::init(
        folder.join("packages"),
        folder.join("skua"),
        folder.join("github-token"),
        cx,
    );
    cx.set_global(theme);
}

struct Stage(AnyView);

impl Render for Stage {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .child(self.0.clone())
            .child(crate::quit::Overlay)
    }
}

#[cfg(feature = "storyboard")]
pub(crate) fn run() -> anyhow::Result<()> {
    mira_platform::application()
        .with_assets(mira_ui::Assets)
        .run(|cx| {
            mira_ui::init(cx);
            globals(cx);
            crate::ruffle_view::bind_keys(cx);
            crate::quit::init(cx);
            match live::open(cx) {
                Ok(index) => {
                    cx.on_window_closed(move |cx, closed| {
                        if closed == index {
                            cx.quit();
                        }
                    })
                    .detach();
                    cx.activate(true);
                }
                Err(error) => {
                    eprintln!("couldn't open the storyboard: {error:#}");
                    cx.quit();
                }
            }
        });
    Ok(())
}

#[cfg(feature = "storyboard")]
mod live {
    use std::rc::Rc;

    use gpui::{
        App, AppContext as _, Bounds, Context, FocusHandle, InteractiveElement as _, IntoElement,
        ParentElement as _, Render, SharedString, Styled as _, TitlebarOptions, Window,
        WindowAppearance, WindowBounds, WindowHandle, WindowId, WindowOptions, point, px, size,
    };
    use mira_ui::{
        ActiveTheme as _, FOCUS_SCOPE, Theme,
        primitives::{
            Button, List, ListItem,
            button::{Size, Variant},
        },
        v_flex,
    };

    use super::{Board, Stage, boards};
    use crate::ruffle_view::{bar_title, drag_area, window_bar};

    enum Row {
        Board(usize),
        Story(usize, usize),
    }

    struct Index {
        boards: Vec<Board>,
        rows: Vec<Row>,
        selected: Option<usize>,
        preview: Option<(usize, WindowHandle<Stage>)>,
        dark: bool,
        focus: FocusHandle,
    }

    pub(super) fn open(cx: &mut App) -> anyhow::Result<WindowId> {
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin: point(px(24.0), px(48.0)),
                size: size(px(260.0), px(640.0)),
            })),
            titlebar: Some(TitlebarOptions {
                title: Some("Storyboard".into()),
                appears_transparent: true,
                traffic_light_position: Some(point(px(12.0), px(12.0))),
            }),
            app_owns_titlebar_drag: true,
            ..Default::default()
        };
        let window = cx.open_window(options, |window, cx| {
            let index = cx.new(Index::new);
            let focus = index.read(cx).focus.clone();
            window.focus(&focus, cx);
            index
        })?;
        Ok(window.window_id())
    }

    impl Index {
        fn new(cx: &mut Context<Self>) -> Self {
            let boards = boards();
            let rows = boards
                .iter()
                .enumerate()
                .flat_map(|(board, stories)| {
                    std::iter::once(Row::Board(board)).chain(
                        (0..stories.stories.len()).map(move |story| Row::Story(board, story)),
                    )
                })
                .collect();
            Self {
                boards,
                rows,
                selected: None,
                preview: None,
                dark: matches!(
                    cx.window_appearance(),
                    WindowAppearance::Dark | WindowAppearance::VibrantDark
                ),
                focus: cx.focus_handle(),
            }
        }

        fn show(&mut self, row: usize, window: &mut Window, cx: &mut Context<Self>) {
            let Some(&Row::Story(board, story)) = self.rows.get(row) else {
                return;
            };
            self.selected = Some(row);
            cx.notify();
            let build = self.boards[board].stories[story].build;
            let replaced = self
                .preview
                .filter(|(shown, _)| *shown == board)
                .is_some_and(|(_, preview)| {
                    preview
                        .update(cx, |stage, window, cx| {
                            stage.0 = build(window, cx);
                            cx.notify();
                        })
                        .is_ok()
                });
            if replaced {
                return;
            }
            if let Some((_, preview)) = self.preview.take() {
                let _ = preview.update(cx, |_, window, _| window.remove_window());
            }
            let Board { size, min_size, .. } = self.boards[board];
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: point(px(300.0), px(48.0)),
                    size,
                })),
                titlebar: Some(TitlebarOptions {
                    title: Some(self.boards[board].name.into()),
                    appears_transparent: true,
                    traffic_light_position: Some(point(px(12.0), px(12.0))),
                }),
                app_owns_titlebar_drag: true,
                window_min_size: Some(min_size),
                ..Default::default()
            };
            match cx.open_window(options, |window, cx| {
                let view = build(window, cx);
                cx.new(|_| Stage(view))
            }) {
                Ok(preview) => {
                    self.preview = Some((board, preview));
                    window.activate_window();
                }
                Err(error) => eprintln!("couldn't open the story: {error:#}"),
            }
        }

        fn toggle_theme(&mut self, cx: &mut Context<Self>) {
            self.dark = !self.dark;
            cx.set_global(if self.dark {
                Theme::dark()
            } else {
                Theme::light()
            });
            cx.refresh_windows();
        }
    }

    impl Render for Index {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let theme = cx.theme().clone();
            let typography = theme.typography;
            let labels: Rc<[(bool, SharedString)]> = self
                .rows
                .iter()
                .map(|row| match *row {
                    Row::Board(board) => (true, self.boards[board].name.into()),
                    Row::Story(board, story) => {
                        (false, self.boards[board].stories[story].name.into())
                    }
                })
                .collect();
            let this = cx.weak_entity();
            v_flex()
                .size_full()
                .bg(theme.background)
                .text_size(typography.body)
                .text_color(theme.foreground)
                .key_context(FOCUS_SCOPE)
                .track_focus(&self.focus)
                .child(crate::quit::Overlay)
                .child(
                    window_bar(window, cx)
                        .child(bar_title("Storyboard", cx))
                        .child(drag_area().flex_1())
                        .child(
                            Button::new("theme", if self.dark { "Light" } else { "Dark" })
                                .variant(Variant::Ghost)
                                .size(Size::Small)
                                .on_click(cx.listener(|this, _, _, cx| this.toggle_theme(cx))),
                        ),
                )
                .child(
                    List::new(
                        "stories",
                        "Stories",
                        labels.len(),
                        move |index| match &labels[index] {
                            (true, label) => ListItem::header(label.clone()),
                            (false, label) => ListItem::new(label.clone()),
                        },
                    )
                    .selected(self.selected)
                    .fill()
                    .on_select(move |row, window, cx| {
                        this.update(cx, |index, cx| index.show(row, window, cx))
                            .ok();
                    }),
                )
        }
    }
}

#[cfg(test)]
mod tests {
    use gpui::AppContext as _;

    use super::*;

    #[test]
    fn snapshot_every_story() {
        for board in boards() {
            for story in board.stories {
                let name = format!("story-{}-{}", slug(board.name), slug(story.name));
                for written in mira_ui::snapshot(&name, board.min_size, move |window, cx| {
                    globals(cx);
                    let view = (story.build)(window, cx);
                    cx.new(|_| Stage(view))
                }) {
                    assert!(written.exists(), "{} was not written", written.display());
                }
            }
        }
    }
}
