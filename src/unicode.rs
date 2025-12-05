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
