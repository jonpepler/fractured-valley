//! Response curves as integer lookup tables: 17 points over an input of 0..=1,
//! linearly interpolated, all in Q16. A consideration is a named curve over
//! one reading.

use crate::units::Q16;

#[derive(Clone, Copy, Debug)]
pub struct Curve {
    pub name: &'static str,
    /// Outputs at inputs 0, 1/16, ..., 16/16, in Q16.
    pub points: [i64; 17],
}

impl Curve {
    /// A curve from 17 outputs in thousandths.
    pub const fn milli(name: &'static str, pm: [i64; 17]) -> Curve {
        let mut points = [0i64; 17];
        let mut i = 0;
        while i < 17 { points[i] = pm[i] * 65_536 / 1_000; i += 1 }
        Curve { name, points }
    }

    pub fn eval(&self, x: Q16) -> Q16 {
        let x = x.clamp01().0;
        let i = (x >> 12) as usize;
        if i >= 16 { return Q16(self.points[16]) }
        let frac = x & 0xFFF;
        let (a, b) = (self.points[i], self.points[i + 1]);
        Q16(a + (((b - a) * frac) >> 12))
    }
}
