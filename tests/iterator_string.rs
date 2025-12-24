use javascript::{Value, evaluate_script, utf16_to_utf8};

#[test]
fn string_iterator_simple() {
    let script = r#"
        let out = [];
        for (let ch of "abc") { out.push(ch); }
        JSON.stringify(out)
    "#;
    let res = evaluate_script(script, None::<&std::path::Path>);
    match res {
        Ok(Value::String(s)) => {
            let s = utf16_to_utf8(&s);
            assert_eq!(s, "[\"a\",\"b\",\"c\"]");
        }
        other => panic!("Expected JSON string result, got {:?}", other),
    }
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
    let res = evaluate_script(script, None::<&std::path::Path>);
    match res {
        Ok(Value::String(s)) => {
            let s = utf16_to_utf8(&s);
            // Expect length 4, then code units [97, 55362, 57271, 98]
            assert!(s.contains("55362"));
            assert!(s.contains("57271"));
            // Also verify that for-of yields a single full codepoint for the non-BMP char
            let v: serde_json::Value = serde_json::from_str(&s).unwrap_or_else(|_| panic!("invalid json: {s}"));
            let arr = v.as_array().expect("expected array");
            assert_eq!(arr[5].as_str().unwrap(), "a");
            assert_eq!(arr[6].as_str().unwrap(), "𠮷");
            assert_eq!(arr[7].as_str().unwrap(), "b");
        }
        other => panic!("Expected JSON string result, got {:?}", other),
    }
}

#[test]
fn string_char_codes_direct() {
    let script = r#"
        (function(){
            let s = "a𠮷b";
            return JSON.stringify([s.length, s.charCodeAt(0), s.charCodeAt(1), s.charCodeAt(2), s.charCodeAt(3)]);
        })()
    "#;
    let res = evaluate_script(script, None::<&std::path::Path>);
    match res {
        Ok(Value::String(s)) => {
            let s = utf16_to_utf8(&s);
            let v: serde_json::Value = serde_json::from_str(&s).unwrap_or_else(|_| panic!("invalid json: {s}"));
            let arr = v.as_array().expect("expected array");
            assert_eq!(arr[0].as_i64().unwrap(), 4);
            assert_eq!(arr[1].as_i64().unwrap(), 97);
            assert_eq!(arr[2].as_i64().unwrap(), 55362);
            assert_eq!(arr[3].as_i64().unwrap(), 57271);
            assert_eq!(arr[4].as_i64().unwrap(), 98);
        }
        other => panic!("Expected JSON string result, got {:?}", other),
    }
}

#[test]
fn string_charcode_single() {
    let script = r#"
        (function(){
            let s = "a𠮷b";
            return s.charCodeAt(1);
        })()
    "#;
    let res = evaluate_script(script, None::<&std::path::Path>);
    match res {
        Ok(Value::Number(n)) => {
            assert_eq!(n as u32, 0xD842);
        }
        other => panic!("Expected number result, got {:?}", other),
    }
}

#[test]
fn string_iterator_combining_mark() {
    let script = r#"
        let s = "y̆";
        let out = [];
        for (let ch of s) { out.push(ch); }
        JSON.stringify(out)
    "#;
    let res = evaluate_script(script, None::<&std::path::Path>);
    match res {
        Ok(Value::String(s)) => {
            let s = utf16_to_utf8(&s);
            let v: serde_json::Value = serde_json::from_str(&s).unwrap_or_else(|_| panic!("invalid json: {s}"));
            let arr = v.as_array().expect("expected array");
            assert_eq!(arr.len(), 2);
            assert_eq!(arr[0].as_str().unwrap(), "y");
            assert_eq!(arr[1].as_str().unwrap(), "\u{0306}");
        }
        other => panic!("Expected JSON string result, got {:?}", other),
    }
}

#[test]
fn string_iterator_flag_regional_indicators() {
    let script = r#"
        let s = "🇨🇦";
        let out = [];
        for (let ch of s) { out.push(ch.length); out.push(ch.charCodeAt(0)); out.push(ch.charCodeAt(1)); }
        JSON.stringify(out)
    "#;
    let res = evaluate_script(script, None::<&std::path::Path>);
    match res {
        Ok(Value::String(s)) => {
            let s = utf16_to_utf8(&s);
            let v: serde_json::Value = serde_json::from_str(&s).unwrap_or_else(|_| panic!("invalid json: {s}"));
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
        other => panic!("Expected JSON string result, got {:?}", other),
    }
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
    let res = evaluate_script(script, None::<&std::path::Path>);
    match res {
        Ok(Value::String(s)) => {
            let s = utf16_to_utf8(&s);
            let v: serde_json::Value = serde_json::from_str(&s).unwrap_or_else(|_| panic!("invalid json: {s}"));
            let arr = v.as_array().expect("expected array");
            // ZWJ sequences are multiple code points; for-of should yield multiple entries
            assert!(arr.len() > 1);
            // None of the entries should be the replacement character
            for item in arr.iter() {
                assert_ne!(item.as_str().unwrap(), "�");
            }
        }
        other => panic!("Expected JSON string result, got {:?}", other),
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
    let res = evaluate_script(script, None::<&std::path::Path>);
    match res {
        Ok(Value::String(s)) => {
            let s = utf16_to_utf8(&s);
            let v: serde_json::Value = serde_json::from_str(&s).unwrap_or_else(|_| panic!("invalid json: {s}"));
            let arr = v.as_array().expect("expected array");
            assert_eq!(arr[0].as_str().unwrap(), "a");
            assert_eq!(arr[1].as_str().unwrap(), "𠮷");
            assert_eq!(arr[2].as_str().unwrap(), "b");
        }
        other => panic!("Expected JSON string result, got {:?}", other),
    }
}
