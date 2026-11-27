// The reaction phase (chem::react_in / react_cell) on the brick pool, one
// thread per cell, line for line with fv-core: the front in fine fuel, heat
// between gas, surface and core, water (boiling, evaporating, dew, mist),
// pyrolysis, char, flame and soot. Every quantity is the same integer as on
// the CPU. Each cell reads and writes only itself.
//
// Read from `table` after the outside cell: the substances (SUB_WORDS
// each), then the tables (offsets in RP). Each cell's pilot flag (a flame
// reaches its gas, chem::flame_reach) is a bit in `aux`.

@group(0) @binding(8) var<storage, read> aux: array<u32>;

// Where things are in `table` (set by the host).
struct ReactParams { subs: u32, heating: u32, pyro: u32, char_b: u32, gas_b: u32, soot_b: u32, sat: u32, surface_g: u32, n_sigma: u32, solid_base: u32, pad0: u32, pad1: u32 }
@group(0) @binding(9) var<uniform> RP: ReactParams;

const SUB_WORDS: u32 = 16u;
const MAX_K: u32 = 6000u;
const VOLATILE: u32 = 0u;
const CHAR: u32 = 1u;
const MINERAL: u32 = 2u;
const O2: u32 = 0u;
const CO2: u32 = 2u;
const VAP: u32 = 3u;
const FUEL: u32 = 4u;
const SOOT: u32 = 5u;
const DROPS: u32 = 6u;
const ONE_F: I = I(0u, 1u); // Fraction::ONE

// A cell, as react_cell sees it.
struct C {
    substance: u32, bonds: u32, front: u32, lit: bool,
    solid: array<I, 3>, water: I, gas: array<I, 7>,
    energy: I, solid_energy: I, skin: I, nuclear: I, flame: I,
}
// A substance's fields that the reaction reads.
struct S { full_mass: I, porosity: I, permeable: bool, sigma: I, comp_vol: I, sh: I, conductivity: I, volatile_chemical: I }

fn tw(o: u32) -> I { return I(table[o], table[o + 1u]); }
fn subst(id: u32) -> S {
    let o = P.outside + 20u + SS + id * SUB_WORDS;
    var s: S;
    s.full_mass = tw(o); s.porosity = tw(o + 2u); s.permeable = table[o + 4u] != 0u; s.sigma = tw(o + 5u);
    s.comp_vol = tw(o + 7u); s.sh = tw(o + 9u); s.conductivity = tw(o + 11u); s.volatile_chemical = tw(o + 13u);
    return s;
}
fn rate(base: u32, k: u32) -> I { return tw(base + 2u * k); }

// ── the solid group (single-buffered: only reaction changes it) ─────────
// Per slot, 14 fields × 512 cells: mbits (substance | bonds << 8 | front << 16),
// lit, then solid[3], water, nuclear, flame as i64.
fn sg(c: u32, f: u32) -> u32 { return RP.solid_base + (c / 512u * 14u + f) * 512u + c % 512u; }

fn load(c: u32) -> C {
    var x: C;
    let mbits = state[sg(c, 0u)];
    x.substance = mbits & 0xffu; x.bonds = (mbits >> 8u) & 0xffu; x.front = mbits >> 16u;
    x.lit = state[sg(c, 1u)] != 0u;
    for (var j = 0u; j < 3u; j++) { x.solid[j] = I(state[sg(c, 2u + 2u * j)], state[sg(c, 3u + 2u * j)]); }
    x.water = I(state[sg(c, 8u)], state[sg(c, 9u)]);
    x.nuclear = I(state[sg(c, 10u)], state[sg(c, 11u)]);
    x.flame = I(state[sg(c, 12u)], state[sg(c, 13u)]);
    for (var k = 0u; k < 7u; k++) { x.gas[k] = gas_at(P.in_copy, c, k); }
    x.energy = heat_at(P.in_copy, c, 0u); x.solid_energy = heat_at(P.in_copy, c, 1u); x.skin = heat_at(P.in_copy, c, 2u);
    return x;
}
fn store(c: u32, x: C) {
    state[sg(c, 0u)] = x.substance | (x.bonds << 8u) | (x.front << 16u);
    state[sg(c, 1u)] = select(0u, 1u, x.lit);
    for (var j = 0u; j < 3u; j++) { state[sg(c, 2u + 2u * j)] = x.solid[j].x; state[sg(c, 3u + 2u * j)] = x.solid[j].y; }
    state[sg(c, 8u)] = x.water.x; state[sg(c, 9u)] = x.water.y;
    state[sg(c, 10u)] = x.nuclear.x; state[sg(c, 11u)] = x.nuclear.y;
    state[sg(c, 12u)] = x.flame.x; state[sg(c, 13u)] = x.flame.y;
    for (var k = 0u; k < 7u; k++) { put_gas(P.out_copy, c, k, x.gas[k]); }
    let slot = c / 512u; let l = c % 512u;
    let ob = P.out_copy * P.copy_words + P.heat_base;
    var hs = array<I, 3>(x.energy, x.solid_energy, x.skin);
    for (var f = 0u; f < 3u; f++) { let o = ob + 2u * ((slot * 3u + f) * 512u + l); state[o] = hs[f].x; state[o + 1u] = hs[f].y; }
}

// ── units ─────────────────────────────────────────────────────────────────
fn cap_t(c: I, t: I) -> I { return mul_div_1e6(c, t); }           // HeatCapacity × Temperature (or ΔT)
fn e_over_c(e: I, c: I) -> I { if (!gt0(c)) { return I(0u, 0u); } return mul_div(e, i_(1000000), c); } // Energy / HeatCapacity
fn g_dt_tick(g: I, dt: I) -> I { return heat(g, dt); }           // (Conductance × ΔT) × TICK
fn of_f(f: I, m: I) -> I { let w = mulw(m, f); return I(w.y, w.z); } // Fraction::of_mass / of_energy

// ── a cell's derived quantities (cell.rs) ─────────────────────────────────
fn solid_mass(x: C) -> I { return add(add(x.solid[0], x.solid[1]), x.solid[2]); }
fn has_solid(x: C) -> bool { return gt0(solid_mass(x)) || gt0(x.water); }
fn open_water(x: C) -> bool { return x.substance == 0u && gt0(x.water); }
fn gas_capacity(x: C) -> I { var c = I(0u, 0u); for (var k = 0u; k < 7u; k++) { c = add(c, muli(x.gas[k], C_GAS_SH[k])); } return c; }
fn solid_capacity(x: C, s: S) -> I {
    return add(add(mul(add(x.solid[VOLATILE], x.solid[MINERAL]), s.sh), muli(x.solid[CHAR], C_CHAR_SH)), muli(x.water, C_WATER_SH));
}
fn latent(x: C) -> I { return muli(x.gas[VAP], C_LATENT); }
fn solid_share(x: C, s: S) -> I {
    if (iszero(s.full_mass)) { return I(0u, 0u); }
    let filled = sub(ONE_F, s.porosity);
    let share = wdiv(mulw(filled, mx(solid_mass(x), I(0u, 0u))), widen(s.full_mass));
    // clamp(0, ONE)
    if (wisneg(share)) { return I(0u, 0u); }
    if (share.z != 0u || share.w != 0u || ult(ONE_F, share.xy)) { return ONE_F; }
    return share.xy;
}
fn share_of(f: I, v: i32) -> I { return of_f(f, i_(v)); }
fn gas_volume_ml(x: C, s: S) -> I {
    var water_ml = I(0u, 0u);
    if (open_water(x)) { water_ml = mn(divi(x.water, 1000000), i_(C_CELL_VOLUME_ML)); }
    return sub(sub(i_(C_CELL_VOLUME_ML), share_of(solid_share(x, s), C_CELL_VOLUME_ML)), water_ml);
}
fn permeable(x: C, s: S) -> bool { return s.permeable || ult(solid_share(x, s), I(0x0cccccccu, 0u)); } // ONE / 20 = 214 748 364
fn opaque(x: C, s: S) -> bool { return !permeable(x, s); }
fn heating_permille(sigma: I) -> I { let k = min(sigma.x, 20000u); return I(table[RP.heating + k], 0u); }
fn skin_capacity(x: C, s: S) -> I {
    if (opaque(x, s)) { return divi(muli(solid_capacity(x, s), C_SKIN_PERMILLE), 1000); }
    if (!gt0(s.sigma) || iszero(solid_mass(x))) { return I(0u, 0u); }
    return divi(mul(solid_capacity(x, s), heating_permille(s.sigma)), 1000);
}
fn core_capacity(x: C, s: S) -> I { return sub(solid_capacity(x, s), skin_capacity(x, s)); }
fn gas_temp(x: C) -> I { let c = gas_capacity(x); if (!gt0(c)) { return I(0u, 0u); } return e_over_c(sub(x.energy, latent(x)), c); }
fn solid_temperature(x: C, s: S) -> I { let c = core_capacity(x, s); if (!gt0(c)) { return gas_temp(x); } return e_over_c(x.solid_energy, c); }
fn surface_temperature(x: C, s: S) -> I { let c = skin_capacity(x, s); if (!gt0(c)) { return solid_temperature(x, s); } return e_over_c(x.skin, c); }
fn inner_surface_mm2(x: C, s: S) -> I { return mul(s.sigma, share_of(solid_share(x, s), C_CELL_VOLUME_ML)); }
// surface_store: an opaque cell's surface layer, an airy cell's solid if it
// has one, else its gas. (0: skin, 1: solid, 2: gas.)
fn surface_store(x: C, s: S) -> u32 { if (gt0(skin_capacity(x, s))) { return 0u; } if (has_solid(x)) { return 1u; } return 2u; }
fn add_surface(x: ptr<function, C>, s: S, e: I) {
    let w = surface_store(*x, s);
    if (w == 0u) { (*x).skin = add((*x).skin, e); } else if (w == 1u) { (*x).solid_energy = add((*x).solid_energy, e); } else { (*x).energy = add((*x).energy, e); }
}
fn fine_fuel(x: C, s: S) -> bool { return s.permeable && x.substance != 0u; }

// ── tables ───────────────────────────────────────────────────────────────
fn k_index(t: I) -> u32 {
    if (isneg(t)) { return 0u; }
    let k = div(t, i_(1000));
    if (k.y != 0u || k.x > MAX_K) { return MAX_K; }
    return k.x;
}
fn saturated_at(t: I) -> I {
    let k = k_index(t);
    let lo = tw(RP.sat + 2u * k);
    let hi = tw(RP.sat + 2u * min(k + 1u, MAX_K));
    // i64::MAX / 8 = 0x0fff…: "unlimited" from boiling up.
    if (!lt(hi, I(0xffffffffu, 0x0fffffffu))) { return lo; }
    let frac = mn(mx(sub(t, muli(i_(i32(k)), 1000)), I(0u, 0u)), i_(1000));
    return add(lo, narrow(wdiv(mulw(sub(hi, lo), frac), widen(i_(1000)))));
}
// saturating_mul by a positive amount (an i64 product, or i64::MAX).
fn sat_mul(a: I, b: I) -> I {
    let w = mulw(a, b);
    let s = widen(w.xy);
    if (s.x == w.x && s.y == w.y && s.z == w.z && s.w == w.w) { return w.xy; }
    return select(I(0xffffffffu, 0x7fffffffu), I(0u, 0x80000000u), wisneg(w));
}
fn surface_h(sigma: I, t_gas: I, ambient: I) -> I {
    if (!gt0(sigma)) { return i_(C_SURFACE_H_PER_SIGMA); }
    let v = mx(buoyant_churn(t_gas, ambient), I(0u, 0u));
    var row = 0u;
    for (var j = 0u; j < RP.n_sigma; j++) { if (eq(tw(RP.surface_g + j * (2u + MAX_K + 1u)), sigma)) { row = RP.surface_g + j * (2u + MAX_K + 1u) + 2u; } }
    let g = I(table[row + k_index(mx(t_gas, i_(250000)))], 0u);
    let sqrt_v = isqrt(I(0u, v.x)); // ⌊√(v·2³²)⌋, v < 2³²
    let term = umulw(sqrt_v, g);    // ×2⁻⁸ below
    let t8 = W((term.x >> 8u) | (term.y << 24u), (term.y >> 8u) | (term.z << 24u), (term.z >> 8u) | (term.w << 24u), term.w >> 8u);
    var nus = wadd(W(1288490189u, 0u, 0u, 0u), t8);
    if (wult(nus, W(0u, 2u, 0u, 0u))) { nus = W(0u, 2u, 0u, 0u); }
    let hw = wadd(umulw(I(u32(C_SURFACE_H_PER_SIGMA), 0u), nus.xy), W(0u, 1u, 0u, 0u));
    return I((hw.y >> 1u) | (hw.z << 31u), (hw.z >> 1u) | (hw.w << 31u));
}
fn water_surface_h(wind: I) -> I { return add(i_(5700), divi(muli(mn(mx(wind, I(0u, 0u)), i_(5000)), 38), 10)); }
fn water_swept_ul(wind: I) -> I {
    let h_m = divi(muli(water_surface_h(wind), 1000), 1077);
    return divi(muli(muli(h_m, C_FACE_MM2), C_TICK), 1000000);
}

// ── chem.rs, line for line ───────────────────────────────────────────────
fn ignite_energy(x: C, s: S, ambient: I) -> I {
    let skin = skin_capacity(x, s);
    let water = narrow(wdiv(mulw(x.water, skin), widen(mx(solid_capacity(x, s), i_(1)))));
    return mx(add(mx(cap_t(skin, sub(i_(C_IGNITION), ambient)), I(0u, 0u)), muli(water, C_LATENT)), i_(1));
}
fn advance_front(x: ptr<function, C>, s: S, ambient: I) {
    if ((*x).front >= 65535u || !fine_fuel(*x, s) || !gt0((*x).solid[VOLATILE])) { return; }
    let skin = skin_capacity(*x, s);
    let held = mx(sub((*x).skin, cap_t(skin, ambient)), I(0u, 0u));
    let need = ignite_energy(*x, s, ambient);
    // β·σ: σ × (10⁹ − porosity·10⁹/2³²) / 10⁹, at least 1.
    let solid_part = sub(i_(1000000000), narrow(wdiv(mulw(s.porosity, i_(1000000000)), W(0u, 1u, 0u, 0u))));
    let beta_sigma = mx(narrow(wdiv(mulw(s.sigma, solid_part), widen(i_(1000000000)))), i_(1));
    let slab = mn(narrow(wdiv(widen(i_(65535 * 1000)), mulw(beta_sigma, i_(C_CELL_MM)))), i_(65535));
    let share = mn(narrow(wdiv(mulw(held, i_(65535)), widen(need))), i_(65535));
    if (!lt(share, slab)) { (*x).front = max((*x).front, share.x); }
}
fn fresh_o2(ml: I) -> I { return divi(muli(ml, C_FRESH_O2), C_CELL_VOLUME_ML); }
fn solid_to_gas(x: ptr<function, C>, s: S, m: I, sh: I, t: I) {
    let e = cap_t(mul(m, sh), t);
    add_surface(x, s, neg_(e));
    (*x).energy = add((*x).energy, e);
}
fn boil_from_core(x: ptr<function, C>, dm: I, t: I) {
    if (!gt0(dm)) { return; }
    let e = add(cap_t(muli(dm, C_GAS_SH[VAP]), t), muli(dm, C_LATENT));
    (*x).water = sub((*x).water, dm);
    (*x).gas[VAP] = add((*x).gas[VAP], dm);
    (*x).solid_energy = sub((*x).solid_energy, e);
    (*x).energy = add((*x).energy, e);
}
fn evaporate(x: ptr<function, C>, s: S, dm: I, ts: I) {
    if (!gt0(dm)) { return; }
    let e = add(cap_t(muli(dm, C_GAS_SH[VAP]), ts), muli(dm, C_LATENT));
    (*x).water = sub((*x).water, dm);
    (*x).gas[VAP] = add((*x).gas[VAP], dm);
    add_surface(x, s, neg_(e));
    (*x).energy = add((*x).energy, e);
}
fn condense(x: ptr<function, C>, s: S, dm: I) {
    if (!gt0(dm)) { return; }
    let e = add(cap_t(muli(dm, C_GAS_SH[VAP]), gas_temp(*x)), muli(dm, C_LATENT));
    (*x).gas[VAP] = sub((*x).gas[VAP], dm);
    (*x).water = add((*x).water, dm);
    (*x).energy = sub((*x).energy, e);
    add_surface(x, s, e);
}
fn pyrolysis_rate(x: C, s: S) -> I {
    if (!gt0(x.solid[VOLATILE])) { return I(0u, 0u); }
    let full = divi(mul(s.full_mass, s.comp_vol), 1000);
    var cap = I(0u, 0u);
    if (permeable(x, s)) {
        cap = narrow(wdiv(wdiv(mulw(full, mul(mx(s.sigma, i_(1)), i_(C_TICK))), widen(i_(C_RESIDENCE))), widen(i_(1000))));
    } else {
        cap = narrow(wdiv(wdiv(wdiv(mulw(full, i_(C_CHAR_UM_PER_MIN * C_TICK)), widen(i_(60000))), widen(i_(C_CELL_MM))), widen(i_(1000))));
    }
    let kinetic = mn(of_f(rate(RP.pyro, k_index(surface_temperature(x, s))), x.solid[VOLATILE]), mx(cap, i_(1)));
    var reached = I(0u, 0u);
    if (fine_fuel(x, s) && x.front > 0u) { reached = narrow(wdiv(mulw(cap, i_(i32(x.front))), widen(i_(65535)))); }
    return mn(mx(kinetic, reached), x.solid[VOLATILE]);
}
struct Holds { holds: bool, could: I, anchored: I }
fn flame_holds(x: C, s: S, pilot: bool) -> Holds {
    let tg = gas_temp(x);
    let could = mn(x.gas[FUEL], divi(muli(x.gas[O2], 1000), C_O2_PER_FUEL));
    let o2_needed = narrow(wdiv(mulw(could, i_(C_O2_PER_FUEL)), widen(i_(1000))));
    let air_cap = sub(gas_capacity(x), muli(x.gas[FUEL], C_GAS_SH[FUEL]));
    let sheet_cap = add(muli(could, C_GAS_SH[FUEL]), narrow(wdiv(mulw(air_cap, o2_needed), widen(mx(x.gas[O2], i_(1))))));
    let rise = e_over_c(muli(could, C_GAS_CHEM_FUEL), mx(sheet_cap, i_(1)));
    var anchored = tg;
    if (has_solid(x)) { anchored = mx(tg, surface_temperature(x, s)); }
    var h: Holds;
    h.holds = (!lt(anchored, i_(C_IGNITION)) || pilot) && !lt(add(tg, rise), i_(C_CRITICAL_FLAME));
    h.could = could; h.anchored = anchored;
    return h;
}

fn react_inner(x: ptr<function, C>, s: S, ambient: I, pilot: bool, wind: I) {
    (*x).lit = false;
    advance_front(x, s, ambient);
    let ml = gas_volume_ml(*x, s);

    // Solid and gas trade heat across the solid's surface.
    if (has_solid(*x) && gt0(gas_capacity(*x))) {
        let porous_skin = !opaque(*x, s) && gt0(skin_capacity(*x, s));
        let cs = select(core_capacity(*x, s), skin_capacity(*x, s), porous_skin);
        let cg = gas_capacity(*x);
        let ts = select(solid_temperature(*x, s), surface_temperature(*x, s), porous_skin);
        let tg = gas_temp(*x);
        let h = mul(surface_h(s.sigma, tg, ambient), s.sigma);
        var g = I(0u, 0u);
        if (open_water(*x)) { g = divi(muli(water_surface_h(wind), C_FACE_MM2), 1000); }
        else { g = narrow(wdiv(mulw(h, inner_surface_mm2(*x, s)), widen(i_(1000)))); }
        var q = g_dt_tick(g, sub(tg, ts));
        let held = select((*x).solid_energy, (*x).skin, porous_skin);
        let shared_t = e_over_c(add(sub((*x).energy, latent(*x)), held), add(cs, cg));
        let limit = sub(sub((*x).energy, latent(*x)), cap_t(cg, shared_t));
        if (lt(abs_(limit), abs_(q)) || !permeable(*x, s)) { q = limit; }
        if (porous_skin) { (*x).skin = add((*x).skin, q); } else { (*x).solid_energy = add((*x).solid_energy, q); }
        (*x).energy = sub((*x).energy, q);
    }
    // An opaque cell's surface layer and core trade heat.
    if (gt0(skin_capacity(*x, s)) && gt0(core_capacity(*x, s))) {
        var g = I(0u, 0u);
        if (opaque(*x, s)) { g = muli(s.conductivity, 2 * C_CELL_MM); }
        else {
            let e = heating_permille(s.sigma);
            // k × σ × A × (1000 − e) / 4·10⁶, through 128 bits.
            let ka = mulw(mul(s.conductivity, s.sigma), inner_surface_mm2(*x, s));
            g = narrow(wdiv(wdiv(wmul_small(ka, sub(i_(1000), e)), widen(i_(4000))), widen(i_(1000))));
        }
        let tk = surface_temperature(*x, s); let tc = solid_temperature(*x, s);
        var q = g_dt_tick(g, sub(tk, tc));
        let ck = skin_capacity(*x, s); let cc = core_capacity(*x, s);
        let shared_t = e_over_c(add((*x).skin, (*x).solid_energy), add(ck, cc));
        let limit = sub((*x).skin, cap_t(ck, shared_t));
        if (lt(abs_(limit), abs_(q))) { q = limit; }
        (*x).solid_energy = add((*x).solid_energy, q);
        (*x).skin = sub((*x).skin, q);
    }
    if (!gt0(ml)) { return; }

    // Water.
    var ts = surface_temperature(*x, s);
    let per_ug = add(i_(C_LATENT), cap_t(i_(C_WATER_SH), i_(C_BOIL)));
    let skin_cap0 = skin_capacity(*x, s);
    if (gt0((*x).water) && lt(i_(C_BOIL), ts) && !opaque(*x, s)) {
        var scap = skin_capacity(*x, s); if (!gt0(scap)) { scap = solid_capacity(*x, s); }
        let excess = cap_t(scap, sub(ts, i_(C_BOIL)));
        var held = (*x).water;
        if (gt0(skin_cap0)) { held = narrow(wdiv(mulw((*x).water, skin_cap0), widen(mx(solid_capacity(*x, s), i_(1))))); }
        let dm = mn(mn(div(excess, mx(per_ug, i_(1))), mx(held, i_(1))), (*x).water);
        evaporate(x, s, dm, ts);
    }
    let tc = solid_temperature(*x, s);
    if (gt0((*x).water) && gt0(skin_cap0) && lt(i_(C_BOIL), tc)) {
        let excess = cap_t(core_capacity(*x, s), sub(tc, i_(C_BOIL)));
        let dm = mn(div(excess, mx(per_ug, i_(1))), (*x).water);
        boil_from_core(x, dm, tc);
    }
    ts = surface_temperature(*x, s);
    var sat = divi(sat_mul(saturated_at(ts), ml), 1000);
    if (gt0((*x).water) && lt(ts, i_(C_BOIL)) && lt((*x).gas[VAP], sat)) {
        let room = sub(sat, (*x).gas[VAP]);
        let speed = mx(mn(s.sigma, i_(C_FINE_SIGMA)), i_(1));
        let skin_cap = skin_capacity(*x, s);
        var held = (*x).water;
        if (gt0(skin_cap)) { held = mx(narrow(wdiv(mulw((*x).water, skin_cap), widen(mx(solid_capacity(*x, s), i_(1))))), i_(1)); }
        var dm = I(0u, 0u);
        if (open_water(*x)) {
            let air = narrow(wdiv(mulw((*x).gas[VAP], i_(1000)), widen(ml)));
            let sat_l = saturated_at(ts);
            dm = mn(narrow(wdiv(wadd(mulw(mx(sub(sat_l, air), I(0u, 0u)), water_swept_ul(wind)), widen(i_(500000))), widen(i_(1000000)))), room);
        } else {
            dm = narrow(wdiv(mulw(room, speed), widen(i_(C_FINE_SIGMA * 4))));
        }
        dm = mn(mn(dm, held), (*x).water);
        evaporate(x, s, dm, ts);
    } else if (lt(ts, i_(C_BOIL)) && lt(sat, (*x).gas[VAP]) && (open_water(*x) || gt0(solid_mass(*x)))) {
        let excess = sub((*x).gas[VAP], sat);
        let speed = mx(mn(s.sigma, i_(C_FINE_SIGMA)), i_(1));
        var dm = I(0u, 0u);
        if (open_water(*x)) {
            let air = narrow(wdiv(mulw((*x).gas[VAP], i_(1000)), widen(ml)));
            let sat_l = saturated_at(ts);
            dm = narrow(wdiv(wadd(mulw(mx(sub(air, sat_l), I(0u, 0u)), water_swept_ul(wind)), widen(i_(500000))), widen(i_(1000000))));
        } else {
            dm = narrow(wdiv(mulw(excess, speed), widen(i_(C_FINE_SIGMA * 4))));
        }
        condense(x, s, mn(dm, excess));
    }
    if ((*x).substance == 0u && iszero((*x).water) && iszero(solid_mass(*x)) && !iszero((*x).solid_energy)) {
        (*x).energy = add((*x).energy, (*x).solid_energy);
        (*x).solid_energy = I(0u, 0u);
    }
    sat = divi(sat_mul(saturated_at(gas_temp(*x)), ml), 1000);
    let surface_takes = (open_water(*x) || gt0(solid_mass(*x))) && le(ts, gas_temp(*x));
    if (lt(sat, (*x).gas[VAP]) && !surface_takes) {
        let dm = divi(sub((*x).gas[VAP], sat), 2);
        (*x).gas[VAP] = sub((*x).gas[VAP], dm);
        (*x).gas[DROPS] = add((*x).gas[DROPS], dm);
    }
    if (gt0((*x).gas[DROPS]) && lt((*x).gas[VAP], sat)) {
        let dm = mn(divi(sub(sat, (*x).gas[VAP]), 2), (*x).gas[DROPS]);
        (*x).gas[DROPS] = sub((*x).gas[DROPS], dm);
        (*x).gas[VAP] = add((*x).gas[VAP], dm);
    }
    ts = surface_temperature(*x, s);
    let ks = k_index(ts);

    // Pyrolysis.
    if (gt0((*x).solid[VOLATILE])) {
        let dm = pyrolysis_rate(*x, s);
        if (gt0(dm)) {
            solid_to_gas(x, s, dm, s.sh, ts);
            (*x).solid[VOLATILE] = sub((*x).solid[VOLATILE], dm);
            (*x).gas[FUEL] = add((*x).gas[FUEL], dm);
            add_surface(x, s, neg_(sub(muli(dm, C_GAS_CHEM_FUEL), mul(dm, s.volatile_chemical))));
        }
    }
    let fresh = mx(fresh_o2(ml), i_(1));

    // Char.
    if (gt0((*x).solid[CHAR]) && gt0((*x).gas[O2])) {
        let kinetic = div(mul(of_f(rate(RP.char_b, ks), (*x).solid[CHAR]), mn((*x).gas[O2], fresh)), fresh);
        var area = i_(C_FACE_MM2);
        if (permeable(*x, s)) { area = inner_surface_mm2(*x, s); }
        let o2_reach = narrow(wdiv(wdiv(mulw(area, (*x).gas[O2]), widen(mx(ml, i_(1)))), widen(i_(400))));
        let diffusion = divi(muli(o2_reach, 2 * C_O2_PER_C_DEN), C_O2_PER_C_NUM);
        let want = mn(kinetic, diffusion);
        let dm = mn(want, divi(muli((*x).gas[O2], 2 * C_O2_PER_C_DEN), C_O2_PER_C_NUM));
        if (gt0(dm)) {
            let ox = divi(muli(dm, C_O2_PER_C_NUM), 2 * C_O2_PER_C_DEN);
            solid_to_gas(x, s, dm, i_(C_CHAR_SH), ts);
            (*x).solid[CHAR] = sub((*x).solid[CHAR], dm);
            (*x).gas[O2] = sub((*x).gas[O2], ox);
            let co_energy = sub(muli(dm, C_CHAR_CHEM), muli(dm, C_CHAR_TO_CO));
            let as_fuel = mn(div(co_energy, i_(C_GAS_CHEM_FUEL)), add(dm, ox));
            (*x).gas[FUEL] = add((*x).gas[FUEL], as_fuel);
            (*x).gas[CO2] = add((*x).gas[CO2], sub(add(dm, ox), as_fuel));
            add_surface(x, s, sub(muli(dm, C_CHAR_CHEM), muli(as_fuel, C_GAS_CHEM_FUEL)));
        }
    }

    // Flame.
    if (gt0((*x).gas[FUEL]) && gt0((*x).gas[O2]) && permeable(*x, s)) {
        let fh = flame_holds(*x, s, pilot);
        let kinetic = of_f(rate(RP.gas_b, k_index(fh.anchored)), (*x).gas[FUEL]);
        var mixable = of_f(I(C_MIXING, 0u), fh.could);
        if (fine_fuel(*x, s) && gt0((*x).solid[VOLATILE]) && (*x).front < 65535u && lt(surface_temperature(*x, s), i_(C_IGNITION))) {
            mixable = narrow(wdiv(mulw(mixable, i_(i32((*x).front))), widen(i_(65535))));
        }
        let mixed = select(I(0u, 0u), mixable, fh.holds);
        let want = select(mn(kinetic, mixable), mixed, fh.holds);
        let starved = sub(i_(1000), mn(div(muli((*x).gas[O2], 1000), fresh), i_(1000)));
        let soot_pm = add(i_(10), divi(muli(starved, 90), 1000));
        let by_o2 = div(muli(divi(muli((*x).gas[O2], 1000), C_O2_PER_FUEL), 1000), mx(sub(i_(1000), soot_pm), i_(1)));
        let dm = mn(want, by_o2);
        if (gt0(dm)) {
            let soot = divi(mul(dm, soot_pm), 1000);
            let burnt = sub(dm, soot);
            let ox = mn(divi(muli(burnt, C_O2_PER_FUEL), 1000), (*x).gas[O2]);
            let products = add(burnt, ox);
            let co2 = divi(muli(products, C_CO2_SHARE), 1000);
            (*x).gas[FUEL] = sub((*x).gas[FUEL], dm);
            (*x).gas[O2] = sub((*x).gas[O2], ox);
            (*x).gas[SOOT] = add((*x).gas[SOOT], soot);
            (*x).gas[CO2] = add((*x).gas[CO2], co2);
            (*x).gas[VAP] = add((*x).gas[VAP], sub(products, co2));
            let heat_r = sub(muli(dm, C_GAS_CHEM_FUEL), muli(soot, C_GAS_CHEM_SOOT));
            let area = inner_surface_mm2(*x, s);
            let share = narrow(wdiv(mulw(I(C_FEEDBACK_MAX, 0u), area), wadd(widen(area), widen(i_(C_FEEDBACK_HALF)))));
            var to_solid = I(0u, 0u);
            if (has_solid(*x)) { to_solid = of_f(share, heat_r); }
            add_surface(x, s, to_solid);
            (*x).energy = add((*x).energy, sub(heat_r, to_solid));
            if (gt0(mixed)) { (*x).flame = add((*x).flame, mn(of_f(I(C_RADIATIVE, 0u), heat_r), sub(heat_r, to_solid))); (*x).lit = true; }
        }
    }

    // Soot.
    if (gt0((*x).gas[SOOT]) && gt0((*x).gas[O2])) {
        let k = k_index(gas_temp(*x));
        let want = div(mul(of_f(rate(RP.soot_b, k), (*x).gas[SOOT]), mn((*x).gas[O2], fresh)), fresh);
        let dm = mn(want, divi(muli((*x).gas[O2], C_O2_PER_C_DEN), C_O2_PER_C_NUM));
        if (gt0(dm)) {
            let ox = divi(muli(dm, C_O2_PER_C_NUM), C_O2_PER_C_DEN);
            (*x).gas[SOOT] = sub((*x).gas[SOOT], dm);
            (*x).gas[O2] = sub((*x).gas[O2], ox);
            (*x).gas[CO2] = add((*x).gas[CO2], add(dm, ox));
            (*x).energy = add((*x).energy, muli(dm, C_GAS_CHEM_SOOT));
        }
    }
}
// A 128-bit value times a small non-negative i64 (< 2³¹), exactly.
fn wmul_small(a: W, b: I) -> W {
    let ua = wabs(a);
    let lo = umulw(ua.xy, b);
    let hi = umulw(ua.zw, b);
    var r = wadd(lo, W(0u, 0u, hi.x, hi.y));
    return select(r, wneg(r), wisneg(a));
}

fn inert_air(x: C, s: S) -> bool {
    if (x.substance != 0u || x.lit || !iszero(solid_mass(x)) || !iszero(x.water) || !iszero(x.solid_energy)) { return false; }
    if (!iszero(x.gas[FUEL]) || !iszero(x.gas[SOOT]) || !iszero(x.gas[DROPS])) { return false; }
    let ml = gas_volume_ml(x, s);
    if (!gt0(ml)) { return false; }
    return le(x.gas[VAP], divi(sat_mul(saturated_at(gas_temp(x)), ml), 1000));
}

fn wind_over(c: u32) -> I {
    var uv = array<i32, 2>(0, 0);
    for (var a = 0u; a < 2u; a++) {
        let dn = step(c, a, false, true);
        uv[a] = (vminus(c, a, dn) + vplus(c, a)) / 2;
    }
    let sq = u32(uv[0] * uv[0] + uv[1] * uv[1]);
    return isqrt(I(sq, 0u));
}

@compute @workgroup_size(64)
fn react_main(@builtin(global_invocation_id) id: vec3<u32>) {
    let c = cell_of(id);
    if (c >= P.slots * 512u) { return; }
    var x = load(c);
    let s = subst(x.substance);
    if (!inert_air(x, s)) {
        var wind = I(0u, 0u);
        if (open_water(x)) { wind = wind_over(c); }
        let pilot = (limits[2u * c + RP.pad1] & 1u) != 0u;
        let core_before = core_capacity(x, s);
        react_inner(&x, s, ambient(), pilot, wind);
        let core_after = core_capacity(x, s);
        if (lt(core_after, core_before) && gt0(core_before)) {
            let keep = narrow(wdiv(mulw(x.solid_energy, core_after), widen(core_before)));
            let moved = sub(x.solid_energy, keep);
            x.solid_energy = keep;
            add_surface(&x, s, moved);
        }
    }
    store(c, x);
}

// ── each cell's side (transport::side_lit), on the GPU ──────────────────────
// What diffusion's faces need of a cell, worked out from the state on the
// GPU instead of uploaded: the same integers as side_lit. The pilot flags
// (flame_reach) are bits in `aux`, as for the reaction.
fn flame_release_gpu(x: C, s: S) -> I {
    if (!gt0(x.gas[FUEL]) || !gt0(x.gas[O2]) || !permeable(x, s) || !gt0(gas_volume_ml(x, s))) { return I(0u, 0u); }
    let fh = flame_holds(x, s, true);
    if (!fh.holds) { return I(0u, 0u); }
    return muli(of_f(I(C_MIXING, 0u), fh.could), C_GAS_CHEM_FUEL);
}

fn put_side(c: u32, v: array<I, 12>, flags: u32) {
    let o = c * SS;
    for (var j = 0u; j < 12u; j++) { sides_rw[o + 2u * j] = v[j].x; sides_rw[o + 2u * j + 1u] = v[j].y; }
    sides_rw[o + 24u] = flags;
}

@compute @workgroup_size(64)
fn sides_main(@builtin(global_invocation_id) id: vec3<u32>) {
    let c = cell_of(id);
    if (c >= P.slots * 512u) { return; }
    let x = load(c);
    let s = subst(x.substance);
    let amb = ambient();
    let pilot = (limits[2u * c + RP.pad1] & 1u) != 0u;
    let share = solid_share(x, s);
    let k = sub(add(of_f(share, s.conductivity), i_(C_AIR_COND)), of_f(share, i_(C_AIR_COND)));
    let t = gas_temp(x);
    let wet = open_water(x);
    let perm = permeable(x, s);
    var t_cond = solid_temperature(x, s);
    if (!wet && perm) { t_cond = t; }
    var water_g = I(0u, 0u);
    if (wet) {
        let half_um = mx(divi(x.water, 2 * C_FACE_MM2), i_(500));
        water_g = div(mul(i_(C_WATER_COND * C_FACE_MM2), i_(1000)), half_um);
    }
    let t_skin = surface_temperature(x, s);
    let gas_ml = gas_volume_ml(x, s);
    var fits = I(0xffffffffu, 0x7fffffffu);
    if (gt0(t)) { fits = narrow(wdiv(wmul_small(mulw(i_(C_FRESH_TOTAL), gas_ml), amb), mulw(i_(C_CELL_VOLUME_ML), t))); }
    let has_vol = gt0(x.solid[VOLATILE]);
    let flaming = has_vol && ((!lt(t_skin, i_(C_IGNITION)) && !lt(t, i_(C_IGNITION))) || (x.front > 0u && x.lit && fine_fuel(x, s)));
    let flame_t = select(I(0u, 0u), i_(C_FLAME_CONTACT), flaming);
    var release = I(0u, 0u);
    if (pilot && gt0(gas_ml)) { release = flame_release_gpu(x, s); }
    let flame_fill = mn(narrow(wdiv(mulw(release, i_(1000)), mulw(i_(C_FLAME_UJ_PER_ML), mx(gas_ml, i_(1))))), i_(1000));
    let excess_e = mx(sub(sub(x.energy, latent(x)), cap_t(gas_capacity(x), amb)), I(0u, 0u));
    let flame_heat = select(I(0u, 0u), divi(excess_e, 6), flaming || gt0(flame_fill));
    var fuel_g = I(0u, 0u);
    if (has_vol && gt0(s.sigma)) {
        var area = i_(C_FACE_MM2);
        var h = i_(C_FLAME_ON_FACE);
        if (perm) { area = inner_surface_mm2(x, s); h = mul(surface_h(s.sigma, t, amb), s.sigma); }
        fuel_g = mul_div(h, area, i_(1000));
    }
    var reached = i_(1000);
    if (fine_fuel(x, s) && lt(t_skin, i_(C_IGNITION))) { reached = divi(muli(i_(i32(x.front)), 1000), 65535); }
    var gm = I(0u, 0u);
    for (var j = 0u; j < 7u; j++) { gm = add(gm, x.gas[j]); }
    // gas mass − fits, or 0 where nothing fits (t = 0: i64::MAX).
    var over = I(0u, 0u);
    if (lt(fits, gm)) { over = sub(gm, fits); }
    let half = muli(k, 2 * C_CELL_MM);
    put_side(c, array<I, 12>(t, t_cond, t_skin, gas_ml, half, over, flame_t, flame_heat, flame_fill, fuel_g, reached, water_g),
        select(0u, 1u, perm) | select(0u, 2u, wet));
}

// ── flame_reach on the GPU ─────────────────────────────────────────────────
// chem::flame_reach is a flood fill: seeds hold burnable gas and can light
// it; the flame spreads to face neighbours (inside the box) holding burnable
// gas that air passes through. The set it reaches is the least fixed point
// of "lit = seed, or can and a neighbour is lit", whatever the order, so
// repeated propagation gives exactly the same set. Per cell, in `limits`
// (free before diffusion's limiters and during reaction): two words, ping-
// ponged between rounds, each bit 0 lit, bit 1 can. Each round, a 4³ tile
// propagates within itself to a standstill in workgroup memory, and flags
// (per workgroup, in `tallies`) whether anything in it changed.

fn burnable(x: C) -> bool { return gt0(x.gas[FUEL]) && gt0(x.gas[O2]); }
fn lights(x: C, s: S) -> bool {
    return x.lit
        || (x.front > 0u && gt0(x.solid[VOLATILE]) && fine_fuel(x, s))
        || (gt0(gas_capacity(x)) && !lt(gas_temp(x), i_(C_IGNITION)))
        || (gt0(add(x.solid[VOLATILE], x.solid[CHAR])) && !lt(surface_temperature(x, s), i_(C_IGNITION)));
}

@compute @workgroup_size(64)
fn reach_init(@builtin(global_invocation_id) id: vec3<u32>) {
    let c = cell_of(id);
    if (c >= P.slots * 512u) { return; }
    let x = load(c);
    let s = subst(x.substance);
    let b = burnable(x);
    let v = select(0u, 1u, b && lights(x, s)) | select(0u, 2u, b && permeable(x, s));
    limits[2u * c] = v;
    limits[2u * c + 1u] = v;
}

var<workgroup> wlit: array<u32, 64>;
var<workgroup> wchanged: atomic<u32>;
var<workgroup> wany: atomic<u32>;

@compute @workgroup_size(64)
fn reach_round(@builtin(workgroup_id) wg: vec3<u32>, @builtin(local_invocation_index) t: u32) {
    let w = wg.x + wg.y * P.tiles_x;
    let live = w < P.slots * 8u;
    let pin = RP.pad0;           // the word read this round; the other is written
    var c = 0u;
    var v = 0u;
    if (live) { c = my_cell(wg, t); v = limits[2u * c + pin]; }
    let start = v & 1u;
    wlit[t] = v & 1u;
    if (t == 0u) { atomicStore(&wany, 0u); }
    // Neighbours: inside the tile from workgroup memory, else from this
    // round's input (box edges have none).
    var nb = array<u32, 6>(NONE, NONE, NONE, NONE, NONE, NONE);
    var nt = array<i32, 6>(-1, -1, -1, -1, -1, -1);
    if (live) {
        for (var f = 0u; f < 6u; f++) {
            let axis = f / 2u; let up = (f & 1u) == 1u;
            let n = step(c, axis, up, false);
            if (n == OUT || n == NONE) { continue; }
            let tp = tile_pos(t, axis);
            if ((up && tp < 3u) || (!up && tp > 0u)) { nt[f] = select(i32(t) - i32(stride(axis)), i32(t + stride(axis)), up); }
            else { nb[f] = n; }
        }
    }
    loop {
        workgroupBarrier();
        if (t == 0u) { atomicStore(&wchanged, 0u); }
        workgroupBarrier();
        if (live && (v & 3u) == 2u) {
            var any = false;
            for (var f = 0u; f < 6u; f++) {
                if (nt[f] >= 0) { any = any || wlit[u32(nt[f])] != 0u; }
                else if (nb[f] != NONE) { any = any || (limits[2u * nb[f] + pin] & 1u) != 0u; }
            }
            if (any) { v = v | 1u; atomicStore(&wchanged, 1u); }
        }
        workgroupBarrier();
        wlit[t] = v & 1u;
        if (workgroupUniformLoad(&wchanged) == 0u) { break; }
    }
    if (live) {
        limits[2u * c + 1u - pin] = v;
        if ((v & 1u) != start) { atomicStore(&wany, 1u); }
    }
    workgroupBarrier();
    if (t == 0u && live) { tallies[w * 16u] = atomicLoad(&wany); }
}
