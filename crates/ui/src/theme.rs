use gpui::{
    App, BoxShadow, Div, Global, Hsla, ParentElement as _, Pixels, Rgba, Styled, WindowAppearance,
    div, point, px,
};

pub const DISABLED_OPACITY: f32 = 0.5;
pub const UI_FONT: &str = "Inter";
pub const SYSTEM_MONO: &str = ".AppleSystemUIFontMonospaced";

pub const TEXT_BODY: Pixels = px(13.0);
pub const TEXT_CAPTION: Pixels = px(11.0);
pub const LINE_BODY: Pixels = px(20.0);
pub const LINE_CAPTION: Pixels = px(16.0);
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
}

impl Default for Typography {
    fn default() -> Self {
        Self {
            body: TEXT_BODY,
            secondary: TEXT_CAPTION,
            body_line: LINE_BODY,
            secondary_line: LINE_CAPTION,
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
    pub script_list_max: Pixels,
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
            script_list_max: px(288.0),
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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tone {
    Neutral,
    Accent,
    Success,
    Warning,
    Danger,
}

#[derive(Clone)]
pub struct Theme {
    pub dark_mode: bool,
    pub font_family: gpui::SharedString,
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
            font_family: UI_FONT.into(),
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
        div()
            .absolute()
            .inset(-border - width)
            .rounded(radius + width)
            .border(width)
            .border_color(color)
            .child(crate::nav::reveal_probe())
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

pub fn perceptual_mix(from: Hsla, to: Hsla, t: f32) -> Hsla {
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
