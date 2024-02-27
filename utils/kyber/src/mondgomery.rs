use crate::params::Q;

pub const QINV: i16 = -3327;

pub fn reduce(a: i32) -> i16 {
    let t = (a as i16).wrapping_mul(QINV);
    let t = (a - i32::from(t) * Q as i32) >> 16;
    t as i16
}
