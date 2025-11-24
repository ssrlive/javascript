#![allow(dead_code)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

use std::cmp::Ordering;

// String utilities

pub fn pstrcpy(buf: &mut [u8], src: &[u8]) {
    let buf_size = buf.len();
    if buf_size == 0 {
        return;
    }

    let mut i = 0;
    for &c in src {
        if c == 0 || i >= buf_size - 1 {
            break;
        }
        buf[i] = c;
        i += 1;
    }
    buf[i] = 0;
}

pub fn pstrcat(buf: &mut [u8], src: &[u8]) {
    let buf_size = buf.len();
    if buf_size == 0 {
        return;
    }

    let mut len = 0;
    while len < buf_size && buf[len] != 0 {
        len += 1;
    }

    if len < buf_size {
        pstrcpy(&mut buf[len..], src);
    }
}

pub fn strstart(str_val: &[u8], val: &[u8], ptr: Option<&mut usize>) -> bool {
    let mut p = 0;
    let mut q = 0;

    // Assuming val is null-terminated or we use its length?
    // C code: while (*q != '\0')
    while q < val.len() && val[q] != 0 {
        if p >= str_val.len() || str_val[p] != val[q] {
            return false;
        }
        p += 1;
        q += 1;
    }

    if let Some(ptr_ref) = ptr {
        *ptr_ref = p;
    }
    true
}

pub fn has_suffix(str_val: &[u8], suffix: &[u8]) -> bool {
    // C code uses strlen. We'll assume slices are the strings (excluding null if present, or including? C uses strlen so it stops at null)
    // For Rust, let's assume the slices are the valid string data.
    let len = str_val.len();
    let slen = suffix.len();

    if len < slen {
        return false;
    }

    &str_val[len - slen..] == suffix
}

// Dynamic buffer package

// In Rust, we can use Vec<u8> to replace DynBuf.
// However, to preserve the API structure:

pub struct DynBuf {
    pub buf: Vec<u8>,
    pub error: bool,
}

impl DynBuf {
    pub fn new() -> Self {
        Self {
            buf: Vec::new(),
            error: false,
        }
    }

    pub fn init(&mut self) {
        self.buf.clear();
        self.error = false;
    }

    pub fn put(&mut self, data: &[u8]) -> i32 {
        if self.error {
            return -1;
        }
        // In C, this handles realloc failure. In Rust, Vec panics on OOM usually,
        // but we can try_reserve if we want to be safe (available in newer Rust).
        // For now, just push.
        self.buf.extend_from_slice(data);
        0
    }

    pub fn put_self(&mut self, offset: usize, len: usize) -> i32 {
        if self.error {
            return -1;
        }
        if offset + len > self.buf.len() {
            return -1; // Invalid range
        }
        // We need to copy from self. Rust doesn't allow mutable borrow and immutable borrow overlap easily.
        // We can copy the range first.
        let chunk = self.buf[offset..offset + len].to_vec();
        self.buf.extend_from_slice(&chunk);
        0
    }

    pub fn putc(&mut self, c: u8) -> i32 {
        if self.error {
            return -1;
        }
        self.buf.push(c);
        0
    }

    pub fn put_u16(&mut self, val: u16) -> i32 {
        self.put(&val.to_ne_bytes()) // C code uses little endian implicitly via pointer cast?
                                     // Actually C code: dbuf_put(s, (uint8_t *)&val, 2); -> This depends on host endianness.
                                     // QuickJS usually assumes little endian or handles it?
                                     // cutils.h has put_u16 which does ((struct packed_u16 *)tab)->v = val;
                                     // This is host endian.
    }

    pub fn put_u32(&mut self, val: u32) -> i32 {
        self.put(&val.to_ne_bytes())
    }

    pub fn put_u64(&mut self, val: u64) -> i32 {
        self.put(&val.to_ne_bytes())
    }

    pub fn putstr(&mut self, str_val: &str) -> i32 {
        self.put(str_val.as_bytes())
    }

    // dbuf_printf is complex to port directly with varargs.
    // We can use std::fmt::Write if we implement it, or just a helper.
    pub fn printf(&mut self, fmt: std::fmt::Arguments) -> i32 {
        use std::io::Write;
        if self.error {
            return -1;
        }
        if self.buf.write_fmt(fmt).is_err() {
            self.error = true;
            return -1;
        }
        0
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn insert(&mut self, offset: usize, data: &[u8]) -> i32 {
        if self.error {
            return -1;
        }
        if offset > self.buf.len() {
            return -1;
        }
        let len = data.len();
        self.buf.reserve(len);
        let tail_len = self.buf.len() - offset;
        unsafe {
            let ptr = self.buf.as_mut_ptr();
            std::ptr::copy(ptr.add(offset), ptr.add(offset + len), tail_len);
            std::ptr::copy_nonoverlapping(data.as_ptr(), ptr.add(offset), len);
            self.buf.set_len(self.buf.len() + len);
        }
        0
    }

    pub fn insert_space(&mut self, offset: usize, len: usize) -> i32 {
        if self.error {
            return -1;
        }
        if offset > self.buf.len() {
            return -1;
        }
        let old_len = self.buf.len();
        self.buf.resize(old_len + len, 0);
        self.buf.copy_within(offset..old_len, offset + len);
        0
    }
}

// UTF8 utilities

pub fn unicode_to_utf8(buf: &mut [u8], c: u32) -> usize {
    // Port of unicode_to_utf8
    let mut q = 0;

    if c < 0x80 {
        buf[q] = c as u8;
        q += 1;
    } else {
        if c < 0x800 {
            buf[q] = ((c >> 6) | 0xc0) as u8;
            q += 1;
        } else {
            if c < 0x10000 {
                buf[q] = ((c >> 12) | 0xe0) as u8;
                q += 1;
            } else {
                if c < 0x00200000 {
                    buf[q] = ((c >> 18) | 0xf0) as u8;
                    q += 1;
                } else {
                    if c < 0x04000000 {
                        buf[q] = ((c >> 24) | 0xf8) as u8;
                        q += 1;
                    } else if c < 0x80000000 {
                        buf[q] = ((c >> 30) | 0xfc) as u8;
                        q += 1;
                        buf[q] = (((c >> 24) & 0x3f) | 0x80) as u8;
                        q += 1;
                    } else {
                        return 0;
                    }
                    buf[q] = (((c >> 18) & 0x3f) | 0x80) as u8;
                    q += 1;
                }
                buf[q] = (((c >> 12) & 0x3f) | 0x80) as u8;
                q += 1;
            }
            buf[q] = (((c >> 6) & 0x3f) | 0x80) as u8;
            q += 1;
        }
        buf[q] = ((c & 0x3f) | 0x80) as u8;
        q += 1;
    }
    q
}

pub fn unicode_from_utf8(p: &[u8], max_len: usize, pp: &mut usize) -> i32 {
    let mut idx = 0;
    if idx >= p.len() {
        return -1;
    }

    let c = p[idx] as u32;
    idx += 1;

    if c < 0x80 {
        *pp += 1;
        return c as i32;
    }

    let l;
    match c {
        0xc0..=0xdf => l = 1,
        0xe0..=0xef => l = 2,
        0xf0..=0xf7 => l = 3,
        0xf8..=0xfb => l = 4,
        0xfc..=0xfd => l = 5,
        _ => return -1,
    }

    if l > max_len - 1 {
        return -1;
    }

    let utf8_first_code_mask = [0x1f, 0xf, 0x7, 0x3, 0x1];
    let mut res = c & utf8_first_code_mask[l - 1];

    for _ in 0..l {
        if idx >= p.len() {
            return -1;
        }
        let b = p[idx] as u32;
        idx += 1;
        if b < 0x80 || b >= 0xc0 {
            return -1;
        }
        res = (res << 6) | (b & 0x3f);
    }

    let utf8_min_code = [0x80, 0x800, 0x10000, 0x00200000, 0x04000000];
    if res < utf8_min_code[l - 1] {
        return -1;
    }

    *pp += idx;
    res as i32
}

// Sorting
// rqsort is a complex introsort implementation.
// We will use Rust's sort_by which is also an introsort (pdqsort).

pub fn rqsort<T, F>(base: &mut [T], cmp: F)
where
    F: Fn(&T, &T) -> Ordering,
{
    base.sort_by(cmp);
}

// Helper inlines from cutils.h

pub fn max_int(a: i32, b: i32) -> i32 {
    if a > b {
        a
    } else {
        b
    }
}

pub fn min_int(a: i32, b: i32) -> i32 {
    if a < b {
        a
    } else {
        b
    }
}

pub fn max_uint32(a: u32, b: u32) -> u32 {
    if a > b {
        a
    } else {
        b
    }
}

pub fn min_uint32(a: u32, b: u32) -> u32 {
    if a < b {
        a
    } else {
        b
    }
}

pub fn max_int64(a: i64, b: i64) -> i64 {
    if a > b {
        a
    } else {
        b
    }
}

pub fn min_int64(a: i64, b: i64) -> i64 {
    if a < b {
        a
    } else {
        b
    }
}

// Bit manipulation

pub fn clz32(a: u32) -> u32 {
    a.leading_zeros()
}

pub fn clz64(a: u64) -> u32 {
    a.leading_zeros()
}

pub fn ctz32(a: u32) -> u32 {
    a.trailing_zeros()
}

pub fn ctz64(a: u64) -> u32 {
    a.trailing_zeros()
}

// Unaligned access helpers
// Rust handles unaligned access safely via read_unaligned if needed,
// but simple pointer casts can be UB.
// We will use safe wrappers.

pub fn get_u64(tab: &[u8]) -> u64 {
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&tab[0..8]);
    u64::from_ne_bytes(bytes)
}

pub fn get_i64(tab: &[u8]) -> i64 {
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&tab[0..8]);
    i64::from_ne_bytes(bytes)
}

pub fn put_u64(tab: &mut [u8], val: u64) {
    let bytes = val.to_ne_bytes();
    tab[0..8].copy_from_slice(&bytes);
}

pub fn get_u32(tab: &[u8]) -> u32 {
    let mut bytes = [0u8; 4];
    bytes.copy_from_slice(&tab[0..4]);
    u32::from_ne_bytes(bytes)
}

pub fn get_i32(tab: &[u8]) -> i32 {
    let mut bytes = [0u8; 4];
    bytes.copy_from_slice(&tab[0..4]);
    i32::from_ne_bytes(bytes)
}

pub fn put_u32(tab: &mut [u8], val: u32) {
    let bytes = val.to_ne_bytes();
    tab[0..4].copy_from_slice(&bytes);
}

pub fn get_u16(tab: &[u8]) -> u16 {
    let mut bytes = [0u8; 2];
    bytes.copy_from_slice(&tab[0..2]);
    u16::from_ne_bytes(bytes)
}

pub fn get_i16(tab: &[u8]) -> i16 {
    let mut bytes = [0u8; 2];
    bytes.copy_from_slice(&tab[0..2]);
    i16::from_ne_bytes(bytes)
}

pub fn put_u16(tab: &mut [u8], val: u16) {
    let bytes = val.to_ne_bytes();
    tab[0..2].copy_from_slice(&bytes);
}

pub fn get_u8(tab: &[u8]) -> u8 {
    tab[0]
}

pub fn get_i8(tab: &[u8]) -> i8 {
    tab[0] as i8
}

pub fn put_u8(tab: &mut [u8], val: u8) {
    tab[0] = val;
}

// Floating point helpers

pub fn float64_as_uint64(d: f64) -> u64 {
    d.to_bits()
}

pub fn uint64_as_float64(u: u64) -> f64 {
    f64::from_bits(u)
}

// FP16 conversion
// This is complex bit manipulation.

pub fn fromfp16(v: u16) -> f64 {
    let v1 = (v & 0x7fff) as u32;
    let v1 = if v1 >= 0x7c00 { v1 + 0x1f8000 } else { v1 };
    let u = ((v as u64 >> 15) << 63) | ((v1 as u64) << (52 - 10));
    let d = uint64_as_float64(u);
    d * 2f64.powi(1008) // 0x1p1008
}

pub fn tofp16(d: f64) -> u16 {
    let a_raw = float64_as_uint64(d);
    let sgn = (a_raw >> 63) as u32;
    let mut a = a_raw & 0x7fffffffffffffff;

    let mut v;
    if a > 0x7ff0000000000000 {
        // nan
        v = 0x7c01;
    } else if a < 0x3f10000000000000 {
        // 0x1p-14
        // subnormal f16 number or zero
        if a <= 0x3e60000000000000 {
            // 0x1p-25
            v = 0x0000;
        } else {
            let shift = 1051 - (a >> 52);
            a = (1u64 << 52) | (a & ((1u64 << 52) - 1));
            let addend = ((a >> shift) & 1) + ((1u64 << (shift - 1)) - 1);
            v = ((a + addend) >> shift) as u32;
        }
    } else {
        // normal number or infinity
        a -= 0x3f00000000000000;
        let addend = ((a >> (52 - 10)) & 1) + ((1u64 << (52 - 11)) - 1);
        v = ((a + addend) >> (52 - 10)) as u32;
        if v > 0x7c00 {
            v = 0x7c00;
        }
    }
    (v | (sgn << 15)) as u16
}

pub fn isfp16nan(v: u16) -> bool {
    (v & 0x7FFF) > 0x7C00
}

pub fn isfp16zero(v: u16) -> bool {
    (v & 0x7FFF) == 0
}
