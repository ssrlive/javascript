#![allow(dead_code)]
#![allow(non_snake_case)]

use crate::libunicode_table::*;
// use std::cmp::{max, min};

#[derive(Clone, Debug)]
pub struct CharRange {
    pub points: Vec<u32>,
}

impl CharRange {
    pub fn new() -> Self {
        Self { points: Vec::new() }
    }

    pub fn init(&mut self) {
        self.points.clear();
    }

    pub fn free(&mut self) {
        self.points.clear();
    }

    pub fn copy(&mut self, other: &CharRange) {
        self.points = other.points.clone();
    }

    pub fn add_point(&mut self, v: u32) {
        self.points.push(v);
    }

    pub fn add_interval(&mut self, c1: u32, c2: u32) {
        self.points.push(c1);
        self.points.push(c2);
    }
}

#[derive(PartialEq, Eq, Clone, Copy)]
pub enum CharRangeOp {
    Union,
    Inter,
    Xor,
    Sub,
}

pub fn cr_op(cr: &mut CharRange, a_pt: &[u32], b_pt: &[u32], op: CharRangeOp) -> i32 {
    let mut a_idx = 0;
    let mut b_idx = 0;
    let mut v: u32;
    let mut is_in: bool;

    cr.points.clear();

    loop {
        if a_idx < a_pt.len() && b_idx < b_pt.len() {
            if a_pt[a_idx] < b_pt[b_idx] {
                v = a_pt[a_idx];
                a_idx += 1;
            } else if a_pt[a_idx] == b_pt[b_idx] {
                v = a_pt[a_idx];
                a_idx += 1;
                b_idx += 1;
            } else {
                v = b_pt[b_idx];
                b_idx += 1;
            }
        } else if a_idx < a_pt.len() {
            v = a_pt[a_idx];
            a_idx += 1;
        } else if b_idx < b_pt.len() {
            v = b_pt[b_idx];
            b_idx += 1;
        } else {
            break;
        }

        match op {
            CharRangeOp::Union => {
                is_in = (a_idx & 1) != 0 || (b_idx & 1) != 0;
            }
            CharRangeOp::Inter => {
                is_in = (a_idx & 1) != 0 && (b_idx & 1) != 0;
            }
            CharRangeOp::Xor => {
                is_in = ((a_idx & 1) != 0) ^ ((b_idx & 1) != 0);
            }
            CharRangeOp::Sub => {
                is_in = (a_idx & 1) != 0 && (b_idx & 1) == 0;
            }
        }

        if is_in != (cr.points.len() & 1 != 0) {
            cr.points.push(v);
        }
    }

    cr_compress(cr);
    0
}

fn cr_compress(cr: &mut CharRange) {
    let mut k = 0;
    let mut i = 0;
    let len = cr.points.len();

    while i + 1 < len {
        if cr.points[i] == cr.points[i + 1] {
            i += 2;
        } else {
            let mut j = i;
            while j + 3 < len && cr.points[j + 1] == cr.points[j + 2] {
                j += 2;
            }
            let pti = cr.points[i];
            let ptj1 = cr.points[j + 1];
            cr.points[k] = pti;
            cr.points[k + 1] = ptj1;
            k += 2;
            i = j + 2;
        }
    }
    cr.points.truncate(k);
}

pub fn cr_union_interval(cr: &mut CharRange, c1: u32, c2: u32) -> i32 {
    let interval = vec![c1, c2];
    let mut res = CharRange::new();
    cr_op(&mut res, &cr.points, &interval, CharRangeOp::Union);
    *cr = res;
    0
}

pub fn cr_invert(cr: &mut CharRange) {
    let full_range = vec![0, 0x110000];
    let mut res = CharRange::new();
    cr_op(&mut res, &full_range, &cr.points, CharRangeOp::Sub);
    *cr = res;
}

// Case Conversion

const RUN_TYPE_U: u32 = 0;
const RUN_TYPE_L: u32 = 1;
const RUN_TYPE_UF: u32 = 2;
const RUN_TYPE_LF: u32 = 3;
const RUN_TYPE_UL: u32 = 4;
const RUN_TYPE_LSU: u32 = 5;
const RUN_TYPE_U2L_399_EXT2: u32 = 6;
const RUN_TYPE_UF_D20: u32 = 7;
const RUN_TYPE_UF_D1_EXT: u32 = 8;
const RUN_TYPE_U_EXT: u32 = 9;
const RUN_TYPE_LF_EXT: u32 = 10;
const RUN_TYPE_UF_EXT2: u32 = 11;
const RUN_TYPE_LF_EXT2: u32 = 12;
const RUN_TYPE_UF_EXT3: u32 = 13;

fn lre_case_conv1(c: u32, conv_type: i32) -> u32 {
    let mut res = [0u32; 3];
    lre_case_conv(&mut res, c, conv_type);
    res[0]
}

fn lre_case_conv_entry(res: &mut [u32], mut c: u32, conv_type: i32, idx: usize, v: u32) -> usize {
    let is_lower = conv_type != 0;
    let type_ = (v >> (32 - 17 - 7 - 4)) & 0xf;
    let data = ((v & 0xf) << 8) | CASE_CONV_TABLE2[idx] as u32;
    let code = v >> (32 - 17);

    match type_ {
        RUN_TYPE_U | RUN_TYPE_L | RUN_TYPE_UF | RUN_TYPE_LF => {
            if conv_type == (type_ & 1) as i32 || (type_ >= RUN_TYPE_UF && conv_type == 2) {
                c = c
                    .wrapping_sub(code)
                    .wrapping_add(CASE_CONV_TABLE1[data as usize] >> (32 - 17));
            }
        }
        RUN_TYPE_UL => {
            let a = c.wrapping_sub(code);
            if (a & 1) == (1 - is_lower as u32) {
                c = (a ^ 1).wrapping_add(code);
            }
        }
        RUN_TYPE_LSU => {
            let a = c.wrapping_sub(code);
            if a == 1 {
                c = c.wrapping_add((2 * is_lower as i32 - 1) as u32);
            } else if a == (1 - is_lower as u32) * 2 {
                c = c.wrapping_add(((2 * is_lower as i32 - 1) * 2) as u32);
            }
        }
        RUN_TYPE_U2L_399_EXT2 => {
            if !is_lower {
                res[0] = c
                    .wrapping_sub(code)
                    .wrapping_add(CASE_CONV_EXT[(data >> 6) as usize] as u32);
                res[1] = 0x399;
                return 2;
            } else {
                c = c
                    .wrapping_sub(code)
                    .wrapping_add(CASE_CONV_EXT[(data & 0x3f) as usize] as u32);
            }
        }
        RUN_TYPE_UF_D20 => {
            if conv_type == 1 {
                // break
            } else {
                c = data.wrapping_add(if conv_type == 2 { 0x20 } else { 0 });
            }
        }
        RUN_TYPE_UF_D1_EXT => {
            if conv_type != 1 {
                c = (CASE_CONV_EXT[data as usize] as u32).wrapping_add(if conv_type == 2 {
                    1
                } else {
                    0
                });
            }
        }
        RUN_TYPE_U_EXT | RUN_TYPE_LF_EXT => {
            if is_lower == ((type_ - RUN_TYPE_U_EXT) != 0) {
                c = CASE_CONV_EXT[data as usize] as u32;
            }
        }
        RUN_TYPE_LF_EXT2 => {
            if is_lower {
                res[0] = c
                    .wrapping_sub(code)
                    .wrapping_add(CASE_CONV_EXT[(data >> 6) as usize] as u32);
                res[1] = CASE_CONV_EXT[(data & 0x3f) as usize] as u32;
                return 2;
            }
        }
        RUN_TYPE_UF_EXT2 => {
            if conv_type != 1 {
                res[0] = c
                    .wrapping_sub(code)
                    .wrapping_add(CASE_CONV_EXT[(data >> 6) as usize] as u32);
                res[1] = CASE_CONV_EXT[(data & 0x3f) as usize] as u32;
                if conv_type == 2 {
                    res[0] = lre_case_conv1(res[0], 1);
                    res[1] = lre_case_conv1(res[1], 1);
                }
                return 2;
            }
        }
        RUN_TYPE_UF_EXT3 => {
            if conv_type != 1 {
                res[0] = CASE_CONV_EXT[(data >> 8) as usize] as u32;
                res[1] = CASE_CONV_EXT[((data >> 4) & 0xf) as usize] as u32;
                res[2] = CASE_CONV_EXT[(data & 0xf) as usize] as u32;
                if conv_type == 2 {
                    res[0] = lre_case_conv1(res[0], 1);
                    res[1] = lre_case_conv1(res[1], 1);
                    res[2] = lre_case_conv1(res[2], 1);
                }
                return 3;
            }
        }
        _ => {}
    }
    res[0] = c;
    1
}

pub fn lre_case_conv(res: &mut [u32], mut c: u32, conv_type: i32) -> usize {
    if c < 128 {
        if conv_type != 0 {
            if c >= 'A' as u32 && c <= 'Z' as u32 {
                c = c - 'A' as u32 + 'a' as u32;
            }
        } else {
            if c >= 'a' as u32 && c <= 'z' as u32 {
                c = c - 'a' as u32 + 'A' as u32;
            }
        }
    } else {
        let mut idx_min = 0;
        let mut idx_max = CASE_CONV_TABLE1.len() as isize - 1;
        while idx_min <= idx_max {
            let idx = (idx_max + idx_min) as usize / 2;
            let v = CASE_CONV_TABLE1[idx];
            let code = v >> (32 - 17);
            let len = (v >> (32 - 17 - 7)) & 0x7f;
            if c < code {
                idx_max = idx as isize - 1;
            } else if c >= code + len {
                idx_min = idx as isize + 1;
            } else {
                return lre_case_conv_entry(res, c, conv_type, idx, v);
            }
        }
    }
    res[0] = c;
    1
}

pub fn lre_is_cased(c: u32) -> bool {
    let mut res = [0u32; 3];
    if lre_case_conv(&mut res, c, 0) == 1 && res[0] != c {
        return true;
    }
    if lre_case_conv(&mut res, c, 1) == 1 && res[0] != c {
        return true;
    }
    false
}

pub fn lre_is_case_ignorable(c: u32) -> bool {
    let mut idx_min = 0;
    let mut idx_max = UNICODE_PROP_CASE_IGNORABLE_TABLE.len() as isize - 1;
    while idx_min <= idx_max {
        let idx = (idx_min + idx_max) / 2;
        let v = UNICODE_PROP_CASE_IGNORABLE_TABLE[idx as usize] as u32;
        if c < v {
            idx_max = idx - 1;
        } else if c > v {
            idx_min = idx + 1;
        } else {
            return true;
        }
    }
    false
}

pub fn unicode_from_utf8(buf: &[u8], max_len: usize, pp: &mut usize) -> u32 {
    let len = buf.len();
    if len == 0 {
        return 0;
    }
    let c = buf[0] as u32;
    if c < 0x80 {
        *pp += 1;
        return c;
    }

    let mut val: u32;
    let n: usize;

    if c < 0xE0 {
        if c < 0xC2 {
            return 0xFFFD;
        } // invalid
        val = c & 0x1F;
        n = 2;
    } else if c < 0xF0 {
        val = c & 0x0F;
        n = 3;
    } else if c < 0xF8 {
        val = c & 0x07;
        n = 4;
    } else {
        return 0xFFFD;
    }

    if n > len || n > max_len {
        return 0xFFFD;
    }

    for i in 1..n {
        let c = buf[i] as u32;
        if (c & 0xC0) != 0x80 {
            return 0xFFFD;
        }
        val = (val << 6) | (c & 0x3F);
    }

    if val > 0x10FFFF {
        return 0xFFFD;
    }
    if val >= 0xD800 && val < 0xE000 {
        return 0xFFFD;
    }

    *pp += n;
    val
}

const UNICODE_INDEX_BLOCK_LEN: usize = 32;

fn get_le24(p: &[u8]) -> u32 {
    p[0] as u32 | ((p[1] as u32) << 8) | ((p[2] as u32) << 16)
}

fn get_index_pos(pcode: &mut u32, c: u32, index_table: &[u8], index_table_len: usize) -> i32 {
    let mut idx_min = 0;
    let mut idx_max = index_table_len - 1;
    let mut code: u32;
    let mut v: u32;
    let mut idx: usize;

    v = get_le24(&index_table[0..]);
    code = v & ((1 << 21) - 1);
    if c < code {
        *pcode = 0;
        return 0;
    }

    v = get_le24(&index_table[idx_max * 3..]);
    code = v & ((1 << 21) - 1);
    if c >= code {
        return -1;
    }

    while (idx_max - idx_min) > 1 {
        idx = (idx_max + idx_min) / 2;
        v = get_le24(&index_table[idx * 3..]);
        code = v & ((1 << 21) - 1);
        if c < code {
            idx_max = idx;
        } else {
            idx_min = idx;
        }
    }
    v = get_le24(&index_table[idx_min * 3..]);
    *pcode = v & ((1 << 21) - 1);
    ((idx_min + 1) * UNICODE_INDEX_BLOCK_LEN + (v >> 21) as usize) as i32
}

fn lre_is_in_table(c: u32, table: &[u8], index_table: &[u8], index_table_len: usize) -> bool {
    let mut code: u32 = 0;
    let pos = get_index_pos(&mut code, c, index_table, index_table_len);
    if pos < 0 {
        return false;
    }
    let mut p = pos as usize;
    let mut bit = false;

    loop {
        let b = table[p] as u32;
        p += 1;
        if b < 64 {
            code += (b >> 3) + 1;
            if c < code {
                return bit;
            }
            bit = !bit;
            code += (b & 7) + 1;
            if c < code {
                return bit;
            }
            bit = !bit;
        } else if b < 96 {
            code += (b & 31) + 1;
            code += (table[p] as u32) << 5;
            p += 1;
            if c < code {
                return bit;
            }
            bit = !bit;
        } else if b < 128 {
            code += (b & 31) + 1;
            code += (table[p] as u32) << 5;
            p += 1;
            code += (table[p] as u32) << 13;
            p += 1;
            if c < code {
                return bit;
            }
            bit = !bit;
        } else {
            code += (b & 127) + 1;
            if c < code {
                return bit;
            }
            bit = !bit;
        }
    }
}

pub fn lre_is_id_start_byte(c: u8) -> bool {
    (c >= b'a' && c <= b'z') || (c >= b'A' && c <= b'Z') || c == b'$' || c == b'_'
}

pub fn lre_is_id_continue_byte(c: u8) -> bool {
    (c >= b'a' && c <= b'z')
        || (c >= b'A' && c <= b'Z')
        || (c >= b'0' && c <= b'9')
        || c == b'$'
        || c == b'_'
}

pub fn lre_is_id_start(c: u32) -> bool {
    lre_is_in_table(
        c,
        &UNICODE_PROP_ID_START_TABLE,
        &UNICODE_PROP_ID_START_INDEX,
        UNICODE_PROP_ID_START_INDEX.len() / 3,
    )
}

pub fn lre_is_id_continue(c: u32) -> bool {
    lre_is_id_start(c)
        || lre_is_in_table(
            c,
            &UNICODE_PROP_ID_CONTINUE1_TABLE,
            &UNICODE_PROP_ID_CONTINUE1_INDEX,
            UNICODE_PROP_ID_CONTINUE1_INDEX.len() / 3,
        )
}

pub fn lre_js_is_ident_first(c: u32) -> bool {
    if c < 128 {
        lre_is_id_start_byte(c as u8)
    } else {
        lre_is_id_start(c)
    }
}

pub fn lre_js_is_ident_next(c: u32) -> bool {
    if c < 128 {
        lre_is_id_continue_byte(c as u8)
    } else {
        lre_is_id_continue(c)
    }
}

pub fn is_hi_surrogate(c: u32) -> bool {
    c >= 0xD800 && c <= 0xDBFF
}

pub fn is_lo_surrogate(c: u32) -> bool {
    c >= 0xDC00 && c <= 0xDFFF
}

pub fn from_surrogate(hi: u32, lo: u32) -> u32 {
    0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)
}

pub fn unicode_to_utf8(buf: &mut [u8], c: u32) -> usize {
    if c < 0x80 {
        buf[0] = c as u8;
        1
    } else if c < 0x800 {
        buf[0] = (0xC0 | (c >> 6)) as u8;
        buf[1] = (0x80 | (c & 0x3F)) as u8;
        2
    } else if c < 0x10000 {
        buf[0] = (0xE0 | (c >> 12)) as u8;
        buf[1] = (0x80 | ((c >> 6) & 0x3F)) as u8;
        buf[2] = (0x80 | (c & 0x3F)) as u8;
        3
    } else {
        buf[0] = (0xF0 | (c >> 18)) as u8;
        buf[1] = (0x80 | ((c >> 12) & 0x3F)) as u8;
        buf[2] = (0x80 | ((c >> 6) & 0x3F)) as u8;
        buf[3] = (0x80 | (c & 0x3F)) as u8;
        4
    }
}
