use gpui::{
    App, BoxShadow, Div, Global, Hsla, ParentElement as _, Pixels, Rgba, Styled, WindowAppearance,
    div, point, px,
};

pub const DISABLED_OPACITY: f32 = 0.5;

pub const TEXT_CAPTION: Pixels = px(11.0);
pub const TEXT_SMALL: Pixels = px(12.0);
pub const TEXT_BODY: Pixels = px(13.0);
pub const TEXT_TITLE: Pixels = px(15.0);
pub const TEXT_DISPLAY: Pixels = px(22.0);

pub const LINE_CAPTION: Pixels = px(14.0);
pub const LINE_SMALL: Pixels = px(16.0);
pub const LINE_BODY: Pixels = px(18.0);
pub const LINE_TITLE: Pixels = px(20.0);
pub const LINE_DISPLAY: Pixels = px(28.0);

pub const SYSTEM_SANS: &str = ".SystemUIFont";
pub const SYSTEM_MONO: &str = ".AppleSystemUIFontMonospaced";

pub const CONTROL_SM: Pixels = px(24.0);
pub const CONTROL_MD: Pixels = px(28.0);
pub const ROW: Pixels = px(28.0);

#[derive(Clone, Copy)]
pub struct Radii {
    pub tag: Pixels,
    pub item: Pixels,
    pub control: Pixels,
    pub panel: Pixels,
    pub dialog: Pixels,
}

impl Radii {
    pub fn scaled(scale: f32) -> Self {
        let panel = px(12.0) * scale;
        Self {
            tag: px(4.0) * scale,
            item: (px(6.0) * scale).min(Self::inside(panel, PANEL_INSET)),
            control: px(8.0) * scale,
            panel,
            dialog: px(16.0) * scale,
        }
    }

    pub fn inside(outer: Pixels, inset: Pixels) -> Pixels {
        (outer - inset).max(Pixels::ZERO)
    }
}
pub const PANEL_INSET: Pixels = px(4.0);

pub const FOCUS_RING: Pixels = px(2.0);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tone {
    Neutral,
    Accent,
    Warning,
    Danger,
}

#[derive(Clone)]
pub struct Theme {
    pub radius: Radii,
    pub base: Hsla,
    pub well: Hsla,
    pub surface: Hsla,
    pub overlay: Hsla,
    pub fill: Hsla,
    pub fill_hover: Hsla,
    pub fill_pressed: Hsla,
    pub selected: Hsla,
    pub hairline: Hsla,
    pub border: Hsla,
    pub border_strong: Hsla,
    pub text: Hsla,
    pub text_secondary: Hsla,
    pub text_tertiary: Hsla,
    pub text_disabled: Hsla,
    pub inverse: Hsla,
    pub inverse_hover: Hsla,
    pub on_inverse: Hsla,
    pub knob: Hsla,
    pub thumb: Hsla,
    pub accent: Hsla,
    pub accent_soft: Hsla,
    pub accent_text: Hsla,
    pub warning: Hsla,
    pub warning_soft: Hsla,
    pub warning_text: Hsla,
    pub danger: Hsla,
    pub danger_hover: Hsla,
    pub danger_soft: Hsla,
    pub danger_text: Hsla,
    pub on_danger: Hsla,
    pub focus: Hsla,
    pub backdrop: Hsla,
    pub pointer_cursor: bool,
    pub stack_tint: f32,
    shadow_alpha: f32,
}

impl Global for Theme {}

impl Theme {
    pub fn dark() -> Self {
        let ink = |alpha| oklch(0.919, 0.0, 0.0, alpha);
        let gray = |lightness| oklch(lightness, 0.0, 0.0, 1.0);
        Self {
            radius: Radii::scaled(1.0),
            base: gray(0.17),
            well: gray(0.135),
            surface: gray(0.19),
            overlay: gray(0.215),
            fill: ink(0.05),
            fill_hover: ink(0.067),
            fill_pressed: ink(0.11),
            selected: ink(0.118),
            hairline: ink(0.075),
            border: ink(0.12),
            border_strong: ink(0.2),
            text: gray(0.87),
            text_secondary: gray(0.62),
            text_tertiary: gray(0.5),
            text_disabled: gray(0.38),
            inverse: gray(0.87),
            inverse_hover: gray(0.93),
            on_inverse: gray(0.17),
            knob: gray(0.9),
            thumb: gray(0.3),
            accent: oklch(0.697, 0.12, 249.0, 1.0),
            accent_soft: oklch(0.697, 0.12, 249.0, 0.14),
            accent_text: oklch(0.78, 0.09, 249.0, 1.0),
            warning: oklch(0.83, 0.14, 78.0, 1.0),
            warning_soft: oklch(0.83, 0.14, 78.0, 0.12),
            warning_text: oklch(0.86, 0.12, 80.0, 1.0),
            danger: oklch(0.6, 0.2, 25.0, 1.0),
            danger_hover: oklch(0.65, 0.2, 25.0, 1.0),
            danger_soft: oklch(0.65, 0.2, 25.0, 0.14),
            danger_text: oklch(0.75, 0.15, 25.0, 1.0),
            on_danger: oklch(0.99, 0.0, 0.0, 1.0),
            focus: ink(0.5),
            backdrop: oklch(0.05, 0.0, 0.0, 0.55),
            pointer_cursor: false,
            stack_tint: 0.06,
            shadow_alpha: 0.5,
        }
    }

    pub fn light() -> Self {
        let ink = |alpha| oklch(0.191, 0.0, 0.0, alpha);
        let gray = |lightness| oklch(lightness, 0.0, 0.0, 1.0);
        Self {
            radius: Radii::scaled(1.0),
            base: gray(0.98),
            well: gray(0.935),
            surface: gray(0.997),
            overlay: gray(1.0),
            fill: ink(0.045),
            fill_hover: ink(0.067),
            fill_pressed: ink(0.11),
            selected: ink(0.09),
            hairline: ink(0.075),
            border: ink(0.15),
            border_strong: ink(0.26),
            text: gray(0.26),
            text_secondary: gray(0.47),
            text_tertiary: gray(0.6),
            text_disabled: gray(0.72),
            inverse: gray(0.26),
            inverse_hover: gray(0.34),
            on_inverse: gray(0.997),
            knob: gray(1.0),
            thumb: gray(1.0),
            accent: oklch(0.565, 0.13, 243.0, 1.0),
            accent_soft: oklch(0.565, 0.13, 243.0, 0.12),
            accent_text: oklch(0.5, 0.11, 243.0, 1.0),
            warning: oklch(0.72, 0.15, 70.0, 1.0),
            warning_soft: oklch(0.75, 0.15, 75.0, 0.16),
            warning_text: oklch(0.48, 0.11, 65.0, 1.0),
            danger: oklch(0.56, 0.2, 25.0, 1.0),
            danger_hover: oklch(0.51, 0.19, 25.0, 1.0),
            danger_soft: oklch(0.6, 0.2, 25.0, 0.1),
            danger_text: oklch(0.5, 0.17, 25.0, 1.0),
            on_danger: oklch(0.99, 0.0, 0.0, 1.0),
            focus: ink(0.45),
            backdrop: ink(0.28),
            pointer_cursor: false,
            stack_tint: 0.01,
            shadow_alpha: 0.16,
        }
    }

    pub fn for_window(appearance: WindowAppearance) -> Self {
        match appearance {
            WindowAppearance::Dark | WindowAppearance::VibrantDark => Self::dark(),
            WindowAppearance::Light | WindowAppearance::VibrantLight => Self::light(),
        }
    }

    pub fn tone(&self, tone: Tone) -> (Hsla, Hsla, Hsla) {
        match tone {
            Tone::Neutral => (self.text_tertiary, self.fill, self.text_secondary),
            Tone::Accent => (self.accent, self.accent_soft, self.accent_text),
            Tone::Warning => (self.warning, self.warning_soft, self.warning_text),
            Tone::Danger => (self.danger, self.danger_soft, self.danger_text),
        }
    }

    pub fn focus_ring(&self, radius: Pixels) -> Div {
        self.ring_in(self.focus, radius)
    }

    pub fn ring_in(&self, color: Hsla, radius: Pixels) -> Div {
        self.ring_at(color, radius, px(1.0))
    }

    pub fn ring_at(&self, color: Hsla, radius: Pixels, gap: Pixels) -> Div {
        let outset = FOCUS_RING + gap;
        div()
            .absolute()
            .top(-outset)
            .left(-outset)
            .right(-outset)
            .bottom(-outset)
            .rounded(if radius > Pixels::ZERO {
                radius + outset
            } else {
                Pixels::ZERO
            })
            .border_2()
            .border_color(color)
            .child(crate::nav::reveal_probe())
    }

    pub fn raised(&self) -> Vec<BoxShadow> {
        vec![shadow(
            oklch(0.0, 0.0, 0.0, self.shadow_alpha * 0.35),
            1.0,
            1.0,
            px(0.0),
        )]
    }

    pub fn edge(&self) -> Vec<BoxShadow> {
        let dark = self.base.l < 0.5;
        vec![BoxShadow {
            color: if dark {
                oklch(1.0, 0.0, 0.0, 0.05)
            } else {
                oklch(0.0, 0.0, 0.0, 0.04)
            },
            offset: point(px(0.0), px(if dark { 1.0 } else { -1.0 })),
            blur_radius: px(0.0),
            spread_radius: px(0.0),
            inset: true,
        }]
    }

    pub fn floating(&self) -> Vec<BoxShadow> {
        vec![
            shadow(
                oklch(0.0, 0.0, 0.0, self.shadow_alpha * 1.4),
                16.0,
                20.0,
                px(-12.0),
            ),
            shadow(
                oklch(0.0, 0.0, 0.0, self.shadow_alpha * 0.6),
                2.0,
                3.0,
                px(0.0),
            ),
            BoxShadow {
                color: oklch(1.0, 0.0, 0.0, 0.07),
                offset: point(px(0.0), px(1.0)),
                blur_radius: px(0.0),
                spread_radius: px(0.0),
                inset: true,
            },
        ]
    }
}

// GPUI's blur radius is the Gaussian sigma, half of a CSS blur value.
fn shadow(color: Hsla, y: f32, blur: f32, spread: Pixels) -> BoxShadow {
    BoxShadow {
        color,
        offset: point(px(0.0), px(y)),
        blur_radius: px(blur),
        spread_radius: spread,
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
