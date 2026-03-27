use regress::Regex;
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    static REGEX_CACHE : RefCell < HashMap < (Vec < u16 >, String), Regex >> = RefCell::new(HashMap::new());
}

thread_local! {
    #[doc = " Manually set input via `RegExp.input = val` (the [[RegExpInput]] slot"]
    #[doc = " that can be written by user code)."]
    static LEGACY_REGEXP_INPUT_OVERRIDE : RefCell< Option < Vec < u16 >>> = const { RefCell::new(None) };
}

/// Compile a regex, returning a cached copy when the same pattern+flags
/// have been compiled before.
pub(crate) fn get_or_compile_regex(pattern: &[u16], flags: &str) -> Result<Regex, String> {
    REGEX_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        let key = (pattern.to_vec(), flags.to_string());
        if let Some(re) = cache.get(&key) {
            return Ok(re.clone());
        }
        let re = create_regex_from_utf16(pattern, flags)?;
        cache.insert(key, re.clone());
        Ok(re)
    })
}

pub fn create_regex_from_utf16(pattern: &[u16], flags: &str) -> Result<Regex, String> {
    if flags.contains('u') || flags.contains('v') {
        let it = std::char::decode_utf16(pattern.iter().cloned()).map(|r| match r {
            Ok(c) => c as u32,
            Err(e) => e.unpaired_surrogate() as u32,
        });
        Regex::from_unicode(it, flags).map_err(|e| e.to_string())
    } else {
        let processed = preprocess_pattern_non_unicode(pattern);
        Regex::from_unicode(processed.into_iter(), flags).map_err(|e| e.to_string())
    }
}
/// For non-unicode regex patterns, pass raw UTF-16 code units to regress so
/// that supplementary characters are matched as two separate code units (via
/// `find_from_ucs2`).  However, named capture group identifiers (`(?<name>`)
/// and named backreferences (`\k<name>`) require valid Unicode identifier
/// characters, so surrogate pairs inside those contexts are decoded into full
/// code points.
fn preprocess_pattern_non_unicode(pattern: &[u16]) -> Vec<u32> {
    let mut result = Vec::with_capacity(pattern.len());
    let mut i = 0;
    let len = pattern.len();
    while i < len {
        if i + 3 <= len && pattern[i] == b'(' as u16 && pattern[i + 1] == b'?' as u16 && pattern[i + 2] == b'<' as u16 {
            if i + 3 < len && (pattern[i + 3] == b'=' as u16 || pattern[i + 3] == b'!' as u16) {
                result.push(pattern[i] as u32);
                i += 1;
                continue;
            }
            result.push(b'(' as u32);
            result.push(b'?' as u32);
            result.push(b'<' as u32);
            i += 3;
            while i < len && pattern[i] != b'>' as u16 {
                if i + 1 < len && (0xD800..=0xDBFF).contains(&pattern[i]) && (0xDC00..=0xDFFF).contains(&pattern[i + 1]) {
                    let hi = pattern[i] as u32;
                    let lo = pattern[i + 1] as u32;
                    result.push(0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00));
                    i += 2;
                } else {
                    result.push(pattern[i] as u32);
                    i += 1;
                }
            }
            if i < len {
                result.push(pattern[i] as u32);
                i += 1;
            }
            continue;
        }
        if i + 3 <= len && pattern[i] == b'\\' as u16 && pattern[i + 1] == b'k' as u16 && pattern[i + 2] == b'<' as u16 {
            result.push(b'\\' as u32);
            result.push(b'k' as u32);
            result.push(b'<' as u32);
            i += 3;
            while i < len && pattern[i] != b'>' as u16 {
                if i + 1 < len && (0xD800..=0xDBFF).contains(&pattern[i]) && (0xDC00..=0xDFFF).contains(&pattern[i + 1]) {
                    let hi = pattern[i] as u32;
                    let lo = pattern[i + 1] as u32;
                    result.push(0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00));
                    i += 2;
                } else {
                    result.push(pattern[i] as u32);
                    i += 1;
                }
            }
            if i < len {
                result.push(pattern[i] as u32);
                i += 1;
            }
            continue;
        }
        if pattern[i] == b'\\' as u16 && i + 1 < len {
            result.push(pattern[i] as u32);
            result.push(pattern[i + 1] as u32);
            i += 2;
            continue;
        }
        result.push(pattern[i] as u32);
        i += 1;
    }
    result
}
