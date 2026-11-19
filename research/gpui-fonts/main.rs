mod measure;

use std::borrow::Cow;

use gpui::{
    App, AppContext as _, Bounds, Context, Div, FontWeight, IntoElement, ParentElement as _,
    Render, SharedString, Styled as _, TitlebarOptions, Window, WindowBounds, WindowOptions,
    actions, div, point, px, rgb, size,
};

const INTER: &[u8] = include_bytes!("Inter.ttf");
const FAMILIES: [(&str, &str); 2] = [
    ("Apple system", ".SystemUIFont"),
    ("Inter Variable", "Inter"),
];
const WEIGHTS: [FontWeight; 3] = [FontWeight::NORMAL, FontWeight::MEDIUM, FontWeight::SEMIBOLD];
const DEFAULT_SAMPLE: &str = "Accounts and combat profiles";

actions!(comparison, [Close]);

struct Specimen(SharedString);

impl Specimen {
    fn panel(&self, dark: bool) -> Div {
        let (paper, ink, caption, edge) = if dark {
            (0x0a0a0a, 0xfafafa, 0xa1a1a1, 0x303030)
        } else {
            (0xffffff, 0x0a0a0a, 0x616161, 0xe5e5e5)
        };
        let row = || div().flex().items_center().gap(px(12.));
        let column = || div().flex_1().min_w_0();
        let header = row()
            .pb(px(8.))
            .border_b_1()
            .border_color(rgb(edge))
            .text_color(rgb(caption))
            .child(div().w(px(48.)).child("Weight"))
            .children(FAMILIES.map(|(label, _)| column().child(label)));
        div()
            .flex()
            .flex_col()
            .flex_1()
            .justify_center()
            .px(px(18.))
            .py(px(12.))
            .gap(px(8.))
            .bg(rgb(paper))
            .text_color(rgb(ink))
            .child(header)
            .children(WEIGHTS.map(|weight| {
                row()
                    .child(
                        div()
                            .w(px(48.))
                            .text_color(rgb(caption))
                            .child(weight.0.to_string()),
                    )
                    .children(FAMILIES.map(|(_, family)| {
                        column()
                            .flex()
                            .flex_col()
                            .font_family(family)
                            .font_weight(weight)
                            .children([(13., 20.), (11., 16.)].map(|(size, leading)| {
                                div()
                                    .truncate()
                                    .text_size(px(size))
                                    .line_height(px(leading))
                                    .child(self.0.clone())
                            }))
                    }))
            }))
    }
}

impl Render for Specimen {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .font_family(".SystemUIFont")
            .text_size(px(11.))
            .line_height(px(16.))
            .child(self.panel(false))
            .child(self.panel(true))
            .child(
                div()
                    .bg(rgb(0x171717))
                    .text_color(rgb(0xc2c2c2))
                    .px(px(18.))
                    .py(px(7.))
                    .child("13 px on the first line, 11 px on the second."),
            )
    }
}

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let mode = args.next().unwrap_or_default();
    let sample = args.next().unwrap_or_else(|| DEFAULT_SAMPLE.into());
    match mode.as_str() {
        "--measure" => return measure::run(Cow::Borrowed(INTER)),
        "--measure-owned" => return measure::run(Cow::Owned(INTER.to_vec())),
        "Before" | "After" => {}
        _ => anyhow::bail!("Choose Before, After or --measure"),
    }
    gpui_platform::application().run(move |cx: &mut App| {
        cx.text_system()
            .add_fonts(vec![Cow::Borrowed(INTER)])
            .expect("load bundled Inter");
        let mut bounds = Bounds::centered(None, size(px(550.), px(460.)), cx);
        if let Some(display) = cx.primary_display() {
            let screen = display.visible_bounds();
            let width = px(550.).min((screen.size.width - px(32.)) / 2.);
            let height = px(460.).min(screen.size.height);
            let x = if mode == "Before" {
                screen.center().x - width - px(5.)
            } else {
                screen.center().x + px(5.)
            };
            bounds = Bounds::new(
                point(x, screen.center().y - height / 2.),
                size(width, height),
            );
        }
        cx.bind_keys([
            gpui::KeyBinding::new("cmd-w", Close, None),
            gpui::KeyBinding::new("cmd-q", Close, None),
        ]);
        cx.on_action(|_: &Close, cx| cx.quit());
        cx.open_window(
            WindowOptions {
                titlebar: Some(TitlebarOptions {
                    title: Some(format!("GPUI fonts · {mode}").into()),
                    ..Default::default()
                }),
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                tabbing_identifier: None,
                ..Default::default()
            },
            |_, cx| cx.new(|_| Specimen(sample.into())),
        )
        .expect("open the font comparison");
        cx.on_window_closed(|cx, _| cx.quit()).detach();
        cx.activate(true);
    });
    Ok(())
}
