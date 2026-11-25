//! Rust port of src/sim.ts. MUST stay bit-for-bit equivalent (see test/determinism.test.ts).
//!
//! No wasm-bindgen, no std, no allocator, no statics: JS owns the linear-memory layout
//! (see src/world.ts) and passes raw pointers, so the same module works with a plain
//! imported memory and with a shared memory used from several worker threads.
//! Raw-pointer indexing => no bounds checks (V8 keeps most of its typed-array checks).
#![no_std]

#[cfg(target_arch = "wasm32")]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

const AIR: u16 = 0;
const GRASS: u16 = 2;
const WOOD: u16 = 3;
const ASH: u16 = 4;
const SAND: u16 = 5;
const IGNITE_TEMP: u8 = 80;
const AIR_AMBIENT: u32 = 20;
const MOVED: u8 = 0x80;
const BURN_MASK: u8 = 0x7f;

#[inline(always)]
fn hash(a: u32, b: u32) -> u32 {
    let mut x = b.wrapping_mul(0x9e37_79b1) ^ a.wrapping_mul(0x85eb_ca77);
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    x
}

/// Step bricks list[0..count) for one tick. See stepBricks in sim.ts.
#[no_mangle]
pub unsafe extern "C" fn step_bricks(
    mat: *mut u16,
    tc: *const u8,
    tn: *mut u8,
    state: *mut u8,
    nb: *const i32,
    list: *const u32,
    count: u32,
    tick: u32,
) {
    for i in 0..count as usize {
        let b = *list.add(i) as usize;
        diffuse(mat, tc, tn, nb, b);
        fire(mat, tn, state, b, tick);
        gravity(mat, tn, state, b, tick);
    }
}

/// Profiling helper: like step_bricks but runs only the phases in `mask`
/// (1 = diffusion, 2 = fire, 4 = gravity). Not used for determinism checks.
#[no_mangle]
pub unsafe extern "C" fn step_bricks_phases(
    mat: *mut u16,
    tc: *const u8,
    tn: *mut u8,
    state: *mut u8,
    nb: *const i32,
    list: *const u32,
    count: u32,
    tick: u32,
    mask: u32,
) {
    for i in 0..count as usize {
        let b = *list.add(i) as usize;
        if mask & 1 != 0 { diffuse(mat, tc, tn, nb, b); }
        if mask & 2 != 0 { fire(mat, tn, state, b, tick); }
        if mask & 4 != 0 { gravity(mat, tn, state, b, tick); }
    }
}

#[inline(always)]
unsafe fn cell(mat: *const u16, tc: *const u8, tn: *mut u8, i: usize, l: u32, r: u32, d: u32, u: u32, zm: u32, zp: u32) {
    let c = *tc.add(i) as u32;
    let s = 2 * c + l + r + d + u + zm + zp;
    let mut v = (s + 4) >> 3;
    if *mat.add(i) == AIR && v > AIR_AMBIENT {
        v -= 1;
    }
    *tn.add(i) = v as u8;
}

#[inline(always)]
unsafe fn diffuse(mat: *const u16, tc: *const u8, tn: *mut u8, nb: *const i32, b: usize) {
    let base = b << 12;
    let n = |d: usize| *nb.add(b * 6 + d);
    let (n0, n1, n2, n3, n4, n5) = (n(0), n(1), n(2), n(3), n(4), n(5));
    let at = |i: usize| *tc.add(i) as u32;
    for y in 0..16usize {
        for z in 0..16usize {
            let yz = (y << 8) | (z << 4);
            let row = base + yz;
            let down = if y > 0 { row - 256 } else if n2 >= 0 { ((n2 as usize) << 12) + (15 << 8) + (z << 4) } else { row };
            let up = if y < 15 { row + 256 } else if n3 >= 0 { ((n3 as usize) << 12) + (z << 4) } else { row };
            let zm = if z > 0 { row - 16 } else if n4 >= 0 { ((n4 as usize) << 12) + (y << 8) + (15 << 4) } else { row };
            let zp = if z < 15 { row + 16 } else if n5 >= 0 { ((n5 as usize) << 12) + (y << 8) } else { row };
            let left_edge = if n0 >= 0 { at(((n0 as usize) << 12) + yz + 15) } else { at(row) };
            let right_edge = if n1 >= 0 { at(((n1 as usize) << 12) + yz) } else { at(row + 15) };

            #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
            {
                diffuse_row_simd(mat, tc, tn, row, down, up, zm, zp, left_edge, right_edge);
            }
            #[cfg(not(all(target_arch = "wasm32", target_feature = "simd128")))]
            {
                cell(mat, tc, tn, row, left_edge, at(row + 1), at(down), at(up), at(zm), at(zp));
                for x in 1..15usize {
                    cell(mat, tc, tn, row + x, at(row + x - 1), at(row + x + 1), at(down + x), at(up + x), at(zm + x), at(zp + x));
                }
                cell(mat, tc, tn, row + 15, at(row + 14), right_edge, at(down + 15), at(up + 15), at(zm + 15), at(zp + 15));
            }
        }
    }
}

/// Whole 16-cell row at once with wasm simd128 (explicit, LLVM does not auto-vectorise
/// the scalar loop). Bit-identical to the scalar `cell` maths.
#[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
#[inline(always)]
#[allow(clippy::too_many_arguments)]
unsafe fn diffuse_row_simd(
    mat: *const u16,
    tc: *const u8,
    tn: *mut u8,
    row: usize,
    down: usize,
    up: usize,
    zm: usize,
    zp: usize,
    left_edge: u32,
    right_edge: u32,
) {
    use core::arch::wasm32::*;
    let ld = |i: usize| v128_load(tc.add(i) as *const v128);
    let c = ld(row);
    // l[x] = c[x-1] (l[0] = left_edge), r[x] = c[x+1] (r[15] = right_edge)
    let le = u8x16_splat(left_edge as u8);
    let re = u8x16_splat(right_edge as u8);
    let l = i8x16_shuffle::<16, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14>(c, le);
    let r = i8x16_shuffle::<1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16>(c, re);
    let d = ld(down);
    let u = ld(up);
    let a = ld(zm);
    let p = ld(zp);
    let four = u16x8_splat(4);
    let lo = |v: v128| u16x8_extend_low_u8x16(v);
    let hi = |v: v128| u16x8_extend_high_u8x16(v);
    let sum = |f: &dyn Fn(v128) -> v128| {
        let cc = f(c);
        let s = i16x8_add(i16x8_add(cc, cc), i16x8_add(f(l), f(r)));
        let s = i16x8_add(s, i16x8_add(i16x8_add(f(d), f(u)), i16x8_add(f(a), f(p))));
        u16x8_shr(i16x8_add(s, four), 3)
    };
    let mut vlo = sum(&lo);
    let mut vhi = sum(&hi);
    // air cooling: if mat == AIR && v > 20 { v -= 1 }
    let m_lo = v128_load(mat.add(row) as *const v128);
    let m_hi = v128_load(mat.add(row + 8) as *const v128);
    let zero = u16x8_splat(0);
    let amb = u16x8_splat(AIR_AMBIENT as u16);
    // mask lanes are all-ones (= -1), so adding the mask subtracts 1
    vlo = i16x8_add(vlo, v128_and(u16x8_eq(m_lo, zero), u16x8_gt(vlo, amb)));
    vhi = i16x8_add(vhi, v128_and(u16x8_eq(m_hi, zero), u16x8_gt(vhi, amb)));
    v128_store(tn.add(row) as *mut v128, u8x16_narrow_i16x8(vlo, vhi));
}

#[inline(always)]
unsafe fn add_heat(t: *mut u8, i: usize, amount: u32) {
    let v = *t.add(i) as u32 + amount;
    *t.add(i) = if v > 255 { 255 } else { v as u8 };
}

#[inline(always)]
unsafe fn fire(mat: *mut u16, t: *mut u8, state: *mut u8, b: usize, tick: u32) {
    let base = b << 12;
    let salt = tick << 1;
    for c in 0..4096usize {
        let i = base + c;
        let m = *mat.add(i);
        if m != GRASS && m != WOOD {
            continue;
        }
        let st = *state.add(i);
        let mut s = st & BURN_MASK;
        if s == 0 {
            if *t.add(i) >= IGNITE_TEMP && (hash(salt, i as u32) & 3) != 0 {
                *state.add(i) = (st & MOVED) | 1;
            }
            continue;
        }
        s += 1;
        add_heat(t, i, if m == GRASS { 24 } else { 32 });
        if c < 15 * 256 {
            add_heat(t, i + 256, 16);
        }
        let h = hash(salt, i as u32);
        let x = c & 15;
        let z = (c >> 4) & 15;
        match (h >> 8) & 3 {
            0 => if x > 0 { add_heat(t, i - 1, 48) },
            1 => if x < 15 { add_heat(t, i + 1, 48) },
            2 => if z > 0 { add_heat(t, i - 16, 48) },
            _ => if z < 15 { add_heat(t, i + 16, 48) },
        }
        let burnout = if m == GRASS { 12 } else { 40 };
        if s >= burnout {
            *mat.add(i) = ASH;
            *state.add(i) = st & MOVED;
        } else {
            *state.add(i) = (st & MOVED) | s;
        }
    }
}

#[inline(always)]
unsafe fn gravity(mat: *mut u16, t: *mut u8, state: *mut u8, b: usize, tick: u32) {
    let base = b << 12;
    let salt = (tick << 1) | 1;
    for y in (0..16usize).rev() {
        for zx in 0..256usize {
            let c = (y << 8) | zx;
            let i = base + c;
            let st = *state.add(i);
            if st & MOVED != 0 {
                *state.add(i) = st & BURN_MASK;
                continue;
            }
            if y == 0 {
                continue;
            }
            let m = *mat.add(i);
            if m != ASH && m != SAND {
                continue;
            }
            let below = i - 256;
            let mut dest = usize::MAX;
            if *mat.add(below) == AIR {
                dest = below;
            } else {
                let h = hash(salt, i as u32);
                let x = zx & 15;
                let z = zx >> 4;
                for k in 0..2u32 {
                    let d = h.wrapping_add(2 * k) & 3;
                    let j = match d {
                        0 => if x > 0 { below - 1 } else { usize::MAX },
                        1 => if x < 15 { below + 1 } else { usize::MAX },
                        2 => if z > 0 { below - 16 } else { usize::MAX },
                        _ => if z < 15 { below + 16 } else { usize::MAX },
                    };
                    if j != usize::MAX && *mat.add(j) == AIR {
                        dest = j;
                        break;
                    }
                }
            }
            if dest != usize::MAX {
                *mat.add(dest) = m;
                *mat.add(i) = AIR;
                let tt = *t.add(dest);
                *t.add(dest) = *t.add(i);
                *t.add(i) = tt;
                *state.add(dest) = (st & BURN_MASK) | MOVED;
                *state.add(i) = 0;
            }
        }
    }
}
