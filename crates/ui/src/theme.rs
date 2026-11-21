use gpui::{
    App, BoxShadow, Div, Global, Hsla, ParentElement as _, Pixels, Rgba, SharedString, Styled,
    WindowAppearance, div, point, px,
};
use serde::{Deserialize, Serialize};

pub const DISABLED_OPACITY: f32 = 0.5;
pub(crate) const SYSTEM_SANS: &str = ".SystemUIFont";
pub(crate) const SYSTEM_MONO: &str = ".AppleSystemUIFontMonospaced";

pub const TEXT_BODY: Pixels = px(13.0);
pub const TEXT_CAPTION: Pixels = px(11.0);
pub const LINE_BODY: Pixels = px(20.0);
pub const LINE_CAPTION: Pixels = px(16.0);
pub const TEXT_HEADING: Pixels = px(16.0);
pub const LINE_HEADING: Pixels = px(24.0);
pub const CONTROL_XS: Pixels = px(20.0);
pub const CONTROL_SM: Pixels = px(24.0);
pub const CONTROL_MD: Pixels = px(28.0);
pub const CONTROL_LG: Pixels = px(32.0);
pub const RING_ROOM: Pixels = px(3.0);

#[derive(Clone, Copy)]
pub struct Typography {
    pub body: Pixels,
    pub secondary: Pixels,
    pub body_line: Pixels,
    pub secondary_line: Pixels,
    pub heading: Pixels,
    pub heading_line: Pixels,
    pub mono: Pixels,
}

impl Typography {
    pub fn sized(body: f32) -> Self {
        let secondary = body - 2.0;
        let heading = (body * 16.0 / 13.0).round();
        Self {
            body: px(body),
            secondary: px(secondary),
            body_line: px((body * 20.0 / 13.0).round()),
            secondary_line: px((secondary * 16.0 / 11.0).round()),
            heading: px(heading),
            heading_line: px((heading * 1.5).round()),
            mono: TEXT_CAPTION,
        }
    }
}

impl Default for Typography {
    fn default() -> Self {
        Self {
            body: TEXT_BODY,
            secondary: TEXT_CAPTION,
            body_line: LINE_BODY,
            secondary_line: LINE_CAPTION,
            heading: TEXT_HEADING,
            heading_line: LINE_HEADING,
            mono: TEXT_CAPTION,
        }
    }
}

#[derive(Clone, Copy)]
pub struct Layout {
    pub unit: Pixels,
    pub control_xs: Pixels,
    pub control_sm: Pixels,
    pub control: Pixels,
    pub control_lg: Pixels,
    pub cell_header: Pixels,
    pub check: Pixels,
    pub check_mark: Pixels,
    pub chip: Pixels,
    pub track: Pixels,
    pub thumb: Pixels,
    pub chrome: Pixels,
    pub toolbar: Pixels,
    pub sidebar: Pixels,
    pub inspector: Pixels,
    pub compact_overlay: Pixels,
    pub overlay_max_width: Pixels,
    pub split_forms: Pixels,
    pub split_packets: Pixels,
    pub card_min: Pixels,
}
impl Default for Layout {
    fn default() -> Self {
        Self {
            unit: px(4.0),
            control_xs: CONTROL_XS,
            control_sm: CONTROL_SM,
            control: CONTROL_MD,
            control_lg: CONTROL_LG,
            cell_header: px(40.0),
            check: px(16.0),
            check_mark: px(13.0),
            chip: CONTROL_SM,
            track: px(4.0),
            thumb: px(12.0),
            chrome: px(38.0),
            toolbar: px(40.0),
            sidebar: px(208.0),
            inspector: px(340.0),
            compact_overlay: px(640.0),
            overlay_max_width: px(384.0),
            split_forms: px(560.0),
            split_packets: px(760.0),
            card_min: px(240.0),
        }
    }
}
impl Layout {
    pub fn space(&self, units: f32) -> Pixels {
        self.unit * units
    }
}

#[derive(Clone, Copy)]
pub struct Radii {
    pub xs: Pixels,
    pub tag: Pixels,
    pub small: Pixels,
    pub control: Pixels,
    pub panel: Pixels,
    pub dialog: Pixels,
}
impl Radii {
    pub fn scaled(scale: f32) -> Self {
        Self {
            xs: px(2.0) * scale,
            tag: px(4.0) * scale,
            small: px(6.0) * scale,
            control: px(8.0) * scale,
            panel: px(10.0) * scale,
            dialog: px(14.0) * scale,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ColorToken {
    Accent,
    Background,
    Foreground,
    Card,
    Popover,
    Sidebar,
    Muted,
    MutedForeground,
    Border,
    Ring,
    Destructive,
}

impl ColorToken {
    pub const SEEDS: [ColorToken; 3] = [
        ColorToken::Accent,
        ColorToken::Background,
        ColorToken::Foreground,
    ];

    pub const ADVANCED: [ColorToken; 8] = [
        ColorToken::Card,
        ColorToken::Popover,
        ColorToken::Sidebar,
        ColorToken::Muted,
        ColorToken::MutedForeground,
        ColorToken::Border,
        ColorToken::Ring,
        ColorToken::Destructive,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ColorToken::Accent => "Accent",
            ColorToken::Background => "Background",
            ColorToken::Foreground => "Foreground",
            ColorToken::Card => "Card",
            ColorToken::Popover => "Popover",
            ColorToken::Sidebar => "Sidebar",
            ColorToken::Muted => "Muted",
            ColorToken::MutedForeground => "Muted text",
            ColorToken::Border => "Border",
            ColorToken::Ring => "Focus ring",
            ColorToken::Destructive => "Destructive",
        }
    }

    fn is_seed(self) -> bool {
        Self::SEEDS.contains(&self)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tone {
    Neutral,
    Accent,
    Success,
    Warning,
    Attention,
    Danger,
}

#[derive(Clone)]
pub struct Theme {
    pub dark_mode: bool,
    pub font_family: gpui::SharedString,
    pub mono_family: gpui::SharedString,
    pub typography: Typography,
    pub layout: Layout,
    pub radius: Radii,
    pub pointer_cursor: bool,
    pub background: Hsla,
    pub foreground: Hsla,
    pub card: Hsla,
    pub card_foreground: Hsla,
    pub popover: Hsla,
    pub popover_foreground: Hsla,
    pub primary: Hsla,
    pub primary_foreground: Hsla,
    pub secondary: Hsla,
    pub secondary_foreground: Hsla,
    pub muted: Hsla,
    pub muted_foreground: Hsla,
    pub accent: Hsla,
    pub accent_foreground: Hsla,
    pub destructive: Hsla,
    pub warning: Hsla,
    pub border: Hsla,
    pub input: Hsla,
    pub ring: Hsla,
    pub sidebar: Hsla,
    pub sidebar_foreground: Hsla,
    pub sidebar_primary: Hsla,
    pub sidebar_primary_foreground: Hsla,
    pub sidebar_accent: Hsla,
    pub sidebar_accent_foreground: Hsla,
    pub sidebar_border: Hsla,
    pub sidebar_ring: Hsla,
    pub chart: [Hsla; 5],
}
impl Global for Theme {}

impl Theme {
    pub fn light() -> Self {
        Self::preset(false)
    }
    pub fn dark() -> Self {
        Self::preset(true)
    }
    fn preset(dark: bool) -> Self {
        let gray = |l| oklch(l, 0.0, 0.0, 1.0);
        let pair = |l, d| gray(if dark { d } else { l });
        let foreground = pair(0.145, 0.985);
        let primary = pair(0.205, 0.922);
        let secondary = pair(0.97, 0.269);
        let secondary_foreground = pair(0.205, 0.985);
        let border = if dark {
            gray(1.0).opacity(0.10)
        } else {
            gray(0.922)
        };
        let ring = pair(0.708, 0.556);
        Self {
            dark_mode: dark,
            font_family: SYSTEM_SANS.into(),
            mono_family: SYSTEM_MONO.into(),
            typography: Typography::default(),
            layout: Layout::default(),
            radius: Radii::scaled(1.0),
            pointer_cursor: false,
            background: pair(1.0, 0.145),
            foreground,
            card: pair(1.0, 0.205),
            card_foreground: foreground,
            popover: pair(1.0, 0.205),
            popover_foreground: foreground,
            primary,
            primary_foreground: pair(0.985, 0.205),
            secondary,
            secondary_foreground,
            muted: secondary,
            muted_foreground: pair(0.556, 0.708),
            accent: secondary,
            accent_foreground: secondary_foreground,
            destructive: if dark {
                oklch(0.704, 0.191, 22.216, 1.0)
            } else {
                oklch(0.577, 0.245, 27.325, 1.0)
            },
            warning: if dark {
                oklch(0.828, 0.189, 84.429, 1.0)
            } else {
                oklch(0.555, 0.163, 48.998, 1.0)
            },
            border,
            input: if dark {
                gray(1.0).opacity(0.15)
            } else {
                border
            },
            ring,
            sidebar: pair(0.985, 0.205),
            sidebar_foreground: foreground,
            sidebar_primary: if dark {
                oklch(0.488, 0.243, 264.376, 1.0)
            } else {
                primary
            },
            sidebar_primary_foreground: gray(0.985),
            sidebar_accent: secondary,
            sidebar_accent_foreground: secondary_foreground,
            sidebar_border: border,
            sidebar_ring: ring,
            chart: [0.87, 0.556, 0.439, 0.371, 0.269].map(gray),
        }
    }
    pub fn for_window(appearance: WindowAppearance) -> Self {
        Self::preset(matches!(
            appearance,
            WindowAppearance::Dark | WindowAppearance::VibrantDark
        ))
    }
    pub fn space(&self, units: f32) -> Pixels {
        self.layout.space(units)
    }
    pub fn customized(dark: bool, colors: &[(ColorToken, Hsla)]) -> Self {
        let preset = Self::preset(dark);
        let mut theme = preset.clone();
        let chosen = |token| {
            colors
                .iter()
                .find(|(chosen, _)| *chosen == token)
                .map(|&(_, color)| color)
        };
        if colors
            .iter()
            .any(|(token, _)| matches!(token, ColorToken::Background | ColorToken::Foreground))
        {
            theme.derive(
                &preset,
                chosen(ColorToken::Background).unwrap_or(preset.background),
                chosen(ColorToken::Foreground).unwrap_or(preset.foreground),
            );
        }
        let (seeds, advanced): (Vec<_>, Vec<_>) =
            colors.iter().partition(|(token, _)| token.is_seed());
        for &(token, color) in seeds.into_iter().chain(advanced) {
            theme.set_color(token, color);
        }
        theme
    }

    fn derive(&mut self, preset: &Theme, background: Hsla, foreground: Hsla) {
        let base = lightness(preset.background);
        let span = lightness(preset.foreground) - base;
        let (bl, bc, bh) = polar(background);
        let surface = |role: Hsla| oklch(bl + lightness(role) - base, bc, bh, 1.0);
        let between = |role: Hsla| {
            let mixed = perceptual_mix(background, foreground, (lightness(role) - base) / span);
            if role.a < 1.0 {
                foreground.opacity(role.a)
            } else {
                mixed
            }
        };
        self.background = background;
        self.card = surface(preset.card);
        self.popover = surface(preset.popover);
        self.sidebar = surface(preset.sidebar);
        self.secondary = surface(preset.secondary);
        self.muted = self.secondary;
        self.accent = self.secondary;
        self.sidebar_accent = self.secondary;
        self.foreground = foreground;
        self.card_foreground = foreground;
        self.popover_foreground = foreground;
        self.sidebar_foreground = foreground;
        self.secondary_foreground = between(preset.secondary_foreground);
        self.accent_foreground = self.secondary_foreground;
        self.sidebar_accent_foreground = self.secondary_foreground;
        self.muted_foreground = between(preset.muted_foreground);
        self.border = between(preset.border);
        self.input = between(preset.input);
        self.sidebar_border = self.border;
        self.ring = between(preset.ring);
        self.sidebar_ring = self.ring;
    }

    pub fn color(&self, token: ColorToken) -> Hsla {
        let role = match token {
            ColorToken::Accent => self.primary,
            ColorToken::Background => self.background,
            ColorToken::Foreground => self.foreground,
            ColorToken::Card => self.card,
            ColorToken::Popover => self.popover,
            ColorToken::Sidebar => self.sidebar,
            ColorToken::Muted => self.muted,
            ColorToken::MutedForeground => self.muted_foreground,
            ColorToken::Border => self.border,
            ColorToken::Ring => self.ring,
            ColorToken::Destructive => self.destructive,
        };
        if role.a < 1.0 {
            self.background.blend(role)
        } else {
            role
        }
    }

    pub fn set_color(&mut self, token: ColorToken, color: Hsla) {
        match token {
            ColorToken::Accent => {
                self.primary = color;
                self.primary_foreground =
                    oklch(if is_dark(color) { 0.985 } else { 0.205 }, 0.0, 0.0, 1.0);
                if !self.dark_mode {
                    self.sidebar_primary = color;
                    self.sidebar_primary_foreground = self.primary_foreground;
                }
            }
            ColorToken::Background => self.background = color,
            ColorToken::Foreground => {
                self.foreground = color;
                self.card_foreground = color;
                self.popover_foreground = color;
                self.sidebar_foreground = color;
            }
            ColorToken::Card => self.card = color,
            ColorToken::Popover => self.popover = color,
            ColorToken::Sidebar => self.sidebar = color,
            ColorToken::Muted => {
                self.secondary = color;
                self.muted = color;
                self.accent = color;
                self.sidebar_accent = color;
            }
            ColorToken::MutedForeground => self.muted_foreground = color,
            ColorToken::Border => {
                self.border = color;
                self.input = color;
                self.sidebar_border = color;
            }
            ColorToken::Ring => {
                self.ring = color;
                self.sidebar_ring = color;
            }
            ColorToken::Destructive => self.destructive = color,
        }
    }

    pub fn input_fill(&self) -> Hsla {
        self.input.opacity(if self.dark_mode { 0.3 } else { 0.2 })
    }
    pub fn outline_fill(&self) -> Hsla {
        self.input.opacity(if self.dark_mode { 0.3 } else { 0.0 })
    }
    pub fn ghost_hover(&self) -> Hsla {
        self.muted.opacity(if self.dark_mode { 0.5 } else { 1.0 })
    }
    pub fn destructive_fill(&self) -> Hsla {
        self.destructive
            .opacity(if self.dark_mode { 0.2 } else { 0.1 })
    }
    pub fn warning_fill(&self) -> Hsla {
        self.warning.opacity(if self.dark_mode { 0.2 } else { 0.1 })
    }
    pub fn invalid_halo(&self) -> Hsla {
        self.destructive
            .opacity(if self.dark_mode { 0.4 } else { 0.2 })
    }
    pub fn invalid_border(&self) -> Hsla {
        self.destructive
            .opacity(if self.dark_mode { 0.5 } else { 1.0 })
    }
    pub fn pressed(&self, surface: Hsla, hover: Hsla, ink: Hsla) -> Hsla {
        perceptual_mix(surface.blend(hover), ink, 0.1)
    }
    pub fn tone(&self, tone: Tone) -> (Hsla, Hsla, Hsla) {
        match tone {
            Tone::Neutral => (
                self.muted_foreground,
                self.secondary,
                self.secondary_foreground,
            ),
            Tone::Success => (self.foreground, self.secondary, self.secondary_foreground),
            Tone::Accent | Tone::Warning => (self.foreground, self.input_fill(), self.foreground),
            Tone::Attention => (self.warning, self.warning_fill(), self.warning),
            Tone::Danger => (self.destructive, self.destructive_fill(), self.destructive),
        }
    }
    pub fn focus_ring(&self, radius: Pixels) -> Div {
        self.ring_around(self.ring, radius, Pixels::ZERO)
    }
    pub fn ring_around(&self, color: Hsla, radius: Pixels, border: Pixels) -> Div {
        self.halo(color.opacity(0.30), radius, border, px(2.0))
    }
    pub fn halo(&self, color: Hsla, radius: Pixels, border: Pixels, width: Pixels) -> Div {
        self.outline(color, radius, border, width)
            .child(crate::nav::reveal_probe())
    }
    pub fn outline(&self, color: Hsla, radius: Pixels, border: Pixels, width: Pixels) -> Div {
        div()
            .absolute()
            .inset(-border - width)
            .rounded(radius + width)
            .border(width)
            .border_color(color)
    }
    pub fn surface_ring(&self, radius: Pixels) -> Div {
        div()
            .absolute()
            .inset(px(-1.0))
            .rounded(radius + px(1.0))
            .border_1()
            .border_color(self.foreground.opacity(0.1))
    }
    pub fn popup_shadow(&self) -> Vec<BoxShadow> {
        let black = gpui::black().opacity(0.1);
        vec![shadow(black, 4.0, 6.0, -1.0), shadow(black, 2.0, 4.0, -2.0)]
    }
    pub fn large_shadow(&self) -> Vec<BoxShadow> {
        let black = gpui::black().opacity(0.1);
        vec![
            shadow(black, 10.0, 15.0, -3.0),
            shadow(black, 4.0, 6.0, -4.0),
        ]
    }
}

// The Metal renderer takes Gaussian sigma; the design recipes specify CSS blur.
fn shadow(color: Hsla, y: f32, blur: f32, spread: f32) -> BoxShadow {
    BoxShadow {
        color,
        offset: point(px(0.0), px(y)),
        blur_radius: px(blur * 0.5),
        spread_radius: px(spread),
        inset: false,
    }
}
pub fn oklch(l: f32, c: f32, h: f32, alpha: f32) -> Hsla {
    let (a, b) = (c * h.to_radians().cos(), c * h.to_radians().sin());
    let l_ = (l + 0.396_337_78 * a + 0.215_803_76 * b).powi(3);
    let m_ = (l - 0.105_561_35 * a - 0.063_854_17 * b).powi(3);
    let s_ = (l - 0.089_484_18 * a - 1.291_485_5 * b).powi(3);
    let encode = |x: f32| {
        let x = x.clamp(0.0, 1.0);
        if x <= 0.003_130_8 {
            12.92 * x
        } else {
            1.055 * x.powf(1.0 / 2.4) - 0.055
        }
    };
    Rgba {
        r: encode(4.076_741_7 * l_ - 3.307_711_6 * m_ + 0.230_969_94 * s_),
        g: encode(-1.268_438 * l_ + 2.609_757_4 * m_ - 0.341_319_4 * s_),
        b: encode(-0.004_196_086 * l_ - 0.703_418_6 * m_ + 1.707_614_7 * s_),
        a: alpha,
    }
    .into()
}

pub fn is_dark(color: Hsla) -> bool {
    let rgb = color.to_rgb();
    let linear = |v: f32| {
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(rgb.r) + 0.7152 * linear(rgb.g) + 0.0722 * linear(rgb.b) <= 0.179
}

pub fn to_hex(color: Hsla) -> SharedString {
    let rgb = color.to_rgb();
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02X}{:02X}{:02X}", byte(rgb.r), byte(rgb.g), byte(rgb.b)).into()
}

pub fn parse_hex(text: &str) -> Option<Hsla> {
    let digits = text.trim();
    let digits = digits.strip_prefix('#').unwrap_or(digits);
    if digits.len() != 6 || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let value = u32::from_str_radix(digits, 16).ok()?;
    Some(gpui::rgb(value).into())
}

pub trait PointerCursor: Styled + Sized {
    fn pointer_cursor(self, theme: &Theme) -> Self {
        if theme.pointer_cursor {
            self.cursor_pointer()
        } else {
            self
        }
    }
}

impl<T: Styled> PointerCursor for T {}

pub trait ActiveTheme {
    fn theme(&self) -> &Theme;
}

impl ActiveTheme for App {
    fn theme(&self) -> &Theme {
        self.global::<Theme>()
    }
}

pub fn mix(from: Hsla, to: Hsla, t: f32) -> Hsla {
    let (a, b) = (from.to_rgb(), to.to_rgb());
    let lerp = |x: f32, y: f32| x + (y - x) * t;
    Rgba {
        r: lerp(a.r, b.r),
        g: lerp(a.g, b.g),
        b: lerp(a.b, b.b),
        a: lerp(a.a, b.a),
    }
    .into()
}

pub fn tabular() -> gpui::FontFeatures {
    gpui::FontFeatures(std::sync::Arc::new(vec![("tnum".into(), 1)]))
}

fn polar(color: Hsla) -> (f32, f32, f32) {
    let rgb = color.to_rgb();
    let linear = |v: f32| {
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    let (r, g, b) = (linear(rgb.r), linear(rgb.g), linear(rgb.b));
    let l = (0.41222146 * r + 0.53633255 * g + 0.051445995 * b).cbrt();
    let m = (0.2119035 * r + 0.6806995 * g + 0.10739696 * b).cbrt();
    let s = (0.08830246 * r + 0.28171885 * g + 0.6299787 * b).cbrt();
    let lightness = 0.21045426 * l + 0.7936178 * m - 0.004072047 * s;
    let a = 1.9779985 * l - 2.4285922 * m + 0.4505937 * s;
    let b = 0.025904037 * l + 0.78277177 * m - 0.80867577 * s;
    (lightness, a.hypot(b), b.atan2(a).to_degrees())
}

fn lightness(color: Hsla) -> f32 {
    polar(color).0
}

pub fn perceptual_mix(from: Hsla, to: Hsla, t: f32) -> Hsla {
    let (l, c, mut h) = polar(from);
    let (l2, c2, mut h2) = polar(to);
    if c < 0.00001 {
        h = h2;
    }
    if c2 < 0.00001 {
        h2 = h;
    }
    let hue = (h2 - h + 180.0).rem_euclid(360.0) - 180.0;
    oklch(
        l + (l2 - l) * t,
        c + (c2 - c) * t,
        h + hue * t,
        from.a + (to.a - from.a) * t,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_text_size_keeps_the_design_metrics() {
        let sized = Typography::sized(13.0);
        let default = Typography::default();
        assert_eq!(
            (
                sized.body,
                sized.secondary,
                sized.body_line,
                sized.secondary_line,
                sized.heading,
                sized.heading_line
            ),
            (
                default.body,
                default.secondary,
                default.body_line,
                default.secondary_line,
                default.heading,
                default.heading_line
            )
        );
    }

    #[test]
    fn without_custom_colors_a_theme_is_its_preset() {
        for dark in [false, true] {
            let preset = Theme::preset(dark);
            let theme = Theme::customized(dark, &[]);
            for token in ColorToken::SEEDS.into_iter().chain(ColorToken::ADVANCED) {
                assert_eq!(to_hex(theme.color(token)), to_hex(preset.color(token)));
            }
        }
    }

    #[test]
    fn a_custom_background_carries_its_tint_and_the_preset_steps_to_every_surface() {
        let preset = Theme::dark();
        let navy = parse_hex("#101830").unwrap();
        let theme = Theme::customized(true, &[(ColorToken::Background, navy)]);
        let step = |role: Hsla, base: Hsla| lightness(role) - lightness(base);
        for (role, was) in [
            (theme.card, preset.card),
            (theme.popover, preset.popover),
            (theme.sidebar, preset.sidebar),
            (theme.muted, preset.muted),
        ] {
            assert!((step(role, navy) - step(was, preset.background)).abs() < 0.005);
            assert!(
                (polar(role).2 - polar(navy).2).abs() < 2.0,
                "{role:?} lost the tint"
            );
        }
        assert_eq!(theme.border, preset.foreground.opacity(0.10));
        let muted = lightness(theme.muted_foreground);
        assert!(lightness(navy) < muted && muted < lightness(theme.foreground));
    }

    #[test]
    fn an_advanced_color_overrides_what_the_seeds_derive() {
        let card = parse_hex("#202020").unwrap();
        let theme = Theme::customized(
            true,
            &[
                (ColorToken::Card, card),
                (ColorToken::Background, parse_hex("#101830").unwrap()),
            ],
        );
        assert_eq!(to_hex(theme.card), "#202020");
    }
}
