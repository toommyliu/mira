use std::{fmt::Write as _, fs, path::Path};

use gpui::Hsla;
use mira_ui::Theme;

fn css_color(color: Hsla) -> String {
    let rgb = color.to_rgb();
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    let (r, g, b) = (byte(rgb.r), byte(rgb.g), byte(rgb.b));
    if rgb.a >= 1.0 {
        format!("#{r:02x}{g:02x}{b:02x}")
    } else {
        format!("rgb({r} {g} {b} / {})", (rgb.a * 1000.0).round() / 1000.0)
    }
}

fn roles(theme: &Theme) -> [(&'static str, Hsla); 29] {
    [
        ("background", theme.background),
        ("foreground", theme.foreground),
        ("card", theme.card),
        ("card-foreground", theme.card_foreground),
        ("popover", theme.popover),
        ("popover-foreground", theme.popover_foreground),
        ("primary", theme.primary),
        ("primary-foreground", theme.primary_foreground),
        ("secondary", theme.secondary),
        ("secondary-foreground", theme.secondary_foreground),
        ("muted", theme.muted),
        ("muted-foreground", theme.muted_foreground),
        ("accent", theme.accent),
        ("accent-foreground", theme.accent_foreground),
        ("destructive", theme.destructive),
        ("warning", theme.warning),
        ("border", theme.border),
        ("input", theme.input),
        ("ring", theme.ring),
        ("frame", theme.frame),
        ("sidebar-foreground", theme.sidebar_foreground),
        ("sidebar-accent", theme.sidebar_accent),
        ("sidebar-accent-foreground", theme.sidebar_accent_foreground),
        ("sidebar-border", theme.sidebar_border),
        ("sidebar-ring", theme.sidebar_ring),
        ("input-fill", theme.input_fill()),
        ("ghost-hover", theme.ghost_hover()),
        ("warning-fill", theme.warning_fill()),
        ("surface-edge", theme.surface_edge()),
    ]
}

fn theme_css() -> String {
    let (light, dark) = (Theme::light(), Theme::dark());
    let mut css = String::from(
        "/* Generated from crates/ui/src/theme.rs by `cargo test -p mira-ui --test docs_theme`. */\n:root {\n",
    );
    for ((name, light), (_, dark)) in roles(&light).into_iter().zip(roles(&dark)) {
        let (light, dark) = (css_color(light), css_color(dark));
        if light == dark {
            writeln!(css, "  --{name}: {light};").unwrap();
        } else {
            writeln!(css, "  --{name}: light-dark({light}, {dark});").unwrap();
        }
    }
    let radius = light.radius;
    for (name, value) in [
        ("xs", radius.xs),
        ("tag", radius.tag),
        ("small", radius.small),
        ("control", radius.control),
        ("panel", radius.panel),
        ("dialog", radius.dialog),
    ] {
        writeln!(css, "  --radius-{name}: {}px;", f32::from(value)).unwrap();
    }
    css.push_str("}\n");
    css
}

#[test]
fn the_docs_site_paints_with_the_app_theme() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packages/docs/src/styles/theme.css");
    let css = theme_css();
    if fs::read_to_string(&path).ok().as_deref() != Some(css.as_str()) {
        fs::write(&path, css).unwrap();
        panic!(
            "{} was out of date with the theme and has been rewritten. Commit it.",
            path.display()
        );
    }
}
