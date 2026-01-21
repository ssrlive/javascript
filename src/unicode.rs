#![allow(dead_code)]

// Helper functions for UTF-16 string operations
pub fn utf8_to_utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

pub fn utf16_to_utf8(v: &[u16]) -> String {
    String::from_utf16_lossy(v)
}

pub fn utf16_len(v: &[u16]) -> usize {
    v.len()
}

pub fn utf16_slice(v: &[u16], start: usize, end: usize) -> Vec<u16> {
    if start >= v.len() {
        Vec::new()
    } else {
        let end = end.min(v.len());
        v[start..end].to_vec()
    }
}

pub fn utf16_char_at(v: &[u16], index: usize) -> Option<u16> {
    v.get(index).copied()
}

pub fn utf16_to_uppercase(v: &[u16]) -> Vec<u16> {
    let s = utf16_to_utf8(v);
    utf8_to_utf16(&s.to_uppercase())
}

pub fn utf16_to_lowercase(v: &[u16]) -> Vec<u16> {
    let s = utf16_to_utf8(v);
    utf8_to_utf16(&s.to_lowercase())
}

pub fn utf16_find(v: &[u16], pattern: &[u16]) -> Option<usize> {
    if pattern.is_empty() {
        return Some(0);
    }
    if pattern.len() > v.len() {
        return None;
    }
    (0..=v.len() - pattern.len()).find(|&i| v[i..i + pattern.len()] == *pattern)
}

pub fn utf16_rfind(v: &[u16], pattern: &[u16]) -> Option<usize> {
    if pattern.is_empty() {
        return Some(v.len());
    }
    if pattern.len() > v.len() {
        return None;
    }
    (0..=v.len() - pattern.len()).rev().find(|&i| v[i..i + pattern.len()] == *pattern)
}

pub fn utf16_replace(v: &[u16], search: &[u16], replace: &[u16]) -> Vec<u16> {
    if let Some(pos) = utf16_find(v, search) {
        let mut result = v[..pos].to_vec();
        result.extend_from_slice(replace);
        result.extend_from_slice(&v[pos + search.len()..]);
        result
    } else {
        v.to_vec()
    }
}

// Grandfathered Other_ID_Start codepoints required by ECMAScript
pub(crate) const GRANDFATHERED_OTHER_ID_START: [char; 6] = [
    '\u{2118}', // ℘
    '\u{212E}', // ℮
    '\u{309B}', // ゛
    '\u{309C}', // ゜
    '\u{1885}', // ᢅ
    '\u{1886}', // ᢆ
];

// Additional ID_Continue codepoints required to satisfy Test262 Unicode v17 cases
// Plus fallback ranges derived from Test262 `part-unicode-16.0.0.js` for ID_Continue
pub(crate) const OTHER_ID_CONTINUE: [char; 52] = [
    '\u{1acf}',
    '\u{1ad0}',
    '\u{1ad1}',
    '\u{1ad2}',
    '\u{1ad3}',
    '\u{1ad4}',
    '\u{1ad5}',
    '\u{1ad6}',
    '\u{1ad7}',
    '\u{1ad8}',
    '\u{1ad9}',
    '\u{1ada}',
    '\u{1adb}',
    '\u{1adc}',
    '\u{1add}',
    '\u{1ae0}',
    '\u{1ae1}',
    '\u{1ae2}',
    '\u{1ae3}',
    '\u{1ae4}',
    '\u{1ae5}',
    '\u{1ae6}',
    '\u{1ae7}',
    '\u{1ae8}',
    '\u{1ae9}',
    '\u{1aea}',
    '\u{1aeb}',
    '\u{10efa}',
    '\u{10efb}',
    '\u{11b60}',
    '\u{11b61}',
    '\u{11b62}',
    '\u{11b63}',
    '\u{11b64}',
    '\u{11b65}',
    '\u{11b66}',
    '\u{11b67}',
    '\u{11de0}',
    '\u{11de1}',
    '\u{11de2}',
    '\u{11de3}',
    '\u{11de4}',
    '\u{11de5}',
    '\u{11de6}',
    '\u{11de7}',
    '\u{11de8}',
    '\u{11de9}',
    '\u{1e6e3}',
    '\u{1e6e6}',
    '\u{1e6ee}',
    '\u{1e6ef}',
    '\u{1e6f5}',
];

// Compact ranges for ID_Continue derived from Test262 `part-unicode-16.0.0.js` (coalesced)
pub(crate) const ADDITIONAL_OTHER_ID_CONTINUE_RANGES: &[(u32, u32)] = &[
    (0x0897, 0x0897),
    (0x10D40, 0x10D49),
    (0x10D69, 0x10D6D),
    (0x10EFC, 0x10EFC),
    (0x113B8, 0x113C0),
    (0x113C2, 0x113C2),
    (0x113C5, 0x113C5),
    (0x113C7, 0x113CA),
    (0x113CC, 0x113D0),
    (0x113D2, 0x113D2),
    (0x113E1, 0x113E2),
    (0x116D0, 0x116E3),
    (0x11BF0, 0x11BF9),
    (0x11F5A, 0x11F5A),
    (0x1611E, 0x16139),
    (0x16D70, 0x16D79),
    (0x1CCF0, 0x1CCF9),
    (0x1E5EE, 0x1E5EF),
    (0x1E5F1, 0x1E5FA),
];

pub(crate) const ADDITIONAL_OTHER_ID_START_RANGES: [(u32, u32); 44] = [
    (0x88F, 0x88F),
    (0xC5C, 0xC5C),
    (0xCDC, 0xCDC),
    (0x1C89, 0x1C8A),
    (0xA7CB, 0xA7CF),
    (0xA7D2, 0xA7D2),
    (0xA7D4, 0xA7D4),
    (0xA7DA, 0xA7DC),
    (0xA7F1, 0xA7F1),
    (0x105C0, 0x105F3),
    (0x10940, 0x10959),
    (0x10D4A, 0x10D65),
    (0x10D6F, 0x10D85),
    (0x10EC2, 0x10EC7),
    (0x11380, 0x11389),
    (0x1138B, 0x1138B),
    (0x1138E, 0x1138E),
    (0x11390, 0x113B5),
    (0x113B7, 0x113B7),
    (0x113D1, 0x113D1),
    (0x113D3, 0x113D3),
    (0x11BC0, 0x11BE0),
    (0x11DB0, 0x11DDB),
    (0x13460, 0x143FA),
    (0x16100, 0x1611D),
    (0x16D40, 0x16D6C),
    (0x16EA0, 0x16EB8),
    (0x16EBB, 0x16ED3),
    (0x16FF2, 0x16FF6),
    (0x187F8, 0x187FF),
    (0x18CFF, 0x18CFF),
    (0x18D09, 0x18D1E),
    (0x18D80, 0x18DF2),
    (0x1E5D0, 0x1E5ED),
    (0x1E5F0, 0x1E5F0),
    (0x1E6C0, 0x1E6DE),
    (0x1E6E0, 0x1E6E2),
    (0x1E6E4, 0x1E6E5),
    (0x1E6E7, 0x1E6ED),
    (0x1E6F0, 0x1E6F4),
    (0x1E6FE, 0x1E6FF),
    (0x2B73A, 0x2B73F),
    (0x2CEA2, 0x2CEAD),
    (0x323B0, 0x33479),
];
