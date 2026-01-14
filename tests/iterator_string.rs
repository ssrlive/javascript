use javascript::evaluate_script;

fn fix_rust_escapes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(&'u') = chars.peek() {
                chars.next(); // eat 'u'
                if let Some(&'{') = chars.peek() {
                    chars.next(); // eat '{'
                    let mut hex = String::new();
                    while let Some(&c2) = chars.peek() {
                        if c2 == '}' {
                            chars.next();
                            break;
                        }
                        hex.push(chars.next().expect("unexpected end"));
                    }
                    if let Ok(u) = u32::from_str_radix(&hex, 16)
                        && let Some(ch) = std::char::from_u32(u)
                    {
                        out.push(ch);
                        continue;
                    }
                    out.push_str(&format!("\\u{{{}}}", hex));
                } else {
                    out.push_str("\\u");
                }
            } else {
                out.push('\\');
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn parse_js_json_result(s: &str) -> serde_json::Value {
    // evaluate_script returns a Debug-formatted string of the JS result.
    // E.g. JS returns '["a"]', Rust receives "\"[\"a\"]\""
    // We first decode the outer string to get the inner JS string content.
    let inner_json: String =
        serde_json::from_str(s).unwrap_or_else(|_| s.trim_matches('"').to_string().replace("\\\"", "\"").replace("\\\\", "\\"));

    // Rust's Debug format uses \u{...} for some unicode characters.
    // serde_json does not support this extension. We must unescape it manually.
    let fixed_json = fix_rust_escapes(&inner_json);

    // Now parse the inner JSON string
    serde_json::from_str(&fixed_json).unwrap_or_else(|_| panic!("invalid inner json: {inner_json}"))
}

#[test]
fn string_iterator_simple() {
    let script = r#"
        let out = [];
        for (let ch of "abc") { out.push(ch); }
        JSON.stringify(out)
    "#;
    let res = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(res, "\"[\\\"a\\\",\\\"b\\\",\\\"c\\\"]\"");
}

#[test]
fn string_iterator_unicode() {
    let script = r#"
        let s = "a𠮷b";
        console.log(s);
        let out = [];
        out.push(s.length);
        out.push(s.charCodeAt(0)); out.push(s.charCodeAt(1)); out.push(s.charCodeAt(2)); out.push(s.charCodeAt(3));
        for (let ch of s) { out.push(ch); }
        JSON.stringify(out)
    "#;
    let res = evaluate_script(script, None::<&std::path::Path>).unwrap();
    println!("DEBUG: string_iterator_unicode result: {}", res);
    assert_eq!(res, "\"[4,97,55362,57271,98,\\\"a\\\",\\\"𠮷\\\",\\\"b\\\"]\"");
}

#[test]
fn string_char_codes_direct() {
    let script = r#"
        (function(){
            let s = "a𠮷b";
            return JSON.stringify([s.length, s.charCodeAt(0), s.charCodeAt(1), s.charCodeAt(2), s.charCodeAt(3)]);
        })()
    "#;
    let res = evaluate_script(script, None::<&std::path::Path>).unwrap();
    let v = parse_js_json_result(&res);
    let arr = v.as_array().expect("expected array");
    assert_eq!(arr[0].as_i64().unwrap(), 4);
    assert_eq!(arr[1].as_i64().unwrap(), 97);
    assert_eq!(arr[2].as_i64().unwrap(), 55362);
    assert_eq!(arr[3].as_i64().unwrap(), 57271);
    assert_eq!(arr[4].as_i64().unwrap(), 98);
}

#[test]
fn string_charcode_single() {
    let script = r#"
        (function(){
            let s = "a𠮷b";
            return s.charCodeAt(1);
        })()
    "#;
    let res = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(res, "55362");
}

#[test]
fn string_iterator_combining_mark() {
    let script = r#"
        let s = "y̆";
        let out = [];
        for (let ch of s) { out.push(ch); }
        JSON.stringify(out)
    "#;
    let res = evaluate_script(script, None::<&std::path::Path>).unwrap();
    assert_eq!(res, "\"[\\\"y\\\",\\\"\u{306}\\\"]\"");
}

#[test]
fn string_iterator_flag_regional_indicators() {
    let script = r#"
        let s = "🇨🇦";
        let out = [];
        for (let ch of s) { out.push(ch.length); out.push(ch.charCodeAt(0)); out.push(ch.charCodeAt(1)); }
        JSON.stringify(out)
    "#;
    let s = evaluate_script(script, None::<&std::path::Path>).unwrap();
    let s = s.trim_start_matches('"').trim_end_matches('"');
    let v = parse_js_json_result(s);
    let arr = v.as_array().expect("expected array");
    // Expect two regional-indicator characters, each length 2 (surrogate pair)
    assert_eq!(arr[0].as_i64().unwrap(), 2);
    assert_eq!(arr[3].as_i64().unwrap(), 2);
    // high surrogates in positions 1 and 4 should be in 0xD800..=0xDBFF
    let h1 = arr[1].as_i64().unwrap() as u32;
    let l1 = arr[2].as_i64().unwrap() as u32;
    let h2 = arr[4].as_i64().unwrap() as u32;
    let l2 = arr[5].as_i64().unwrap() as u32;
    assert!((0xD800..=0xDBFF).contains(&h1));
    assert!((0xDC00..=0xDFFF).contains(&l1));
    assert!((0xD800..=0xDBFF).contains(&h2));
    assert!((0xDC00..=0xDFFF).contains(&l2));
}

#[test]
fn string_iterator_zwj_sequence() {
    // Family emoji with ZWJ joiners: multiple code points joined visually
    let script = r#"
        let s = "👩‍👩‍👧‍👦";
        let out = [];
        for (let ch of s) { out.push(ch); }
        JSON.stringify(out)
    "#;
    let s = evaluate_script(script, None::<&std::path::Path>).unwrap();
    println!("DEBUG: string_iterator_zwj_sequence result: {}", s);
    let s = s.trim_start_matches('"').trim_end_matches('"');
    let v = parse_js_json_result(s);
    let arr = v.as_array().expect("expected array");
    // ZWJ sequences are multiple code points; for-of should yield multiple entries
    assert!(arr.len() > 1);
    // None of the entries should be the replacement character
    for item in arr.iter() {
        assert_ne!(item.as_str().unwrap(), "�");
    }
}

#[test]
fn string_object_iterator_behaves_same() {
    let script = r#"
        let s = new String("a𠮷b");
        let out = [];
        for (let ch of s) { out.push(ch); }
        JSON.stringify(out)
    "#;
    let s = evaluate_script(script, None::<&std::path::Path>).unwrap();
    let s = s.trim_start_matches('"').trim_end_matches('"');
    let v = parse_js_json_result(s);
    let arr = v.as_array().expect("expected array");
    assert_eq!(arr[0].as_str().unwrap(), "a");
    assert_eq!(arr[1].as_str().unwrap(), "𠮷");
    assert_eq!(arr[2].as_str().unwrap(), "b");
}
