use crate::core::{Expr, JSObjectData, JSObjectDataPtr, Value, evaluate_expr, obj_set_value};
use crate::error::JSError;
use crate::unicode::{utf8_to_utf16, utf16_slice, utf16_to_utf8};
use fancy_regex::Regex;

// Best-effort transform to swap greediness of quantifiers: greedy <-> lazy.
fn swap_greed_transform(pattern: &str) -> String {
    // Use regex-syntax to parse the pattern into an AST, then walk the AST
    // and flip greediness on repetition nodes (greedy <-> lazy). This is a
    // robust approach compared to naive string replacement and avoids changing
    // content inside character classes or escapes.
    use regex_syntax::ast::{self, Ast};

    match ast::parse::Parser::new().parse(pattern) {
        Ok(mut ast) => {
            // Walk AST and flip greedy flags below

            let mut flipped = 0usize;
            fn flip_and_count(node: &mut Ast, flipped: &mut usize) {
                use ast::*;
                match node {
                    Ast::Repetition(rep) => {
                        rep.greedy = !rep.greedy;
                        *flipped += 1;
                        flip_and_count(&mut rep.ast, flipped);
                    }
                    Ast::Concat(c) => {
                        for sub in &mut c.asts {
                            flip_and_count(sub, flipped);
                        }
                    }
                    Ast::Alternation(a) => {
                        for sub in &mut a.asts {
                            flip_and_count(sub, flipped);
                        }
                    }
                    Ast::Group(g) => flip_and_count(&mut g.ast, flipped),
                    _ => {}
                }
            }

            flip_and_count(&mut ast, &mut flipped);
            log::debug!("swap_greed_transform: flipped {} repetition nodes", flipped);
            log::trace!("swap_greed_transform ast debug: {:#?}", ast);

            // Convert AST back into a pattern string. If we can't, fall back to
            // returning the original pattern.
            format!("{}", ast)
        }
        Err(_) => pattern.to_string(),
    }
}

// Map a byte offset in the original string to a byte offset in the modified (CRLF-normalized) string.
fn original_to_modified_byte_offset(original: &str, modified: &str, orig_byte_offset: usize) -> usize {
    let mut o_iter = original.chars();
    let mut m_iter = modified.chars();
    let mut o_byte = 0usize;
    let mut m_byte = 0usize;

    while o_byte < orig_byte_offset {
        if let Some(o_ch) = o_iter.next() {
            // if CRLF pair in original, we need to consume both in original but only '\n' in modified
            if o_ch == '\r' {
                // peek next original char
                if let Some(next_o_ch) = o_iter.next() {
                    let o_inc = '\r'.len_utf8() + next_o_ch.len_utf8();
                    o_byte = (o_byte + o_inc).min(orig_byte_offset);
                    // advance modified by a single '\n' (if present)
                    if let Some(m_ch) = m_iter.next() {
                        m_byte += m_ch.len_utf8();
                    }
                } else {
                    o_byte += o_ch.len_utf8();
                    if let Some(m_ch) = m_iter.next() {
                        m_byte += m_ch.len_utf8();
                    }
                }
            } else {
                // normal case: consume one character in both
                o_byte += o_ch.len_utf8();
                if let Some(m_ch) = m_iter.next() {
                    m_byte += m_ch.len_utf8();
                }
            }
        } else {
            break;
        }
    }
    m_byte.min(modified.len())
}

// Map a byte offset in the modified (CRLF-normalized) string back to a byte offset in the original string
fn modified_to_original_byte_offset(original: &str, modified: &str, mod_byte_offset: usize) -> usize {
    let mut o_iter = original.chars();
    let mut m_iter = modified.chars();
    let mut o_byte = 0usize;
    let mut m_byte = 0usize;

    while m_byte < mod_byte_offset {
        if let Some(m_ch) = m_iter.next() {
            m_byte += m_ch.len_utf8();
            // match corresponding original chars; original may have CRLF pair for this single '\n'
            if m_ch == '\n' {
                // check if next two original chars are '\r' '\n'
                if let Some(o_ch) = o_iter.next() {
                    if o_ch == '\r' {
                        if let Some(o_ch2) = o_iter.next() {
                            if o_ch2 == '\n' {
                                o_byte += '\r'.len_utf8() + '\n'.len_utf8();
                                continue;
                            } else {
                                o_byte += o_ch.len_utf8();
                                // push back second original char by placing it at front of iterator is non-trivial; skip
                            }
                        } else {
                            o_byte += o_ch.len_utf8();
                            continue;
                        }
                    } else {
                        o_byte += o_ch.len_utf8();
                    }
                }
            } else if let Some(o_ch) = o_iter.next() {
                o_byte += o_ch.len_utf8();
            }
        } else {
            break;
        }
    }

    o_byte.min(original.len())
}
use std::cell::RefCell;
use std::rc::Rc;

/// Handle RegExp constructor calls
pub(crate) fn handle_regexp_constructor(args: &[Expr], env: &JSObjectDataPtr) -> Result<Value, JSError> {
    let (pattern, flags) = if args.is_empty() {
        // new RegExp() - empty regex
        ("".to_string(), "".to_string())
    } else if args.len() == 1 {
        // new RegExp(pattern)
        let pattern_val = evaluate_expr(env, &args[0])?;
        let pattern = match pattern_val {
            Value::String(s) => utf16_to_utf8(&s),
            Value::Number(n) => n.to_string(),
            Value::Boolean(b) => b.to_string(),
            _ => {
                return Err(raise_type_error!("Invalid RegExp pattern"));
            }
        };
        (pattern, "".to_string())
    } else {
        // new RegExp(pattern, flags)
        let pattern_val = evaluate_expr(env, &args[0])?;
        let flags_val = evaluate_expr(env, &args[1])?;

        let pattern = match pattern_val {
            Value::String(s) => utf16_to_utf8(&s),
            Value::Number(n) => n.to_string(),
            Value::Boolean(b) => b.to_string(),
            _ => {
                return Err(raise_type_error!("Invalid RegExp pattern"));
            }
        };

        let flags = match flags_val {
            Value::String(s) => utf16_to_utf8(&s),
            Value::Number(n) => n.to_string(),
            Value::Boolean(b) => b.to_string(),
            _ => {
                return Err(raise_type_error!("Invalid RegExp flags"));
            }
        };

        (pattern, flags)
    };

    // Build regex with flags
    let regex_pattern = pattern.clone();

    // Parse flags
    let mut global = false;
    let mut ignore_case = false;
    let mut multiline = false;
    let mut dot_matches_new_line = false;
    let mut swap_greed = false;
    let mut unicode = false;
    let mut sticky = false;
    let mut crlf = false;
    let mut case_insensitive = false;

    for flag in flags.chars() {
        match flag {
            'g' => global = true,
            'i' => {
                ignore_case = true;
                case_insensitive = true;
            }
            'm' => multiline = true,
            's' => dot_matches_new_line = true,
            'U' => swap_greed = true,
            'u' => unicode = true,
            'y' => sticky = true,
            'R' => crlf = true,
            _ => {
                return Err(raise_syntax_error!(format!("Invalid RegExp flag: {flag}")));
            }
        }
    }

    // Combine inline flags so fancy-regex can parse features like backreferences
    let mut inline_flags = String::new();
    if case_insensitive {
        inline_flags.push('i');
    }
    if multiline {
        inline_flags.push('m');
    }
    if dot_matches_new_line {
        inline_flags.push('s');
    }
    let effective_pattern = if inline_flags.is_empty() {
        regex_pattern.clone()
    } else {
        format!("(?{}){}", inline_flags, regex_pattern)
    };

    // Validate the regex pattern by trying to compile it via fancy-regex
    if let Err(e) = Regex::new(&effective_pattern) {
        return Err(raise_syntax_error!(format!("Invalid RegExp: {e}")));
    }

    // Create RegExp object
    let regexp_obj = Rc::new(RefCell::new(JSObjectData::new()));

    // Store regex and flags as properties
    obj_set_value(&regexp_obj, &"__regex".into(), Value::String(utf8_to_utf16(&pattern)))?;
    obj_set_value(&regexp_obj, &"__flags".into(), Value::String(utf8_to_utf16(&flags)))?;
    obj_set_value(&regexp_obj, &"__global".into(), Value::Boolean(global))?;
    obj_set_value(&regexp_obj, &"__ignoreCase".into(), Value::Boolean(ignore_case))?;
    obj_set_value(&regexp_obj, &"__multiline".into(), Value::Boolean(multiline))?;
    obj_set_value(&regexp_obj, &"__dotAll".into(), Value::Boolean(dot_matches_new_line))?;
    obj_set_value(&regexp_obj, &"__unicode".into(), Value::Boolean(unicode))?;
    obj_set_value(&regexp_obj, &"__sticky".into(), Value::Boolean(sticky))?;
    obj_set_value(&regexp_obj, &"__swapGreed".into(), Value::Boolean(swap_greed))?;
    obj_set_value(&regexp_obj, &"__crlf".into(), Value::Boolean(crlf))?;
    obj_set_value(&regexp_obj, &"__lastIndex".into(), Value::Number(0.0))?;

    // Add methods
    obj_set_value(&regexp_obj, &"exec".into(), Value::Function("RegExp.prototype.exec".to_string()))?;
    obj_set_value(&regexp_obj, &"test".into(), Value::Function("RegExp.prototype.test".to_string()))?;
    obj_set_value(
        &regexp_obj,
        &"toString".into(),
        Value::Function("RegExp.prototype.toString".to_string()),
    )?;

    Ok(Value::Object(regexp_obj))
}

/// Handle RegExp instance method calls
pub(crate) fn handle_regexp_method(
    obj_map: &JSObjectDataPtr,
    method: &str,
    args: &[Expr],
    env: &JSObjectDataPtr,
) -> Result<Value, JSError> {
    match method {
        "exec" => {
            if args.is_empty() {
                return Err(raise_type_error!("RegExp.prototype.exec requires a string argument"));
            }

            let input_val = evaluate_expr(env, &args[0])?;
            let input = match input_val {
                Value::String(s) => utf16_to_utf8(&s),
                _ => {
                    return Err(raise_type_error!("RegExp.prototype.exec requires a string argument"));
                }
            };

            // Get regex pattern and flags
            let pattern = match crate::core::get_own_property(obj_map, &"__regex".into()) {
                Some(val) => match &*val.borrow() {
                    Value::String(s) => utf16_to_utf8(s),
                    _ => {
                        return Err(raise_type_error!("Invalid regex pattern"));
                    }
                },
                None => {
                    return Err(raise_type_error!("Invalid regex object"));
                }
            };

            let flags = match crate::core::get_own_property(obj_map, &"__flags".into()) {
                Some(val) => match &*val.borrow() {
                    Value::String(s) => utf16_to_utf8(s),
                    _ => "".to_string(),
                },
                None => "".to_string(),
            };

            // Build regex using inline flags so fancy-regex supports backrefs etc.
            let mut inline = String::new();
            if flags.contains('i') {
                inline.push('i');
            }
            if flags.contains('m') {
                inline.push('m');
            }
            if flags.contains('s') {
                inline.push('s');
            }
            // If sticky, anchor pattern to the beginning so it matches only at lastIndex
            let sticky = flags.contains('y');

            // Possibly transform greediness if 'U' flag was set
            let swap_greed = flags.contains('U');
            let crlf = flags.contains('R');

            let mut base_pattern = pattern.clone();
            if swap_greed {
                // Best-effort transform of quantifier greediness
                base_pattern = swap_greed_transform(&base_pattern);
            }

            let eff_pat = if inline.is_empty() {
                if sticky {
                    format!("^{}", base_pattern)
                } else {
                    base_pattern.clone()
                }
            } else if sticky {
                format!("(?{})^{}", inline, base_pattern)
            } else {
                format!("(?{}){}", inline, base_pattern)
            };

            log::debug!("RegExp.exec: effective_pattern='{}'", eff_pat);
            let regex = Regex::new(&eff_pat).map_err(|e| raise_syntax_error!(format!("Invalid RegExp: {e}")))?;

            // Get lastIndex for global regex
            // Per ECMAScript, lastIndex is a UTF-16 code unit index. We'll treat it as such.
            let mut last_index = 0usize; // index in UTF-16 code units
            let global = flags.contains('g');
            let use_last = global || flags.contains('y');
            if use_last
                && let Some(last_index_val) = crate::core::get_own_property(obj_map, &"__lastIndex".into())
                && let Value::Number(n) = &*last_index_val.borrow()
            {
                // Clamp and use as UTF-16 code unit index
                let mut li = *n as isize;
                if li < 0 {
                    li = 0;
                }
                last_index = li as usize;
            }

            // Execute regex
            // Convert last_index (UTF-16 code units) to byte index for slicing
            let byte_start = if last_index == 0 {
                0
            } else {
                // obtain utf16 vector and take prefix length in bytes on the original input
                let u = utf8_to_utf16(&input);
                let clamped = last_index.min(u.len());
                let prefix_bytes = utf16_to_utf8(&utf16_slice(&u, 0, clamped)).len();
                if crlf {
                    // compute corresponding byte index in modified input where CRLF pairs are normalized
                    let working_input = input.replace("\r\n", "\n");
                    original_to_modified_byte_offset(&input, &working_input, prefix_bytes)
                } else {
                    prefix_bytes
                }
            };

            // Use working input when CRLF normalization is enabled
            let working_input = if crlf { input.replace("\r\n", "\n") } else { input.clone() };
            log::debug!(
                "RegExp.exec: flags={} last_index={} byte_start={} working_input_len={}",
                flags,
                last_index,
                byte_start,
                working_input.len()
            );
            match regex.captures(&working_input[byte_start..]) {
                Ok(Some(captures)) => {
                    // Create result array
                    let result_array = Rc::new(RefCell::new(JSObjectData::new()));

                    // Add matched string
                    if let Some(matched) = captures.get(0) {
                        log::debug!(
                            "RegExp.exec: found match start={} end={} matched='{}'",
                            matched.start(),
                            matched.end(),
                            matched.as_str()
                        );
                        // If CRLF normalization was used for matching, map the matched
                        // substring back to the original input so the returned value
                        // reflects the original content (with CRLF sequences).
                        if crlf {
                            let matched_byte_start_in_working = byte_start + matched.start();
                            let matched_byte_end_in_working = byte_start + matched.end();
                            let orig_start = modified_to_original_byte_offset(&input, &working_input, matched_byte_start_in_working);
                            let orig_end = modified_to_original_byte_offset(&input, &working_input, matched_byte_end_in_working);
                            let real_match = if orig_end <= input.len() && orig_start <= orig_end {
                                &input[orig_start..orig_end]
                            } else {
                                matched.as_str()
                            };
                            obj_set_value(&result_array, &"0".into(), Value::String(utf8_to_utf16(real_match)))?;
                        } else {
                            obj_set_value(&result_array, &"0".into(), Value::String(utf8_to_utf16(matched.as_str())))?;
                        }

                        // Compute index in UTF-16 code units: last_index + utf16_len(prefix within match start)
                        let prefix_bytes = &input[byte_start..byte_start + matched.start()];
                        let prefix_u16_len = utf8_to_utf16(prefix_bytes).len();
                        obj_set_value(&result_array, &"index".into(), Value::Number((last_index + prefix_u16_len) as f64))?;
                        obj_set_value(&result_array, &"input".into(), Value::String(utf8_to_utf16(&input)))?;
                    }

                    // Add capture groups
                    let mut group_index = 1;
                    for capture in captures.iter().skip(1) {
                        if let Some(capture_match) = capture {
                            obj_set_value(
                                &result_array,
                                &group_index.to_string().into(),
                                Value::String(utf8_to_utf16(capture_match.as_str())),
                            )?;
                        } else {
                            obj_set_value(&result_array, &group_index.to_string().into(), Value::Undefined)?;
                        }
                        group_index += 1;
                    }

                    // Set length
                    obj_set_value(&result_array, &"length".into(), Value::Number(group_index as f64))?;

                    // Update lastIndex for global or sticky regex
                    if use_last && let Some(matched) = captures.get(0) {
                        log::debug!(
                            "RegExp.exec: updating lastIndex from {} (utf16 units) using matched.as_str()='{}'",
                            last_index,
                            matched.as_str()
                        );
                        // matched.end() is bytes into the sliced substring (working input). We must
                        // map that back to the original string before computing UTF-16 length.
                        let mut matched_str = matched.as_str().to_string();
                        let matched_byte_start_in_working = byte_start + matched.start();
                        let matched_byte_end_in_working = byte_start + matched.end();
                        let orig_start = if crlf {
                            let working_input = input.replace("\r\n", "\n");
                            modified_to_original_byte_offset(&input, &working_input, matched_byte_start_in_working)
                        } else {
                            matched_byte_start_in_working
                        };
                        let orig_end = if crlf {
                            let working_input = input.replace("\r\n", "\n");
                            modified_to_original_byte_offset(&input, &working_input, matched_byte_end_in_working)
                        } else {
                            matched_byte_end_in_working
                        };
                        // Ensure we don't slice out of bounds
                        if orig_end <= input.len() && orig_start <= orig_end {
                            matched_str = input[orig_start..orig_end].to_string();
                        }

                        // Compute new_last_index as the UTF-16 code unit length up to the
                        // end of the match in the original input so lastIndex is an
                        // absolute UTF-16 code unit index.
                        let new_last_index = if orig_end <= input.len() {
                            utf8_to_utf16(&input[..orig_end]).len()
                        } else {
                            // fallback to the previous calculation
                            last_index + utf8_to_utf16(&matched_str).len()
                        };
                        obj_set_value(obj_map, &"__lastIndex".into(), Value::Number(new_last_index as f64))?;
                        log::debug!("RegExp.exec: new __lastIndex={}", new_last_index);
                    }

                    Ok(Value::Object(result_array))
                }
                Ok(None) => {
                    // Reset lastIndex for global regex on no match
                    if global {
                        obj_set_value(obj_map, &"__lastIndex".into(), Value::Number(0.0))?;
                    }
                    // RegExp.exec returns null on no match, but we use Undefined
                    Ok(Value::Undefined)
                }
                Err(e) => Err(raise_syntax_error!(format!("Invalid RegExp: {e}"))),
            }
        }
        "test" => {
            if args.is_empty() {
                return Err(raise_type_error!("RegExp.prototype.test requires a string argument"));
            }

            let input_val = evaluate_expr(env, &args[0])?;
            let input = match input_val {
                Value::String(s) => utf16_to_utf8(&s),
                _ => {
                    return Err(raise_type_error!("RegExp.prototype.test requires a string argument"));
                }
            };

            // Get regex pattern and flags
            let pattern = match crate::core::get_own_property(obj_map, &"__regex".into()) {
                Some(val) => match &*val.borrow() {
                    Value::String(s) => utf16_to_utf8(s),
                    _ => {
                        return Err(raise_type_error!("Invalid regex pattern"));
                    }
                },
                None => {
                    return Err(raise_type_error!("Invalid regex object"));
                }
            };

            let flags = match crate::core::get_own_property(obj_map, &"__flags".into()) {
                Some(val) => match &*val.borrow() {
                    Value::String(s) => utf16_to_utf8(s),
                    _ => "".to_string(),
                },
                None => "".to_string(),
            };

            // Build regex (with inline flags for fancy-regex)
            let mut inline = String::new();
            if flags.contains('i') {
                inline.push('i');
            }
            if flags.contains('m') {
                inline.push('m');
            }
            if flags.contains('s') {
                inline.push('s');
            }
            // Possibly transform greediness and respect sticky/crlf flags
            let swap_greed = flags.contains('U');
            let crlf = flags.contains('R');
            let sticky = flags.contains('y');

            let mut base_pattern = pattern.clone();
            if swap_greed {
                base_pattern = swap_greed_transform(&base_pattern);
            }

            let eff_pat = if inline.is_empty() {
                if sticky {
                    format!("^{}", base_pattern)
                } else {
                    base_pattern.clone()
                }
            } else {
                format!("(?{}){}", inline, base_pattern)
            };
            log::debug!("RegExp.test: effective_pattern='{}'", eff_pat);
            let regex = Regex::new(&eff_pat).map_err(|e| raise_syntax_error!(format!("Invalid RegExp: {e}")))?;

            // Get lastIndex for global regex
            // Per ECMAScript, lastIndex is a UTF-16 code unit index; use if global or sticky.
            let mut last_index = 0usize;
            let global = flags.contains('g');
            let use_last = global || flags.contains('y');
            if use_last
                && let Some(last_index_val) = crate::core::get_own_property(obj_map, &"__lastIndex".into())
                && let Value::Number(n) = &*last_index_val.borrow()
            {
                let mut li = *n as isize;
                if li < 0 {
                    li = 0;
                }
                last_index = li as usize;
            }

            // Test regex
            // compute byte start from UTF-16 last_index
            let byte_start = if last_index == 0 {
                0
            } else {
                let u = utf8_to_utf16(&input);
                let clamped = last_index.min(u.len());
                let prefix_bytes = utf16_to_utf8(&utf16_slice(&u, 0, clamped)).len();
                if crlf {
                    let working_input = input.replace("\r\n", "\n");
                    original_to_modified_byte_offset(&input, &working_input, prefix_bytes)
                } else {
                    prefix_bytes
                }
            };

            let working_input = if crlf { input.replace("\r\n", "\n") } else { input.clone() };
            log::debug!(
                "RegExp.test: flags={} last_index={} byte_start={} working_input_len={}",
                flags,
                last_index,
                byte_start,
                working_input.len()
            );

            let is_match = match regex.is_match(&working_input[byte_start..]) {
                Ok(b) => b,
                Err(e) => {
                    return Err(raise_syntax_error!(format!("Invalid RegExp: {e}")));
                }
            };

            // Update lastIndex for global or sticky regex
            if use_last && is_match {
                log::debug!("RegExp.test: is_match=true; updating lastIndex (global/sticky)");
                match regex.find(&working_input[byte_start..]) {
                    Ok(Some(mat)) => {
                        log::debug!(
                            "RegExp.test: found match start={} end={} in working; mat.as_str()='{}'",
                            mat.start(),
                            mat.end(),
                            mat.as_str()
                        );
                        // matched is relative to working_input; map start/end back to original bytes
                        let matched_byte_start_in_working = byte_start + mat.start();
                        let matched_byte_end_in_working = byte_start + mat.end();
                        let orig_start = if crlf {
                            modified_to_original_byte_offset(&input, &working_input, matched_byte_start_in_working)
                        } else {
                            matched_byte_start_in_working
                        };
                        let orig_end = if crlf {
                            modified_to_original_byte_offset(&input, &working_input, matched_byte_end_in_working)
                        } else {
                            matched_byte_end_in_working
                        };
                        let matched_prefix = if orig_end <= input.len() && orig_start <= orig_end {
                            &input[orig_start..orig_end]
                        } else {
                            mat.as_str()
                        };
                        // Compute new_last_index as absolute UTF-16 code unit count up to match end
                        let new_last_index = if orig_end <= input.len() {
                            utf8_to_utf16(&input[..orig_end]).len()
                        } else {
                            last_index + utf8_to_utf16(matched_prefix).len()
                        };
                        obj_set_value(obj_map, &"__lastIndex".into(), Value::Number(new_last_index as f64))?;
                    }
                    Ok(None) => {}
                    Err(e) => {
                        return Err(raise_syntax_error!(format!("Invalid RegExp: {e}")));
                    }
                }
            } else if global && !is_match {
                obj_set_value(obj_map, &"__lastIndex".into(), Value::Number(0.0))?;
            }

            Ok(Value::Boolean(is_match))
        }
        "toString" => {
            // Get pattern and flags (two-step get to avoid long-lived borrows)
            let pattern = match crate::core::get_own_property(obj_map, &"__regex".into()) {
                Some(val) => match &*val.borrow() {
                    Value::String(s) => utf16_to_utf8(s),
                    _ => "".to_string(),
                },
                None => "".to_string(),
            };

            let flags = match crate::core::get_own_property(obj_map, &"__flags".into()) {
                Some(val) => match &*val.borrow() {
                    Value::String(s) => utf16_to_utf8(s),
                    _ => "".to_string(),
                },
                None => "".to_string(),
            };

            let result = format!("/{}/{}", pattern, flags);
            Ok(Value::String(utf8_to_utf16(&result)))
        }
        _ => Err(raise_eval_error!(format!("RegExp.prototype.{method} is not implemented"))),
    }
}
