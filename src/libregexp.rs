#![allow(dead_code)]
#![allow(non_snake_case)]

pub const LRE_FLAG_GLOBAL: i32 = 1 << 0;
pub const LRE_FLAG_IGNORECASE: i32 = 1 << 1;
pub const LRE_FLAG_MULTILINE: i32 = 1 << 2;
pub const LRE_FLAG_DOTALL: i32 = 1 << 3;
pub const LRE_FLAG_UNICODE: i32 = 1 << 4;
pub const LRE_FLAG_STICKY: i32 = 1 << 5;
pub const LRE_FLAG_INDICES: i32 = 1 << 6;
pub const LRE_FLAG_NAMED_GROUPS: i32 = 1 << 7;
pub const LRE_FLAG_UNICODE_SETS: i32 = 1 << 8;

pub const LRE_RET_MEMORY_ERROR: i32 = -1;
pub const LRE_RET_TIMEOUT: i32 = -2;

use crate::cutils::*;
use crate::libunicode::*;

pub fn lre_compile(
    plen: &mut i32,
    error_msg: &mut [u8],
    buf: &[u8],
    re_flags: i32,
    opaque: *mut std::ffi::c_void,
) -> Option<Vec<u8>> {
    let mut s = REParseState::new(buf, re_flags, opaque);

    // Header placeholder
    for _ in 0..8 {
        s.byte_code.putc(0);
    }

    if re_parse_out(&mut s, false) != 0 {
        let bytes = s.error_msg.as_bytes();
        let len = std::cmp::min(bytes.len(), error_msg.len() - 1);
        error_msg[..len].copy_from_slice(&bytes[..len]);
        error_msg[len] = 0;
        return None;
    }

    if s.total_capture_count < 0 {
        s.total_capture_count = s.capture_count;
    }
    if s.has_named_captures < 0 {
        s.has_named_captures = 0;
    }

    // Fill header
    s.byte_code.buf[0] = s.re_flags as u8;
    s.byte_code.buf[1] = s.capture_count as u8;
    s.byte_code.buf[2] = s.total_capture_count as u8;
    s.byte_code.buf[3] = s.has_named_captures as u8;

    // Group names
    if s.group_names.len() > 0 {
        let offset = s.byte_code.len() as u32;
        s.byte_code.put(&s.group_names.buf);
        s.byte_code.buf[4..8].copy_from_slice(&offset.to_ne_bytes());
    } else {
        s.byte_code.buf[4..8].copy_from_slice(&0u32.to_ne_bytes());
    }

    *plen = s.byte_code.len() as i32;
    Some(s.byte_code.buf)
}

fn re_parse_out(s: &mut REParseState, is_backward: bool) -> i32 {
    if re_parse_disjunction(s, is_backward) != 0 {
        return -1;
    }
    s.emit_op(REOPCode::Match as u8);
    0
}

fn re_parse_disjunction(s: &mut REParseState, is_backward: bool) -> i32 {
    let start = s.byte_code.len();
    if re_parse_alternative(s, is_backward) != 0 {
        return -1;
    }

    while let Some(c) = s.peek() {
        if c == b'|' {
            s.next_u8();

            let len = s.byte_code.len() - start;

            let mut buf = [0u8; 5];
            buf[0] = REOPCode::SplitNextFirst as u8;
            let offset = (len + 5) as u32;
            buf[1..5].copy_from_slice(&offset.to_ne_bytes());

            if s.byte_code.insert(start, &buf) != 0 {
                return -1;
            }

            let pos = s.emit_goto(REOPCode::Goto as u8, 0);

            if re_parse_alternative(s, is_backward) != 0 {
                return -1;
            }

            let len = s.byte_code.len() - (pos + 4);
            let val = (len as u32).to_ne_bytes();
            s.byte_code.buf[pos..pos + 4].copy_from_slice(&val);
        } else {
            break;
        }
    }
    0
}

fn re_parse_alternative(s: &mut REParseState, is_backward: bool) -> i32 {
    loop {
        if let Some(c) = s.peek() {
            if c == b'|' || c == b')' {
                break;
            }
        } else {
            break;
        }

        if re_parse_term(s, is_backward) != 0 {
            return -1;
        }
    }
    0
}

fn get_class_atom(s: &mut REParseState, cr: Option<&mut REStringList>, inclass: bool) -> i32 {
    let mut p = s.buf_pos;
    if p >= s.buf.len() {
        return -1;
    }

    let mut c = s.buf[p] as u32;

    match c as u8 {
        b'\\' => {
            p += 1;
            if p >= s.buf.len() {
                return re_parse_error(s, "unexpected end");
            }
            let c2 = s.buf[p] as u32;
            p += 1;

            match c2 as u8 {
                b'd' => {
                    if let Some(cr) = cr {
                        if cr_init_char_range(s, cr, CLASS_RANGE_BASE + 0) != 0 {
                            return -1;
                        }
                    }
                    s.buf_pos = p;
                    return CLASS_RANGE_BASE as i32;
                }
                b'D' => {
                    if let Some(cr) = cr {
                        if cr_init_char_range(s, cr, CLASS_RANGE_BASE + 1) != 0 {
                            return -1;
                        }
                    }
                    s.buf_pos = p;
                    return CLASS_RANGE_BASE as i32;
                }
                b's' => {
                    if let Some(cr) = cr {
                        if cr_init_char_range(s, cr, CLASS_RANGE_BASE + 2) != 0 {
                            return -1;
                        }
                    }
                    s.buf_pos = p;
                    return CLASS_RANGE_BASE as i32;
                }
                b'S' => {
                    if let Some(cr) = cr {
                        if cr_init_char_range(s, cr, CLASS_RANGE_BASE + 3) != 0 {
                            return -1;
                        }
                    }
                    s.buf_pos = p;
                    return CLASS_RANGE_BASE as i32;
                }
                b'w' => {
                    if let Some(cr) = cr {
                        if cr_init_char_range(s, cr, CLASS_RANGE_BASE + 4) != 0 {
                            return -1;
                        }
                    }
                    s.buf_pos = p;
                    return CLASS_RANGE_BASE as i32;
                }
                b'W' => {
                    if let Some(cr) = cr {
                        if cr_init_char_range(s, cr, CLASS_RANGE_BASE + 5) != 0 {
                            return -1;
                        }
                    }
                    s.buf_pos = p;
                    return CLASS_RANGE_BASE as i32;
                }
                b'c' => {
                    if p < s.buf.len() {
                        let c3 = s.buf[p] as u32;
                        if (c3 >= b'a' as u32 && c3 <= b'z' as u32)
                            || (c3 >= b'A' as u32 && c3 <= b'Z' as u32)
                            || (((c3 >= b'0' as u32 && c3 <= b'9' as u32) || c3 == b'_' as u32)
                                && inclass
                                && !s.is_unicode)
                        {
                            c = c3 & 0x1f;
                            p += 1;
                        } else if s.is_unicode {
                            return re_parse_error(s, "invalid escape sequence");
                        } else {
                            p -= 1;
                            c = b'\\' as u32;
                        }
                    } else {
                        p -= 1;
                        c = b'\\' as u32;
                    }
                }
                _ => {
                    p -= 1;
                    let mut p_mut = p;
                    let ret = lre_parse_escape(&mut p_mut, s.buf, if s.is_unicode { 2 } else { 0 });
                    p = p_mut;
                    if ret >= 0 {
                        c = ret as u32;
                    } else {
                        if s.is_unicode {
                            return re_parse_error(s, "invalid escape sequence");
                        } else {
                            c = c2;
                            p += 1;
                        }
                    }
                }
            }
        }
        _ => {
            if c >= 128 {
                let mut offset: usize = 0;
                c = crate::libunicode::unicode_from_utf8(&s.buf[p..], 4, &mut offset);
                p += offset;

                if c > 0xffff && !s.is_unicode {
                    return re_parse_error(s, "malformed unicode char");
                }
            } else {
                p += 1;
            }
        }
    }

    if let Some(cr) = cr {
        if cr_init_char_range(s, cr, c) != 0 {
            return -1;
        }
    }

    s.buf_pos = p;
    c as i32
}

fn parse_class_atom(
    s: &mut REParseState,
    is_backward: bool,
    last_atom_start: &mut isize,
    last_capture_count: &mut i32,
) -> i32 {
    let start = s.byte_code.len() as isize;
    *last_capture_count = s.capture_count;
    if is_backward {
        s.emit_op(REOPCode::Prev as u8);
    }

    let c_val = get_class_atom(s, None, false);
    if c_val < 0 {
        return -1;
    }

    if c_val >= CLASS_RANGE_BASE as i32 {
        let mut cr = REStringList::new();
        if cr_init_char_range(s, &mut cr, c_val as u32) != 0 {
            return -1;
        }
        if re_emit_range(s, &cr.cr) != 0 {
            return -1;
        }
    } else {
        let mut c_final = c_val as u32;
        if s.ignore_case {
            c_final = lre_canonicalize(c_final, s.is_unicode);
        }
        re_emit_char(s, c_final);
    }

    if is_backward {
        s.emit_op(REOPCode::Prev as u8);
    }
    *last_atom_start = start;
    0
}

fn re_parse_term(s: &mut REParseState, is_backward: bool) -> i32 {
    let mut last_atom_start = -1isize;
    let mut last_capture_count = 0;

    if s.buf_pos >= s.buf.len() {
        return -1;
    }

    let c = s.peek().unwrap();

    match c {
        b'^' => {
            s.next_u8();
            let op = if s.multi_line {
                REOPCode::LineStartM
            } else {
                REOPCode::LineStart
            };
            s.emit_op(op as u8);
        }
        b'$' => {
            s.next_u8();
            let op = if s.multi_line {
                REOPCode::LineEndM
            } else {
                REOPCode::LineEnd
            };
            s.emit_op(op as u8);
        }
        b'.' => {
            s.next_u8();
            last_atom_start = s.byte_code.len() as isize;
            last_capture_count = s.capture_count;
            if is_backward {
                s.emit_op(REOPCode::Prev as u8);
            }
            let op = if s.dotall {
                REOPCode::Any
            } else {
                REOPCode::Dot
            };
            s.emit_op(op as u8);
            if is_backward {
                s.emit_op(REOPCode::Prev as u8);
            }
        }
        b'{' => {
            if s.is_unicode {
                return re_parse_error(s, "syntax error");
            }
            let p = s.buf_pos + 1;
            if p < s.buf.len() {
                let c2 = s.buf[p];
                if !(c2 >= b'0' && c2 <= b'9') {
                    return parse_class_atom(
                        s,
                        is_backward,
                        &mut last_atom_start,
                        &mut last_capture_count,
                    );
                }
            }
            return re_parse_error(s, "nothing to repeat");
        }
        b'*' | b'+' | b'?' => {
            return re_parse_error(s, "nothing to repeat");
        }
        b'(' => {
            s.next_u8();
            if s.peek() == Some(b'?') {
                s.next_u8();
                if s.peek() == Some(b':') {
                    s.next_u8();
                    last_atom_start = s.byte_code.len() as isize;
                    last_capture_count = s.capture_count;
                    if re_parse_disjunction(s, is_backward) < 0 {
                        return -1;
                    }
                    if s.next_u8() != Some(b')') {
                        return re_parse_error(s, "expecting ')'");
                    }
                } else if s.peek() == Some(b'=') || s.peek() == Some(b'!') {
                    let is_neg = s.peek() == Some(b'!');
                    s.next_u8();

                    if !s.is_unicode {
                        last_atom_start = s.byte_code.len() as isize;
                        last_capture_count = s.capture_count;
                    }

                    let op = if is_neg {
                        REOPCode::NegativeLookahead
                    } else {
                        REOPCode::Lookahead
                    };
                    let pos = s.emit_op_u32(op as u8, 0);

                    if re_parse_disjunction(s, false) < 0 {
                        return -1;
                    }
                    if s.next_u8() != Some(b')') {
                        return re_parse_error(s, "expecting ')'");
                    }

                    let op_match = if is_neg {
                        REOPCode::NegativeLookaheadMatch
                    } else {
                        REOPCode::LookaheadMatch
                    };
                    s.emit_op(op_match as u8);

                    let len = s.byte_code.len() - (pos + 4);
                    let bytes = (len as u32).to_ne_bytes();
                    s.byte_code.buf[pos..pos + 4].copy_from_slice(&bytes);
                } else if s.peek() == Some(b'<') {
                    s.next_u8();
                    if s.peek() == Some(b'=') || s.peek() == Some(b'!') {
                        return re_parse_error(s, "lookbehind not supported yet");
                    } else {
                        let mut name_buf = [0u8; 128];
                        if re_parse_group_name(&mut name_buf, s) < 0 {
                            return re_parse_error(s, "invalid group name");
                        }
                        // re_parse_group_name consumes '>'

                        let capture_idx = s.capture_count;
                        s.capture_count += 1;
                        s.emit_op(REOPCode::SaveStart as u8);
                        s.byte_code.put_u32(capture_idx as u32);

                        last_atom_start = s.byte_code.len() as isize;
                        last_capture_count = s.capture_count;

                        if re_parse_disjunction(s, is_backward) < 0 {
                            return -1;
                        }
                        if s.next_u8() != Some(b')') {
                            return re_parse_error(s, "expecting ')'");
                        }

                        s.emit_op(REOPCode::SaveEnd as u8);
                        s.byte_code.put_u32(capture_idx as u32);
                    }
                } else {
                    let mut p = s.buf_pos;
                    let add_mask = re_parse_modifiers(s, &mut p);
                    if add_mask < 0 {
                        return -1;
                    }

                    let mut remove_mask = 0;
                    if p < s.buf.len() && s.buf[p] == b'-' {
                        p += 1;
                        remove_mask = re_parse_modifiers(s, &mut p);
                        if remove_mask < 0 {
                            return -1;
                        }
                    }

                    if p < s.buf.len() && s.buf[p] == b':' {
                        p += 1;
                        let saved_ignore_case = s.ignore_case;
                        let saved_multi_line = s.multi_line;
                        let saved_dotall = s.dotall;

                        s.ignore_case = update_modifier(
                            s.ignore_case,
                            add_mask,
                            remove_mask,
                            LRE_FLAG_IGNORECASE,
                        );
                        s.multi_line = update_modifier(
                            s.multi_line,
                            add_mask,
                            remove_mask,
                            LRE_FLAG_MULTILINE,
                        );
                        s.dotall =
                            update_modifier(s.dotall, add_mask, remove_mask, LRE_FLAG_DOTALL);

                        s.buf_pos = p;
                        last_atom_start = s.byte_code.len() as isize;
                        last_capture_count = s.capture_count;

                        if re_parse_disjunction(s, is_backward) < 0 {
                            return -1;
                        }

                        if s.next_u8() != Some(b')') {
                            return re_parse_error(s, "expecting ')'");
                        }

                        s.ignore_case = saved_ignore_case;
                        s.multi_line = saved_multi_line;
                        s.dotall = saved_dotall;
                    } else if p < s.buf.len() && s.buf[p] == b')' {
                        p += 1;
                        s.buf_pos = p;
                        s.ignore_case = update_modifier(
                            s.ignore_case,
                            add_mask,
                            remove_mask,
                            LRE_FLAG_IGNORECASE,
                        );
                        s.multi_line = update_modifier(
                            s.multi_line,
                            add_mask,
                            remove_mask,
                            LRE_FLAG_MULTILINE,
                        );
                        s.dotall =
                            update_modifier(s.dotall, add_mask, remove_mask, LRE_FLAG_DOTALL);
                    } else {
                        return re_parse_error(s, "invalid group syntax");
                    }
                }
            } else {
                let capture_idx = s.capture_count;
                s.capture_count += 1;
                s.emit_op(REOPCode::SaveStart as u8);
                s.byte_code.put_u32(capture_idx as u32);

                last_atom_start = s.byte_code.len() as isize;
                last_capture_count = s.capture_count;

                if re_parse_disjunction(s, is_backward) < 0 {
                    return -1;
                }
                if s.next_u8() != Some(b')') {
                    return re_parse_error(s, "expecting ')'");
                }

                s.emit_op(REOPCode::SaveEnd as u8);
                s.byte_code.put_u32(capture_idx as u32);
            }
        }
        b'\\' => {
            s.next_u8();
            if s.buf_pos >= s.buf.len() {
                return re_parse_error(s, "unexpected end");
            }
            let c2 = s.peek().unwrap();
            match c2 {
                b'b' => {
                    s.next_u8();
                    s.emit_op(REOPCode::WordBoundary as u8);
                }
                b'B' => {
                    s.next_u8();
                    s.emit_op(REOPCode::NotWordBoundary as u8);
                }
                b'k' => {
                    return re_parse_error(s, "named backreference not supported yet");
                }
                _ => {
                    s.buf_pos -= 1;
                    return parse_class_atom(
                        s,
                        is_backward,
                        &mut last_atom_start,
                        &mut last_capture_count,
                    );
                }
            }
        }
        b'[' => {
            last_atom_start = s.byte_code.len() as isize;
            last_capture_count = s.capture_count;
            if is_backward {
                s.emit_op(REOPCode::Prev as u8);
            }
            if re_parse_char_class(s) < 0 {
                return -1;
            }
            if is_backward {
                s.emit_op(REOPCode::Prev as u8);
            }
        }
        _ => {
            return parse_class_atom(
                s,
                is_backward,
                &mut last_atom_start,
                &mut last_capture_count,
            );
        }
    }

    // Quantifier
    if last_atom_start >= 0 {
        if let Some(c) = s.peek() {
            match c {
                b'*' | b'+' | b'?' | b'{' => {
                    let quant_min;
                    let mut quant_max;
                    let mut greedy = true;
                    let p_start = s.buf_pos;

                    match c {
                        b'*' => {
                            quant_min = 0;
                            quant_max = i32::MAX;
                            s.next_u8();
                        }
                        b'+' => {
                            quant_min = 1;
                            quant_max = i32::MAX;
                            s.next_u8();
                        }
                        b'?' => {
                            quant_min = 0;
                            quant_max = 1;
                            s.next_u8();
                        }
                        b'{' => {
                            s.next_u8();
                            if let Some(c2) = s.peek() {
                                if !is_digit(c2) {
                                    if s.is_unicode {
                                        return re_parse_error(s, "syntax error");
                                    }
                                    s.buf_pos = p_start;
                                    return 0;
                                }
                            } else {
                                if s.is_unicode {
                                    return re_parse_error(s, "syntax error");
                                }
                                s.buf_pos = p_start;
                                return 0;
                            }

                            let min = parse_digits(s, true);
                            if min < 0 {
                                return -1;
                            }
                            quant_min = min;
                            quant_max = min;

                            if s.peek() == Some(b',') {
                                s.next_u8();
                                if let Some(c3) = s.peek() {
                                    if is_digit(c3) {
                                        let max = parse_digits(s, true);
                                        if max < 0 {
                                            return -1;
                                        }
                                        quant_max = max;
                                        if quant_max < quant_min {
                                            return re_parse_error(s, "invalid repetition count");
                                        }
                                    } else {
                                        quant_max = i32::MAX;
                                    }
                                } else {
                                    quant_max = i32::MAX;
                                }
                            }

                            if s.peek() == Some(b'}') {
                                s.next_u8();
                            } else {
                                if s.is_unicode {
                                    return re_parse_error(s, "syntax error");
                                }
                                s.buf_pos = p_start;
                                return 0;
                            }
                        }
                        _ => unreachable!(),
                    }

                    if s.peek() == Some(b'?') {
                        s.next_u8();
                        greedy = false;
                    }

                    let len = s.byte_code.len() as isize - last_atom_start;
                    let add_zero_advance_check =
                        re_need_check_advance(&s.byte_code.buf[last_atom_start as usize..]);
                    let check_len = if add_zero_advance_check { 2 } else { 0 };

                    if quant_min == 0 {
                        if last_capture_count != s.capture_count {
                            if s.byte_code.insert_space(last_atom_start as usize, 3) != 0 {
                                return -1;
                            }
                            s.byte_code.buf[last_atom_start as usize] = REOPCode::SaveReset as u8;
                            s.byte_code.buf[last_atom_start as usize + 1] =
                                last_capture_count as u8;
                            s.byte_code.buf[last_atom_start as usize + 2] =
                                (s.capture_count - 1) as u8;
                            last_atom_start += 3;
                        }

                        if quant_max == 0 {
                            s.byte_code.buf.truncate(last_atom_start as usize);
                        } else if quant_max == 1 || quant_max == i32::MAX {
                            let has_goto = quant_max == i32::MAX;
                            let insert_len = 5 + check_len * 2;
                            if s.byte_code
                                .insert_space(last_atom_start as usize, insert_len)
                                != 0
                            {
                                return -1;
                            }

                            let op = if greedy {
                                REOPCode::SplitNextFirst
                            } else {
                                REOPCode::SplitGotoFirst
                            };
                            s.byte_code.buf[last_atom_start as usize] = op as u8;

                            let jump_len = len as i32
                                + 5 * (if has_goto { 1 } else { 0 })
                                + check_len as i32 * 2;
                            let val = (jump_len as u32).to_ne_bytes();
                            s.byte_code.buf
                                [last_atom_start as usize + 1..last_atom_start as usize + 5]
                                .copy_from_slice(&val);

                            if add_zero_advance_check {
                                let pos = last_atom_start as usize + 5;
                                s.byte_code.buf[pos] = REOPCode::PushCharPos as u8;
                                s.byte_code.buf[pos + 1] = 0;
                                s.emit_op_u8(REOPCode::CheckAdvance as u8, 0);
                            }

                            if has_goto {
                                s.byte_code.putc(REOPCode::Goto as u8);
                                let current_pos = s.byte_code.len();
                                let offset = last_atom_start as i32 - current_pos as i32;
                                s.byte_code.put_u32(offset as u32);
                            }
                        } else {
                            let insert_len = 11 + check_len * 2;
                            if s.byte_code
                                .insert_space(last_atom_start as usize, insert_len)
                                != 0
                            {
                                return -1;
                            }
                            let mut pos = last_atom_start as usize;
                            s.byte_code.buf[pos] = REOPCode::PushI32 as u8;
                            pos += 1;
                            s.byte_code.buf[pos] = 0;
                            pos += 1;
                            s.byte_code.buf[pos..pos + 4]
                                .copy_from_slice(&(quant_max as u32).to_ne_bytes());
                            pos += 4;

                            let op = if greedy {
                                REOPCode::SplitNextFirst
                            } else {
                                REOPCode::SplitGotoFirst
                            };
                            s.byte_code.buf[pos] = op as u8;
                            pos += 1;

                            let jump_len = len as i32 + 6 + check_len as i32 * 2;
                            s.byte_code.buf[pos..pos + 4]
                                .copy_from_slice(&(jump_len as u32).to_ne_bytes());
                            pos += 4;

                            if add_zero_advance_check {
                                s.byte_code.buf[pos] = REOPCode::PushCharPos as u8;
                                s.byte_code.buf[pos + 1] = 0;
                                s.emit_op_u8(REOPCode::CheckAdvance as u8, 0);
                            }

                            s.byte_code.putc(REOPCode::Loop as u8);
                            s.byte_code.putc(0);
                            let current_pos = s.byte_code.len();
                            let target = last_atom_start as i32 + 6;
                            let offset = target - current_pos as i32;
                            s.byte_code.put_u32(offset as u32);
                        }
                    } else {
                        return re_parse_error(s, "quantifier min > 0 not supported yet");
                    }
                }
                _ => {}
            }
        }
    }

    0
}

fn is_digit(c: u8) -> bool {
    c >= b'0' && c <= b'9'
}

fn parse_digits(s: &mut REParseState, strict: bool) -> i32 {
    let mut val = 0;
    let mut has_digits = false;
    while let Some(c) = s.peek() {
        if is_digit(c) {
            val = val * 10 + (c - b'0') as i32;
            s.next_u8();
            has_digits = true;
        } else {
            break;
        }
    }
    if !has_digits && strict {
        -1
    } else {
        val
    }
}

fn re_parse_expect(s: &mut REParseState, c: u8) -> i32 {
    if let Some(next) = s.next_u8() {
        if next == c {
            return 0;
        }
    }
    // re_parse_error(s, "expecting '%c'", c);
    -1
}

fn re_parse_error(s: &mut REParseState, msg: &str) -> i32 {
    s.error_msg = msg.to_string();
    -1
}

fn re_emit_char(s: &mut REParseState, c: u32) {
    if c <= 0xff {
        let op = if s.ignore_case {
            REOPCode::CharI
        } else {
            REOPCode::Char
        };
        s.emit_op_u8(op as u8, c as u8);
    } else {
        let op = if s.ignore_case {
            REOPCode::Char32I
        } else {
            REOPCode::Char32
        };
        s.emit_op_u32(op as u8, c);
    }
}

fn re_parse_char_class(s: &mut REParseState) -> i32 {
    let mut cr = REStringList::new();
    if re_parse_nested_class(s, &mut cr) != 0 {
        return -1;
    }
    if re_emit_string_list(s, &cr) != 0 {
        return -1;
    }
    0
}

fn re_parse_nested_class(s: &mut REParseState, cr: &mut REStringList) -> i32 {
    cr.init();

    if s.buf_pos >= s.buf.len() || s.buf[s.buf_pos] != b'[' {
        return re_parse_error(s, "expecting '['");
    }
    s.buf_pos += 1;

    let mut invert = false;
    if s.buf_pos < s.buf.len() && s.buf[s.buf_pos] == b'^' {
        s.buf_pos += 1;
        invert = true;
    }

    let mut _is_first = true;
    let mut cr1 = REStringList::new();

    loop {
        if s.buf_pos >= s.buf.len() {
            return re_parse_error(s, "unexpected end");
        }
        if s.buf[s.buf_pos] == b']' {
            break;
        }

        // TODO: Implement full nested class logic (unions, intersections, subtractions)
        // For now, just basic atoms and ranges

        let c1 = get_class_atom(s, Some(&mut cr1), true);
        if c1 < 0 {
            return -1;
        }

        if s.buf_pos < s.buf.len()
            && s.buf[s.buf_pos] == b'-'
            && s.buf_pos + 1 < s.buf.len()
            && s.buf[s.buf_pos + 1] != b']'
        {
            s.buf_pos += 1; // skip '-'
            let c2 = get_class_atom(s, Some(&mut cr1), true);
            if c2 < 0 {
                return -1;
            }

            if c1 >= CLASS_RANGE_BASE as i32 || c2 >= CLASS_RANGE_BASE as i32 {
                return re_parse_error(s, "invalid class range");
            }

            if c2 < c1 {
                return re_parse_error(s, "invalid class range");
            }

            if s.ignore_case {
                let mut cr2 = CharRange::new();
                cr2.add_interval(c1 as u32, c2 as u32 + 1);
                // cr_regexp_canonicalize(&mut cr2, s.is_unicode);
                // cr_op(&mut cr.cr, &cr2.points, CharRangeOp::Union);
                // For now just add interval
                cr.cr.add_interval(c1 as u32, c2 as u32 + 1);
            } else {
                cr.cr.add_interval(c1 as u32, c2 as u32 + 1);
            }
        } else {
            if c1 >= CLASS_RANGE_BASE as i32 {
                // Union with cr1
                // re_string_list_op(cr, &cr1, CR_OP_UNION);
                // For now, assume cr1 only has ranges in cr1.cr
                // cr_op(&mut cr.cr, &cr1.cr.points, CharRangeOp::Union);
                // But cr1 might have strings?
                // TODO
            } else {
                let mut c1_u = c1 as u32;
                if s.ignore_case {
                    c1_u = lre_canonicalize(c1_u, s.is_unicode);
                }
                cr.cr.add_interval(c1_u, c1_u + 1);
            }
        }

        _is_first = false;
    }
    s.buf_pos += 1; // skip ']'

    if invert {
        if cr.strings.len() != 0 {
            return re_parse_error(s, "negated character class with strings");
        }
        cr_invert(&mut cr.cr);
    }

    0
}

fn re_emit_string_list(s: &mut REParseState, sl: &REStringList) -> i32 {
    if sl.strings.len() == 0 {
        return re_emit_range(s, &sl.cr);
    }
    // TODO: emit strings
    re_emit_range(s, &sl.cr)
}

fn re_parse_group_name(out_buf: &mut [u8], s: &mut REParseState) -> i32 {
    re_parse_group_name_at(out_buf, s.buf, &mut s.buf_pos)
}

fn re_parse_group_name_at(out_buf: &mut [u8], buf: &[u8], p: &mut usize) -> i32 {
    let mut q = 0;
    let buf_size = out_buf.len();

    loop {
        if *p >= buf.len() {
            return -1;
        }
        let mut c = buf[*p] as u32;
        if c == b'\\' as u32 {
            *p += 1;
            if *p >= buf.len() || buf[*p] != b'u' {
                return -1;
            }
            let ret = lre_parse_escape(p, buf, 2);
            if ret < 0 {
                return -1;
            }
            c = ret as u32;
        } else if c == b'>' as u32 {
            break;
        } else if c >= 128 {
            let mut offset = 0;
            c = crate::libunicode::unicode_from_utf8(&buf[*p..], 4, &mut offset);
            if crate::libunicode::is_hi_surrogate(c) {
                let mut offset2 = 0;
                let d = crate::libunicode::unicode_from_utf8(&buf[*p + offset..], 4, &mut offset2);
                if crate::libunicode::is_lo_surrogate(d) {
                    c = crate::libunicode::from_surrogate(c, d);
                    offset += offset2;
                }
            }
            *p += offset;
        } else {
            *p += 1;
        }

        if c > 0x10FFFF {
            return -1;
        }

        if q == 0 {
            if !crate::libunicode::lre_js_is_ident_first(c) {
                return -1;
            }
        } else {
            if !crate::libunicode::lre_js_is_ident_next(c) {
                return -1;
            }
        }

        if q + 4 + 1 > buf_size {
            return -1;
        }

        if c < 128 {
            out_buf[q] = c as u8;
            q += 1;
        } else {
            let len = crate::libunicode::unicode_to_utf8(&mut out_buf[q..], c);
            q += len;
        }
    }

    if q == 0 {
        return -1;
    }
    out_buf[q] = 0;
    *p += 1; // skip '>'
    0
}

fn find_group_name(s: &mut REParseState, name: &[u8]) -> i32 {
    let mut has_named_captures = 0;
    re_parse_captures(s, &mut has_named_captures, Some(name))
}

fn re_has_named_captures(s: &REParseState) -> bool {
    s.has_named_captures > 0
}

fn re_count_captures(s: &mut REParseState) -> i32 {
    let mut has_named_captures = 0;
    re_parse_captures(s, &mut has_named_captures, None)
}

fn re_parse_captures(
    s: &mut REParseState,
    has_named_captures: &mut i32,
    capture_name: Option<&[u8]>,
) -> i32 {
    let mut p = 0;
    let mut capture_index = 1;
    *has_named_captures = 0;

    while p < s.buf.len() {
        let c = s.buf[p];
        match c {
            b'(' => {
                if p + 1 < s.buf.len() && s.buf[p + 1] == b'?' {
                    if p + 2 < s.buf.len()
                        && s.buf[p + 2] == b'<'
                        && (p + 3 >= s.buf.len() || (s.buf[p + 3] != b'=' && s.buf[p + 3] != b'!'))
                    {
                        *has_named_captures = 1;
                        p += 3;
                        if let Some(name) = capture_name {
                            let mut name_buf = [0u8; 128];
                            if re_parse_group_name_at(&mut name_buf, s.buf, &mut p) == 0 {
                                let name_len = name_buf
                                    .iter()
                                    .position(|&x| x == 0)
                                    .unwrap_or(name_buf.len());
                                if &name_buf[..name_len] == name {
                                    return capture_index;
                                }
                            }
                        }
                        capture_index += 1;
                    } else {
                        p += 1; // skip '?'
                    }
                } else {
                    capture_index += 1;
                    p += 1;
                }
            }
            b'\\' => {
                p += 2;
            }
            b'[' => {
                p += 1;
                while p < s.buf.len() {
                    if s.buf[p] == b']' {
                        p += 1;
                        break;
                    }
                    if s.buf[p] == b'\\' {
                        p += 1;
                    }
                    p += 1;
                }
            }
            _ => {
                p += 1;
            }
        }
    }
    capture_index
}

fn lre_canonicalize(c: u32, is_unicode: bool) -> u32 {
    if is_unicode {
        let mut res = [0u32; 3];
        if crate::libunicode::lre_case_conv(&mut res, c, 2) == 1 {
            res[0]
        } else {
            c
        }
    } else {
        if c >= b'a' as u32 && c <= b'z' as u32 {
            c - b'a' as u32 + b'A' as u32
        } else {
            c
        }
    }
}

fn get_reop_size(op: u8) -> usize {
    if op == REOPCode::Char as u8 {
        return 3;
    }
    if op == REOPCode::CharI as u8 {
        return 3;
    }
    if op == REOPCode::Char32 as u8 {
        return 5;
    }
    if op == REOPCode::Char32I as u8 {
        return 5;
    }
    if op == REOPCode::Dot as u8 {
        return 1;
    }
    if op == REOPCode::Any as u8 {
        return 1;
    }
    if op == REOPCode::LineStart as u8 {
        return 1;
    }
    if op == REOPCode::LineStartM as u8 {
        return 1;
    }
    if op == REOPCode::LineEnd as u8 {
        return 1;
    }
    if op == REOPCode::LineEndM as u8 {
        return 1;
    }
    if op == REOPCode::Goto as u8 {
        return 5;
    }
    if op == REOPCode::SplitGotoFirst as u8 {
        return 5;
    }
    if op == REOPCode::SplitNextFirst as u8 {
        return 5;
    }
    if op == REOPCode::Match as u8 {
        return 1;
    }
    if op == REOPCode::LookaheadMatch as u8 {
        return 1;
    }
    if op == REOPCode::NegativeLookaheadMatch as u8 {
        return 1;
    }
    if op == REOPCode::SaveStart as u8 {
        return 2;
    }
    if op == REOPCode::SaveEnd as u8 {
        return 2;
    }
    if op == REOPCode::SaveReset as u8 {
        return 3;
    }
    if op == REOPCode::Loop as u8 {
        return 6;
    }
    if op == REOPCode::PushI32 as u8 {
        return 6;
    }
    if op == REOPCode::WordBoundary as u8 {
        return 1;
    }
    if op == REOPCode::WordBoundaryI as u8 {
        return 1;
    }
    if op == REOPCode::NotWordBoundary as u8 {
        return 1;
    }
    if op == REOPCode::NotWordBoundaryI as u8 {
        return 1;
    }
    if op == REOPCode::BackReference as u8 {
        return 2;
    }
    if op == REOPCode::BackReferenceI as u8 {
        return 2;
    }
    if op == REOPCode::BackwardBackReference as u8 {
        return 2;
    }
    if op == REOPCode::BackwardBackReferenceI as u8 {
        return 2;
    }
    if op == REOPCode::Range as u8 {
        return 3;
    }
    if op == REOPCode::RangeI as u8 {
        return 3;
    }
    if op == REOPCode::Range32 as u8 {
        return 3;
    }
    if op == REOPCode::Range32I as u8 {
        return 3;
    }
    if op == REOPCode::Lookahead as u8 {
        return 5;
    }
    if op == REOPCode::NegativeLookahead as u8 {
        return 5;
    }
    if op == REOPCode::PushCharPos as u8 {
        return 2;
    }
    if op == REOPCode::CheckAdvance as u8 {
        return 2;
    }
    if op == REOPCode::Prev as u8 {
        return 1;
    }
    1
}

fn re_need_check_advance(bc_buf: &[u8]) -> bool {
    let mut pos = 0;
    let mut ret = true;

    while pos < bc_buf.len() {
        let opcode = bc_buf[pos];
        let mut len = get_reop_size(opcode);

        if opcode == REOPCode::Range as u8 || opcode == REOPCode::RangeI as u8 {
            let val = u16::from_ne_bytes([bc_buf[pos + 1], bc_buf[pos + 2]]) as usize;
            len += val * 4;
            ret = false;
        } else if opcode == REOPCode::Range32 as u8 || opcode == REOPCode::Range32I as u8 {
            let val = u16::from_ne_bytes([bc_buf[pos + 1], bc_buf[pos + 2]]) as usize;
            len += val * 8;
            ret = false;
        } else if opcode == REOPCode::Char as u8
            || opcode == REOPCode::CharI as u8
            || opcode == REOPCode::Char32 as u8
            || opcode == REOPCode::Char32I as u8
            || opcode == REOPCode::Dot as u8
            || opcode == REOPCode::Any as u8
        {
            ret = false;
        }

        pos += len;
    }
    ret
}

fn re_parse_modifiers(s: &mut REParseState, p: &mut usize) -> i32 {
    let mut mask = 0;
    loop {
        if *p >= s.buf.len() {
            break;
        }
        let c = s.buf[*p];
        let val = match c {
            b'i' => LRE_FLAG_IGNORECASE,
            b'm' => LRE_FLAG_MULTILINE,
            b's' => LRE_FLAG_DOTALL,
            _ => break,
        };
        if (mask & val) != 0 {
            return re_parse_error(s, "duplicate modifier");
        }
        mask |= val;
        *p += 1;
    }
    mask
}

fn update_modifier(current: bool, add_mask: i32, remove_mask: i32, flag: i32) -> bool {
    if (add_mask & flag) != 0 {
        true
    } else if (remove_mask & flag) != 0 {
        false
    } else {
        current
    }
}

const RE_HEADER_FLAGS: usize = 0;
const RE_HEADER_CAPTURE_COUNT: usize = 2;
const RE_HEADER_STACK_SIZE: usize = 3;
const RE_HEADER_BYTECODE_LEN: usize = 4;
const RE_HEADER_LEN: usize = 8;

pub fn lre_get_capture_count(bc_buf: &[u8]) -> i32 {
    bc_buf[RE_HEADER_CAPTURE_COUNT] as i32
}

pub fn lre_get_flags(bc_buf: &[u8]) -> i32 {
    get_u16(&bc_buf[RE_HEADER_FLAGS..]) as i32
}

pub fn lre_get_groupnames(bc_buf: &[u8]) -> Option<&[u8]> {
    let offset = u32::from_ne_bytes([bc_buf[4], bc_buf[5], bc_buf[6], bc_buf[7]]) as usize;
    if offset == 0 {
        None
    } else {
        Some(&bc_buf[offset..])
    }
}

#[derive(Copy, Clone)]
struct StackElem {
    val: usize,
}

impl StackElem {
    fn new_val(val: usize) -> Self {
        Self { val }
    }
    fn new_ptr(ptr: *mut u8) -> Self {
        Self { val: ptr as usize }
    }
    fn as_ptr(self) -> *mut u8 {
        self.val as *mut u8
    }
}

#[repr(usize)]
enum REExecState {
    Split = 0,
    Lookahead = 1,
    NegativeLookahead = 2,
}

const BP_TYPE_BITS: usize = 3;

struct REExecContext<'a> {
    cbuf: &'a [u8],
    cbuf_end: usize,
    cbuf_type: i32,
    capture_count: usize,
    stack_size_max: usize,
    is_unicode: bool,
    interrupt_counter: i32,
    opaque: *mut std::ffi::c_void,
    stack_buf: Vec<StackElem>,
}

const INTERRUPT_COUNTER_INIT: i32 = 10000;

pub fn lre_exec(
    capture: &mut [*mut u8],
    bc_buf: &mut [u8],
    cbuf: &[u8],
    cindex: i32,
    clen: i32,
    cshift: i32,
    opaque: *mut std::ffi::c_void,
) -> i32 {
    let re_flags = lre_get_flags(bc_buf);
    let is_unicode = (re_flags & (LRE_FLAG_UNICODE | LRE_FLAG_UNICODE_SETS)) != 0;
    let mut cbuf_type = cshift;
    if cbuf_type == 1 && is_unicode {
        cbuf_type = 2;
    }

    let capture_count = bc_buf[RE_HEADER_CAPTURE_COUNT] as usize;
    let stack_size_max = bc_buf[RE_HEADER_STACK_SIZE] as usize;

    let mut s = REExecContext {
        cbuf,
        cbuf_end: (clen as usize) << cbuf_type,
        cbuf_type,
        capture_count,
        stack_size_max,
        is_unicode,
        interrupt_counter: INTERRUPT_COUNTER_INIT,
        opaque,
        stack_buf: Vec::new(),
    };

    for i in 0..capture_count * 2 {
        capture[i] = std::ptr::null_mut();
    }

    let mut aux_stack: Vec<*mut u8> = vec![std::ptr::null_mut(); stack_size_max];

    let cptr_offset = (cindex as usize) << cbuf_type;

    if cindex > 0 && cindex < clen && cbuf_type == 2 {
        // Surrogate pair adjustment
        // TODO: Implement surrogate check
    }

    lre_exec_backtrack(
        &mut s,
        capture,
        &mut aux_stack,
        &mut bc_buf[RE_HEADER_LEN..],
        cptr_offset,
    )
}

macro_rules! goto_no_match {
    ($s:expr, $capture:expr, $aux_stack:expr, $pc_idx:expr, $cptr:expr, $bp:expr, $label:lifetime) => {
        loop {
            if $bp == 0 {
                return 0;
            }
            while $s.stack_buf.len() > $bp {
                let idx2 = $s.stack_buf.pop().unwrap().val as isize;
                let ptr = $s.stack_buf.pop().unwrap().as_ptr();
                if idx2 >= 0 {
                    $capture[idx2 as usize] = ptr;
                } else {
                    $aux_stack[(-idx2 - 1) as usize] = ptr;
                }
            }

            let bp_packed = $s.stack_buf.pop().unwrap().val;
            let cptr_val = $s.stack_buf.pop().unwrap().val;
            let pc_val = $s.stack_buf.pop().unwrap().val;

            $pc_idx = pc_val;
            $cptr = cptr_val;

            let type_ = bp_packed & ((1 << BP_TYPE_BITS) - 1);
            let prev_bp = bp_packed >> BP_TYPE_BITS;
            $bp = prev_bp;

            if type_ != REExecState::Lookahead as usize {
                break;
            }
        }
        continue $label;
    };
}

fn lre_exec_backtrack(
    s: &mut REExecContext,
    capture: &mut [*mut u8],
    aux_stack: &mut [*mut u8],
    pc: &mut [u8],
    cptr_offset: usize,
) -> i32 {
    let mut pc_idx = 0;
    let mut cptr = cptr_offset;
    let mut bp = 0; // Base pointer index in stack_buf

    'outer: loop {
        if pc_idx >= pc.len() {
            return -1;
        }
        let opcode = pc[pc_idx];
        pc_idx += 1;

        match opcode {
            x if x == REOPCode::Match as u8 => {
                return 1;
            }
            x if x == REOPCode::Char as u8 => {
                let val = get_u16(&pc[pc_idx..]) as u32;
                pc_idx += 2;
                if cptr >= s.cbuf_end {
                    goto_no_match!(s, capture, aux_stack, pc_idx, cptr, bp, 'outer);
                }
                let c = if s.cbuf_type == 1 {
                    get_u16(&s.cbuf[cptr..]) as u32
                } else {
                    s.cbuf[cptr] as u32
                };
                if c != val {
                    goto_no_match!(s, capture, aux_stack, pc_idx, cptr, bp, 'outer);
                }
                cptr += 1 << s.cbuf_type;
            }
            x if x == REOPCode::Goto as u8 => {
                let offset = get_u32(&pc[pc_idx..]) as i32;
                pc_idx += 4;
                pc_idx = (pc_idx as isize + offset as isize) as usize;
            }
            x if x == REOPCode::SplitGotoFirst as u8 => {
                let offset = get_u32(&pc[pc_idx..]) as i32;
                pc_idx += 4;
                let next_pc = (pc_idx as isize + offset as isize) as usize;

                if s.stack_buf.len() + 3 > s.stack_size_max {
                    return LRE_RET_MEMORY_ERROR;
                }
                s.stack_buf.push(StackElem::new_val(next_pc));
                s.stack_buf.push(StackElem::new_val(cptr));
                let val = (bp << BP_TYPE_BITS) | (REExecState::Split as usize);
                s.stack_buf.push(StackElem::new_val(val));
                bp = s.stack_buf.len();
            }
            x if x == REOPCode::SplitNextFirst as u8 => {
                let offset = get_u32(&pc[pc_idx..]) as i32;
                pc_idx += 4;
                let next_pc = (pc_idx as isize + offset as isize) as usize;

                if s.stack_buf.len() + 3 > s.stack_size_max {
                    return LRE_RET_MEMORY_ERROR;
                }
                s.stack_buf.push(StackElem::new_val(pc_idx));
                s.stack_buf.push(StackElem::new_val(cptr));
                let val = (bp << BP_TYPE_BITS) | (REExecState::Split as usize);
                s.stack_buf.push(StackElem::new_val(val));
                bp = s.stack_buf.len();
                pc_idx = next_pc;
            }
            x if x == REOPCode::SaveStart as u8 || x == REOPCode::SaveEnd as u8 => {
                let val = pc[pc_idx];
                pc_idx += 1;
                let idx = (val as usize) * 2 + (opcode - REOPCode::SaveStart as u8) as usize;

                if s.stack_buf.len() + 2 > s.stack_size_max {
                    return LRE_RET_MEMORY_ERROR;
                }
                s.stack_buf.push(StackElem::new_val(idx));
                s.stack_buf.push(StackElem::new_ptr(capture[idx]));

                capture[idx] = unsafe { s.cbuf.as_ptr().add(cptr) as *mut u8 };
            }
            x if x == REOPCode::SaveReset as u8 => {
                let val = pc[pc_idx] as usize;
                let val2 = pc[pc_idx + 1] as usize;
                pc_idx += 2;

                if s.stack_buf.len() + 2 * (val2 - val + 1) > s.stack_size_max {
                    return LRE_RET_MEMORY_ERROR;
                }
                for i in val..=val2 {
                    let idx = i * 2;
                    s.stack_buf.push(StackElem::new_val(idx));
                    s.stack_buf.push(StackElem::new_ptr(capture[idx]));
                    capture[idx] = std::ptr::null_mut();

                    let idx = i * 2 + 1;
                    s.stack_buf.push(StackElem::new_val(idx));
                    s.stack_buf.push(StackElem::new_ptr(capture[idx]));
                    capture[idx] = std::ptr::null_mut();
                }
            }
            x if x == REOPCode::PushI32 as u8 => {
                let idx = pc[pc_idx] as usize;
                let val = get_u32(&pc[pc_idx + 1..]) as usize;
                pc_idx += 5;

                let mut saved = false;
                let mut i = s.stack_buf.len();
                while i > bp {
                    let marker = s.stack_buf[i - 2].val as isize;
                    if marker == -(idx as isize + 1) {
                        saved = true;
                        break;
                    }
                    i -= 2;
                }

                if !saved {
                    if s.stack_buf.len() + 2 > s.stack_size_max {
                        return LRE_RET_MEMORY_ERROR;
                    }
                    let marker = (-(idx as isize + 1)) as usize;
                    s.stack_buf.push(StackElem::new_val(marker));
                    s.stack_buf.push(StackElem::new_ptr(aux_stack[idx]));
                }
                aux_stack[idx] = val as *mut u8;
            }
            x if x == REOPCode::Loop as u8 => {
                let idx = pc[pc_idx] as usize;
                let val = get_u32(&pc[pc_idx + 1..]) as i32;
                pc_idx += 5;

                let val2 = (aux_stack[idx] as usize).wrapping_sub(1);

                let mut saved = false;
                let mut i = s.stack_buf.len();
                while i > bp {
                    let marker = s.stack_buf[i - 2].val as isize;
                    if marker == -(idx as isize + 1) {
                        saved = true;
                        break;
                    }
                    i -= 2;
                }

                if !saved {
                    if s.stack_buf.len() + 2 > s.stack_size_max {
                        return LRE_RET_MEMORY_ERROR;
                    }
                    let marker = (-(idx as isize + 1)) as usize;
                    s.stack_buf.push(StackElem::new_val(marker));
                    s.stack_buf.push(StackElem::new_ptr(aux_stack[idx]));
                }
                aux_stack[idx] = val2 as *mut u8;

                if val2 != 0 {
                    pc_idx = (pc_idx as isize + val as isize) as usize;
                    if s.interrupt_counter <= 0 {
                        s.interrupt_counter = INTERRUPT_COUNTER_INIT;
                        // TODO: check timeout
                    }
                    s.interrupt_counter -= 1;
                }
            }
            _ => {
                return -1;
            }
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum REOPCode {
    Invalid = 0,
    Char,
    CharI,
    Char32,
    Char32I,
    Dot,
    Any,
    LineStart,
    LineStartM,
    LineEnd,
    LineEndM,
    Goto,
    SplitGotoFirst,
    SplitNextFirst,
    Match,
    LookaheadMatch,
    NegativeLookaheadMatch,
    SaveStart,
    SaveEnd,
    SaveReset,
    Loop,
    PushI32,
    WordBoundary,
    WordBoundaryI,
    NotWordBoundary,
    NotWordBoundaryI,
    BackReference,
    BackReferenceI,
    BackwardBackReference,
    BackwardBackReferenceI,
    Range,
    RangeI,
    Range32,
    Range32I,
    Lookahead,
    NegativeLookahead,
    PushCharPos,
    CheckAdvance,
    Prev,
}

pub const REOP_COUNT: usize = REOPCode::Prev as usize + 1;

pub struct REOpCodeInfo {
    pub name: &'static str,
    pub size: u8,
}

pub const REOPCODE_INFO: [REOpCodeInfo; REOP_COUNT] = [
    REOpCodeInfo {
        name: "invalid",
        size: 1,
    },
    REOpCodeInfo {
        name: "char",
        size: 3,
    },
    REOpCodeInfo {
        name: "char_i",
        size: 3,
    },
    REOpCodeInfo {
        name: "char32",
        size: 5,
    },
    REOpCodeInfo {
        name: "char32_i",
        size: 5,
    },
    REOpCodeInfo {
        name: "dot",
        size: 1,
    },
    REOpCodeInfo {
        name: "any",
        size: 1,
    },
    REOpCodeInfo {
        name: "line_start",
        size: 1,
    },
    REOpCodeInfo {
        name: "line_start_m",
        size: 1,
    },
    REOpCodeInfo {
        name: "line_end",
        size: 1,
    },
    REOpCodeInfo {
        name: "line_end_m",
        size: 1,
    },
    REOpCodeInfo {
        name: "goto",
        size: 5,
    },
    REOpCodeInfo {
        name: "split_goto_first",
        size: 5,
    },
    REOpCodeInfo {
        name: "split_next_first",
        size: 5,
    },
    REOpCodeInfo {
        name: "match",
        size: 1,
    },
    REOpCodeInfo {
        name: "lookahead_match",
        size: 1,
    },
    REOpCodeInfo {
        name: "negative_lookahead_match",
        size: 1,
    },
    REOpCodeInfo {
        name: "save_start",
        size: 2,
    },
    REOpCodeInfo {
        name: "save_end",
        size: 2,
    },
    REOpCodeInfo {
        name: "save_reset",
        size: 3,
    },
    REOpCodeInfo {
        name: "loop",
        size: 6,
    },
    REOpCodeInfo {
        name: "push_i32",
        size: 6,
    },
    REOpCodeInfo {
        name: "word_boundary",
        size: 1,
    },
    REOpCodeInfo {
        name: "word_boundary_i",
        size: 1,
    },
    REOpCodeInfo {
        name: "not_word_boundary",
        size: 1,
    },
    REOpCodeInfo {
        name: "not_word_boundary_i",
        size: 1,
    },
    REOpCodeInfo {
        name: "back_reference",
        size: 2,
    },
    REOpCodeInfo {
        name: "back_reference_i",
        size: 2,
    },
    REOpCodeInfo {
        name: "backward_back_reference",
        size: 2,
    },
    REOpCodeInfo {
        name: "backward_back_reference_i",
        size: 2,
    },
    REOpCodeInfo {
        name: "range",
        size: 3,
    },
    REOpCodeInfo {
        name: "range_i",
        size: 3,
    },
    REOpCodeInfo {
        name: "range32",
        size: 3,
    },
    REOpCodeInfo {
        name: "range32_i",
        size: 3,
    },
    REOpCodeInfo {
        name: "lookahead",
        size: 5,
    },
    REOpCodeInfo {
        name: "negative_lookahead",
        size: 5,
    },
    REOpCodeInfo {
        name: "push_char_pos",
        size: 2,
    },
    REOpCodeInfo {
        name: "check_advance",
        size: 2,
    },
    REOpCodeInfo {
        name: "prev",
        size: 1,
    },
];

pub struct REParseState<'a> {
    pub byte_code: DynBuf,
    pub buf: &'a [u8],
    pub buf_pos: usize,
    pub re_flags: i32,
    pub is_unicode: bool,
    pub unicode_sets: bool,
    pub ignore_case: bool,
    pub multi_line: bool,
    pub dotall: bool,
    pub capture_count: i32,
    pub total_capture_count: i32,
    pub has_named_captures: i32,
    pub opaque: *mut std::ffi::c_void,
    pub group_names: DynBuf,
    pub error_msg: String,
}

impl<'a> REParseState<'a> {
    pub fn new(buf: &'a [u8], re_flags: i32, opaque: *mut std::ffi::c_void) -> Self {
        let is_unicode = (re_flags & LRE_FLAG_UNICODE) != 0;
        let unicode_sets = (re_flags & LRE_FLAG_UNICODE_SETS) != 0;
        Self {
            byte_code: DynBuf::new(),
            buf,
            buf_pos: 0,
            re_flags,
            is_unicode: is_unicode || unicode_sets,
            unicode_sets,
            ignore_case: (re_flags & LRE_FLAG_IGNORECASE) != 0,
            multi_line: (re_flags & LRE_FLAG_MULTILINE) != 0,
            dotall: (re_flags & LRE_FLAG_DOTALL) != 0,
            capture_count: 1,
            total_capture_count: -1,
            has_named_captures: -1,
            opaque,
            group_names: DynBuf::new(),
            error_msg: String::new(),
        }
    }

    pub fn emit_op(&mut self, op: u8) {
        self.byte_code.putc(op);
    }

    pub fn emit_val(&mut self, val: u32) {
        self.byte_code.put_u32(val);
    }

    pub fn emit_goto(&mut self, op: u8, val: u32) -> usize {
        self.byte_code.putc(op);
        let pos = self.byte_code.len();
        self.byte_code.put_u32(val);
        pos
    }

    pub fn emit_op_u8(&mut self, op: u8, val: u8) {
        self.byte_code.putc(op);
        self.byte_code.putc(val);
    }

    pub fn emit_op_u32(&mut self, op: u8, val: u32) -> usize {
        self.byte_code.putc(op);
        let pos = self.byte_code.len();
        self.byte_code.put_u32(val);
        pos
    }

    pub fn emit_goto_offset(&mut self, op: u8, target_pos: usize) -> usize {
        self.byte_code.putc(op);
        let pos = self.byte_code.len();
        let offset = target_pos as isize - (pos as isize + 4);
        self.byte_code.put_u32(offset as u32);
        pos
    }

    pub fn peek(&self) -> Option<u8> {
        if self.buf_pos < self.buf.len() {
            Some(self.buf[self.buf_pos])
        } else {
            None
        }
    }

    pub fn next_u8(&mut self) -> Option<u8> {
        if self.buf_pos < self.buf.len() {
            let c = self.buf[self.buf_pos];
            self.buf_pos += 1;
            Some(c)
        } else {
            None
        }
    }
}

fn from_hex(c: u8) -> i32 {
    if c >= b'0' && c <= b'9' {
        (c - b'0') as i32
    } else if c >= b'A' && c <= b'F' {
        (c - b'A' + 10) as i32
    } else if c >= b'a' && c <= b'f' {
        (c - b'a' + 10) as i32
    } else {
        -1
    }
}

fn is_hi_surrogate(c: u32) -> bool {
    c >= 0xD800 && c <= 0xDBFF
}

fn is_lo_surrogate(c: u32) -> bool {
    c >= 0xDC00 && c <= 0xDFFF
}

fn from_surrogate(hi: u32, lo: u32) -> u32 {
    0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)
}

fn lre_parse_escape(p: &mut usize, buf: &[u8], allow_utf16: i32) -> i32 {
    if *p >= buf.len() {
        return -2;
    }
    let c = buf[*p];
    *p += 1;
    let mut res: u32;

    match c {
        b'b' => res = 0x08,
        b'f' => res = 0x0C,
        b'n' => res = 0x0A,
        b'r' => res = 0x0D,
        b't' => res = 0x09,
        b'v' => res = 0x0B,
        b'x' | b'u' => {
            let mut h: i32;
            let n: i32;
            let mut c_val: u32 = 0;

            if *p < buf.len() && buf[*p] == b'{' && allow_utf16 != 0 {
                *p += 1;
                loop {
                    if *p >= buf.len() {
                        return -1;
                    }
                    h = from_hex(buf[*p]);
                    *p += 1;
                    if h < 0 {
                        return -1;
                    }
                    c_val = (c_val << 4) | (h as u32);
                    if c_val > 0x10FFFF {
                        return -1;
                    }
                    if *p < buf.len() && buf[*p] == b'}' {
                        break;
                    }
                }
                *p += 1;
            } else {
                if c == b'x' {
                    n = 2;
                } else {
                    n = 4;
                }

                for _ in 0..n {
                    if *p >= buf.len() {
                        return -1;
                    }
                    h = from_hex(buf[*p]);
                    *p += 1;
                    if h < 0 {
                        return -1;
                    }
                    c_val = (c_val << 4) | (h as u32);
                }

                if is_hi_surrogate(c_val)
                    && allow_utf16 == 2
                    && *p + 5 < buf.len()
                    && buf[*p] == b'\\'
                    && buf[*p + 1] == b'u'
                {
                    let mut c1: u32 = 0;
                    let mut is_valid = true;
                    for i in 0..4 {
                        h = from_hex(buf[*p + 2 + i]);
                        if h < 0 {
                            is_valid = false;
                            break;
                        }
                        c1 = (c1 << 4) | (h as u32);
                    }
                    if is_valid && is_lo_surrogate(c1) {
                        *p += 6;
                        c_val = from_surrogate(c_val, c1);
                    }
                }
            }
            res = c_val;
        }
        b'0'..=b'7' => {
            res = (c - b'0') as u32;
            if allow_utf16 == 2 {
                if res != 0 || (*p < buf.len() && buf[*p] >= b'0' && buf[*p] <= b'9') {
                    return -1;
                }
            } else {
                // legacy octal
                if *p < buf.len() {
                    let v = buf[*p] as u32 - b'0' as u32;
                    if v <= 7 {
                        res = (res << 3) | v;
                        *p += 1;
                        if res < 32 && *p < buf.len() {
                            let v2 = buf[*p] as u32 - b'0' as u32;
                            if v2 <= 7 {
                                res = (res << 3) | v2;
                                *p += 1;
                            }
                        }
                    }
                }
            }
        }
        _ => return -2,
    }

    res as i32
}

pub struct REStringList {
    pub cr: CharRange,
    pub strings: Vec<Vec<u32>>,
}

impl REStringList {
    pub fn new() -> Self {
        Self {
            cr: CharRange::new(),
            strings: Vec::new(),
        }
    }

    pub fn init(&mut self) {
        self.cr.init();
        self.strings.clear();
    }
}

const CLASS_RANGE_BASE: u32 = 0x40000000;

const CHAR_RANGE_D_DATA: &[u16] = &[1, 0x0030, 0x0039 + 1];

const CHAR_RANGE_S_DATA: &[u16] = &[
    10,
    0x0009,
    0x000D + 1,
    0x0020,
    0x0020 + 1,
    0x00A0,
    0x00A0 + 1,
    0x1680,
    0x1680 + 1,
    0x2000,
    0x200A + 1,
    0x2028,
    0x2029 + 1,
    0x202F,
    0x202F + 1,
    0x205F,
    0x205F + 1,
    0x3000,
    0x3000 + 1,
    0xFEFF,
    0xFEFF + 1,
];

const CHAR_RANGE_W_DATA: &[u16] = &[
    4,
    0x0030,
    0x0039 + 1,
    0x0041,
    0x005A + 1,
    0x005F,
    0x005F + 1,
    0x0061,
    0x007A + 1,
];

fn cr_init_char_range(s: &mut REParseState, cr: &mut REStringList, c: u32) -> i32 {
    cr.init();
    if c >= CLASS_RANGE_BASE {
        let idx = c - CLASS_RANGE_BASE;
        let data = match idx {
            0 => CHAR_RANGE_D_DATA, // d
            1 => CHAR_RANGE_D_DATA, // D
            2 => CHAR_RANGE_S_DATA, // s
            3 => CHAR_RANGE_S_DATA, // S
            4 => CHAR_RANGE_W_DATA, // w
            5 => CHAR_RANGE_W_DATA, // W
            _ => return -1,
        };

        let len = data[0] as usize;
        for i in 0..len {
            cr.cr
                .add_interval(data[1 + i * 2] as u32, data[2 + i * 2] as u32);
        }

        if s.ignore_case {
            // cr_regexp_canonicalize(&mut cr.cr, s.is_unicode);
        }

        if idx % 2 != 0 {
            cr_invert(&mut cr.cr);
        }
    } else {
        let mut c1 = c;
        if s.ignore_case {
            c1 = lre_canonicalize(c1, s.is_unicode);
        }
        cr.cr.add_interval(c1, c1 + 1);
    }
    0
}

fn re_emit_range(s: &mut REParseState, cr: &CharRange) -> i32 {
    let len = cr.points.len() / 2;
    if len >= 65535 {
        return re_parse_error(s, "too many ranges");
    }
    if len == 0 {
        s.emit_op(REOPCode::Char32 as u8);
        s.byte_code.put_u32(0xFFFFFFFF);
    } else {
        let mut high = cr.points[cr.points.len() - 1];
        if high == 0xFFFFFFFF {
            high = cr.points[cr.points.len() - 2];
        }
        if high <= 0xffff {
            let op = if s.ignore_case {
                REOPCode::RangeI
            } else {
                REOPCode::Range
            };
            s.emit_op(op as u8);
            s.byte_code.put_u16(len as u16);
            for i in (0..cr.points.len()).step_by(2) {
                s.byte_code.put_u16(cr.points[i] as u16);
                let mut h = cr.points[i + 1] - 1;
                if h == 0xFFFFFFFF - 1 {
                    h = 0xFFFF;
                }
                s.byte_code.put_u16(h as u16);
            }
        } else {
            let op = if s.ignore_case {
                REOPCode::Range32I
            } else {
                REOPCode::Range32
            };
            s.emit_op(op as u8);
            s.byte_code.put_u16(len as u16);
            for i in (0..cr.points.len()).step_by(2) {
                s.byte_code.put_u32(cr.points[i]);
                s.byte_code.put_u32(cr.points[i + 1] - 1);
            }
        }
    }
    0
}
