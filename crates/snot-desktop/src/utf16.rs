//! Offset conversions. GPUI's text layout counts UTF-8 bytes, the platform
//! input protocol counts UTF-16 units, and snot-core counts chars.

use std::ops::Range;

pub fn offset_to(text: &str, byte: usize) -> usize {
    text[..byte.min(text.len())]
        .chars()
        .map(char::len_utf16)
        .sum()
}

pub fn offset_from(text: &str, utf16: usize) -> usize {
    let mut units = 0;
    for (i, ch) in text.char_indices() {
        if units >= utf16 {
            return i;
        }
        units += ch.len_utf16();
    }
    text.len()
}

pub fn range_to(text: &str, r: &Range<usize>) -> Range<usize> {
    offset_to(text, r.start)..offset_to(text, r.end)
}

pub fn range_from(text: &str, r: &Range<usize>) -> Range<usize> {
    offset_from(text, r.start)..offset_from(text, r.end)
}

/// Byte offset to char count.
pub fn char_at(text: &str, byte: usize) -> u32 {
    text[..byte.min(text.len())].chars().count() as u32
}

/// Char count to byte offset.
pub fn byte_at(text: &str, ch: u32) -> usize {
    text.char_indices()
        .nth(ch as usize)
        .map(|(i, _)| i)
        .unwrap_or(text.len())
}

pub fn chars(text: &str, r: &Range<usize>) -> Range<u32> {
    char_at(text, r.start)..char_at(text, r.end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_across_planes() {
        let s = "a😀é한";
        for (byte, _) in s.char_indices().chain([(s.len(), ' ')]) {
            assert_eq!(offset_from(s, offset_to(s, byte)), byte);
            assert_eq!(byte_at(s, char_at(s, byte)), byte);
        }
        assert_eq!(offset_to(s, s.len()), 5);
    }
}
