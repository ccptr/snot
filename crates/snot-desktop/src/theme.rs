//! Colours for the light and dark appearance.

use gpui::{Hsla, Rgba, WindowAppearance, rgb, rgba};

#[derive(Clone, Copy)]
pub struct Theme {
    pub dark: bool,
    /// The window behind everything.
    pub app_bg: Hsla,
    pub sidebar_bg: Hsla,
    pub list_bg: Hsla,
    pub page_bg: Hsla,
    pub card_bg: Hsla,
    pub card_hover: Hsla,
    pub card_selected: Hsla,
    pub border: Hsla,
    pub text: Hsla,
    pub text_muted: Hsla,
    pub text_faint: Hsla,
    pub accent: Hsla,
    pub accent_text: Hsla,
    pub accent_soft: Hsla,
    pub hover: Hsla,
    pub selection: Hsla,
    pub caret: Hsla,
    pub danger: Hsla,
    pub paper_rule: Hsla,
    pub popover_bg: Hsla,
    pub shadow: Hsla,
    pub quote_bar: Hsla,
    pub code_bg: Hsla,
}

fn c(v: u32) -> Hsla {
    rgb(v).into()
}

fn ca(v: u32) -> Hsla {
    rgba(v).into()
}

impl Theme {
    /// The theme for the system appearance, unless `SNOT_THEME` is set to
    /// `light` or `dark`.
    pub fn for_appearance(appearance: WindowAppearance) -> Self {
        match std::env::var("SNOT_THEME").as_deref() {
            Ok("light") => return Self::light(),
            Ok("dark") => return Self::dark(),
            _ => {}
        }
        match appearance {
            WindowAppearance::Dark | WindowAppearance::VibrantDark => Self::dark(),
            WindowAppearance::Light | WindowAppearance::VibrantLight => Self::light(),
        }
    }

    pub fn light() -> Self {
        Theme {
            dark: false,
            app_bg: c(0xf3f2ef),
            sidebar_bg: c(0xf3f2ef),
            list_bg: c(0xfaf9f7),
            page_bg: c(0xffffff),
            card_bg: c(0xffffff),
            card_hover: c(0xf6f5f2),
            card_selected: c(0xfdf0e2),
            border: c(0xe4e2dd),
            text: c(0x1f1f22),
            text_muted: c(0x66656b),
            text_faint: c(0x9d9ca2),
            accent: c(0xe8710a),
            accent_text: c(0xffffff),
            accent_soft: ca(0xe8710a24),
            hover: ca(0x1f1f220d),
            selection: ca(0x1a73e840),
            caret: c(0xe8710a),
            danger: c(0xd93025),
            paper_rule: ca(0x1a73e82e),
            popover_bg: c(0xffffff),
            shadow: ca(0x0000002a),
            quote_bar: c(0xd8d6d0),
            code_bg: c(0xf3f2ef),
        }
    }

    pub fn dark() -> Self {
        Theme {
            dark: true,
            app_bg: c(0x141416),
            sidebar_bg: c(0x141416),
            list_bg: c(0x18181b),
            page_bg: c(0x1c1c1f),
            card_bg: c(0x202024),
            card_hover: c(0x26262b),
            card_selected: c(0x352a1f),
            border: c(0x2c2c31),
            text: c(0xe8e8ea),
            text_muted: c(0xa4a4aa),
            text_faint: c(0x6e6e75),
            accent: c(0xffa657),
            accent_text: c(0x1c1c1f),
            accent_soft: ca(0xffa65726),
            hover: ca(0xffffff0f),
            selection: ca(0x79b8ff4a),
            caret: c(0xffa657),
            danger: c(0xff7b72),
            paper_rule: ca(0x79b8ff26),
            popover_bg: c(0x26262b),
            shadow: ca(0x00000080),
            quote_bar: c(0x3a3a40),
            code_bg: c(0x232327),
        }
    }
}

/// `0xAARRGGBB` from the core's palette, as a GPUI colour.
pub fn argb(v: u32) -> Hsla {
    let rgba = Rgba {
        r: ((v >> 16) & 0xff) as f32 / 255.0,
        g: ((v >> 8) & 0xff) as f32 / 255.0,
        b: (v & 0xff) as f32 / 255.0,
        a: ((v >> 24) & 0xff) as f32 / 255.0,
    };
    rgba.into()
}
