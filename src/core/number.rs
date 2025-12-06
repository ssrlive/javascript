// Shared numeric conversion helpers used by core evaluation

const TWO_32: f64 = 2_i64.pow(32) as f64; // 2^32

/// JS ToInt32 semantics for Number inputs
pub(crate) fn to_int32(n: f64) -> i32 {
    if !n.is_finite() || n == 0.0 {
        return 0;
    }
    let int = n.trunc();
    let int32bit = ((int % TWO_32) + TWO_32) % TWO_32;
    if int32bit >= TWO_32 / 2.0 {
        (int32bit - TWO_32) as i32
    } else {
        int32bit as i32
    }
}

/// JS ToUint32 semantics for Number inputs
pub(crate) fn to_uint32(n: f64) -> u32 {
    if !n.is_finite() {
        return 0_u32;
    }
    let int = n.trunc();
    let u = ((int % TWO_32) + TWO_32) % TWO_32;
    u as u32
}
