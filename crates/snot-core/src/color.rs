//! The shared palette.
//!
//! Notes store colours as tokens ("red", "ink") rather than raw values, so
//! each one can resolve differently in light and dark mode: black ink
//! written at night is still visible the next morning, and a pastel note
//! background does not glare in a dark room. A `#rrggbb` value is accepted
//! anywhere a token is and is used as given.
//!
//! Every function returns `0xAARRGGBB`.

/// Tokens offered in pickers, in display order.
pub const TOKENS: [&str; 10] = [
    "ink", "gray", "red", "orange", "yellow", "green", "teal", "blue", "purple", "pink",
];

/// Highlighter colours offered in pickers.
pub const HIGHLIGHTS: [&str; 5] = ["yellow", "green", "blue", "pink", "orange"];

/// Note background colours offered in pickers; `None` is the plain page.
pub const NOTE_COLORS: [&str; 7] = ["yellow", "orange", "red", "pink", "purple", "blue", "green"];

struct Entry {
    token: &'static str,
    /// Text and ink on a light page, then on a dark one.
    fg: (u32, u32),
    /// Note background on a light page, then on a dark one.
    bg: (u32, u32),
}

const PALETTE: [Entry; 10] = [
    Entry {
        token: "ink",
        fg: (0x1f1f22, 0xe8e8ea),
        bg: (0xffffff, 0x1c1c1f),
    },
    Entry {
        token: "gray",
        fg: (0x7a7a80, 0xa4a4aa),
        bg: (0xf0f0f2, 0x2a2a2e),
    },
    Entry {
        token: "red",
        fg: (0xd93025, 0xff7b72),
        bg: (0xfde2df, 0x3d2220),
    },
    Entry {
        token: "orange",
        fg: (0xe8710a, 0xffa657),
        bg: (0xffe8cf, 0x3d2c1c),
    },
    Entry {
        token: "yellow",
        fg: (0xc9a400, 0xf2cc60),
        bg: (0xfff6c7, 0x37321b),
    },
    Entry {
        token: "green",
        fg: (0x188038, 0x56d364),
        bg: (0xdff3e1, 0x1e3423),
    },
    Entry {
        token: "teal",
        fg: (0x00897b, 0x39c5bb),
        bg: (0xd7f2ef, 0x1b3432),
    },
    Entry {
        token: "blue",
        fg: (0x1a73e8, 0x79b8ff),
        bg: (0xdde9fb, 0x1d2a3d),
    },
    Entry {
        token: "purple",
        fg: (0x8430ce, 0xd2a8ff),
        bg: (0xeee2fb, 0x2e2240),
    },
    Entry {
        token: "pink",
        fg: (0xd01884, 0xff8ccb),
        bg: (0xfde1ef, 0x3b2030),
    },
];

fn entry(token: &str) -> Option<&'static Entry> {
    PALETTE.iter().find(|e| e.token == token)
}

fn hex(token: &str) -> Option<u32> {
    let h = token.strip_prefix('#')?;
    if h.len() != 6 {
        return None;
    }
    u32::from_str_radix(h, 16).ok()
}

fn opaque(rgb: u32) -> u32 {
    0xff00_0000 | (rgb & 0xff_ffff)
}

/// Colour for text and ink. Unknown tokens fall back to the default ink.
pub fn ink(token: &str, dark: bool) -> u32 {
    if let Some(rgb) = hex(token) {
        return opaque(rgb);
    }
    let e = entry(token).unwrap_or(&PALETTE[0]);
    opaque(if dark { e.fg.1 } else { e.fg.0 })
}

/// A text highlight: the token's colour, translucent so the text shows
/// through in either theme.
pub fn highlight(token: &str, dark: bool) -> u32 {
    let rgb = hex(token).unwrap_or_else(|| {
        let e = entry(token).unwrap_or(&PALETTE[4]);
        if dark { e.fg.1 } else { e.fg.0 }
    });
    (if dark { 0x5c00_0000 } else { 0x4c00_0000 }) | (rgb & 0xff_ffff)
}

/// A note's page colour; `None` is the plain page.
pub fn note_background(token: Option<&str>, dark: bool) -> u32 {
    let e = token.and_then(entry).unwrap_or(&PALETTE[0]);
    if let Some(rgb) = token.and_then(hex) {
        return opaque(rgb);
    }
    opaque(if dark { e.bg.1 } else { e.bg.0 })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ink_flips_with_the_theme() {
        assert_ne!(ink("ink", false), ink("ink", true));
        assert_eq!(ink("nonsense", false), ink("ink", false));
        assert_eq!(ink("#123456", true), 0xff123456);
    }

    #[test]
    fn every_offered_token_resolves() {
        for t in TOKENS.iter().chain(&HIGHLIGHTS).chain(&NOTE_COLORS) {
            assert!(entry(t).is_some(), "{t}");
        }
    }
}
