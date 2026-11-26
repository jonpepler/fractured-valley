// Pixel-art iso sprites for fv-core cells, drawn on the GPU.
//
// Every cell is a 4 px wide sprite (2:1 dimetric), anchored at
//   screen x = 2·(x − y),  screen y = x + y − 2·z   (plus the camera offset):
//
//    . T T .      T = top face
//    T T T T      L = the +y face (lower left)
//    L L R R      R = the +x face (lower right)
//    L L R R
//
// The quad drawn for each cell is taller than the sprite (rows −8 … 3) so grass
// blades and flames can poke above it; pixels a sprite doesn't use are thrown
// away. Everything is drawn into a small "native" image, one texel per sprite
// pixel, which the last pass blows up by an integer scale, so the result is
// exact pixel art.
//
// Colours here are linear light, already divided by the exposure:
//   surface = albedo × light / exposure + glow / exposure.

struct Globals {
    // Camera offset added to every sprite anchor, in native pixels.
    origin: vec2<i32>,
    // Size of the native image.
    native_size: vec2<u32>,
    // Size of the output image and the pixel scale between the two.
    out_size: vec2<u32>,
    scale: u32,
    tick: u32,
    // 1 / exposure (W/m² that shows as "full brightness").
    inv_exposure: f32,
    // 0 at night … 1 in daylight: how bright the backdrop is.
    day: f32,
    // Divides the depth key into 0..1.
    depth_div: f32,
    // 1 = the output is a plain unorm texture, so encode sRGB here.
    encode_srgb: u32,
    outline: vec4<f32>,
    // Tone curve on light: shows (light / exposure) ^ curve. Below 1 it lifts
    // shadows and tames highlights, like a photographer's grade.
    light_curve: f32,
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
};

@group(0) @binding(0) var<uniform> g: Globals;

// One sprite. `cell.w` holds packed flags (see lib.rs, `flags`).
struct Inst {
    @location(0) cell: vec4<i32>,
    @location(1) albedo: vec4<f32>,
    @location(2) light: vec4<f32>,
    @location(3) glow: vec4<f32>,
};

struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) @interpolate(flat) cell: vec4<i32>,
    @location(1) @interpolate(flat) anchor: vec2<i32>,
    @location(2) @interpolate(flat) albedo: vec4<f32>,
    @location(3) @interpolate(flat) light: vec4<f32>,
    @location(4) @interpolate(flat) glow: vec4<f32>,
};

// Flag bits (keep in step with lib.rs).
const OPEN_X: i32 = 1;      // +x neighbour lets light through
const OPEN_Y: i32 = 2;
const OPEN_Z: i32 = 4;
const TOP_AO: i32 = 8;      // something stands just behind-above the top face
const LEDGE: i32 = 16;      // the top face is on a front edge (catches light)
const SECTION_X: i32 = 32;  // the +x face is the box edge: a cut through the ground
const SECTION_Y: i32 = 64;

// Which face each pixel of the 4×4 sprite belongs to: -1 none, 0 top, 1 left, 2 right.
const FACE = array<i32, 16>(
    -1, 0, 0, -1,
     0, 0, 0,  0,
     1, 1, 2,  2,
     1, 1, 2,  2,
);
// Per-face shading, kept small: the physical light already makes tops brighter.
const FACE_SHADE = array<f32, 3>(1.0, 0.82, 0.66);
// Texture: brightness steps picked per pixel, dark → light.
const RAMP = array<f32, 8>(0.80, 0.87, 0.93, 0.97, 1.0, 1.03, 1.08, 1.14);
// Ground cut by the edge of the box, dark purple (linear).
const SECTION = vec3<f32>(0.040, 0.026, 0.044);

// Surface patterns (bits 8..15 of the flags), chosen per substance in lib.rs.
const PLAIN: i32 = 0;
const STONE: i32 = 1;
const WOOD: i32 = 2;
const SOIL: i32 = 3;
const GRASS: i32 = 4;
const LEAVES: i32 = 5;
const METAL: i32 = 6;

// Integer hash (PCG-style): the same cell, pixel and tick always give the same value.
fn hash(a: u32) -> u32 {
    var s = a * 747796405u + 2891336453u;
    s = ((s >> ((s >> 28u) + 4u)) ^ s) * 277803737u;
    return (s >> 22u) ^ s;
}
fn hash4(a: i32, b: i32, c: i32, d: i32) -> u32 {
    return hash(u32(a) ^ hash(u32(b) ^ hash(u32(c) ^ hash(u32(d)))));
}
fn unit(h: u32) -> f32 { return f32(h & 0xffffu) / 65535.0; }

// Smooth noise on a grid, 0..1, for smoke and flames (screen space, so it is
// continuous from one cell's sprite to the next).
fn value_noise(p: vec2<f32>, seed: i32) -> f32 {
    let i = vec2<i32>(floor(p));
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = unit(hash4(i.x, i.y, seed, 0));
    let b = unit(hash4(i.x + 1, i.y, seed, 0));
    let c = unit(hash4(i.x, i.y + 1, seed, 0));
    let d = unit(hash4(i.x + 1, i.y + 1, seed, 0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// 4×4 ordered dither, 0..1.
const BAYER = array<f32, 16>(0., 8., 2., 10., 12., 4., 14., 6., 3., 11., 1., 9., 15., 7., 13., 5.);
fn bayer(p: vec2<i32>) -> f32 { return (BAYER[(p.y & 3) * 4 + (p.x & 3)] + 0.5) / 16.0; }

@vertex
fn vs_sprite(@builtin(vertex_index) vi: u32, inst: Inst) -> VOut {
    var corners = array<vec2<f32>, 6>(vec2(0., 0.), vec2(1., 0.), vec2(0., 1.), vec2(1., 0.), vec2(1., 1.), vec2(0., 1.));
    let c = corners[vi];
    let x = inst.cell.x;
    let y = inst.cell.y;
    let z = inst.cell.z;
    let anchor = vec2<i32>(2 * (x - y), x + y - 2 * z) + g.origin;
    let px = vec2<f32>(anchor) + vec2<f32>(c.x * 4.0, -8.0 + c.y * 12.0);
    let size = vec2<f32>(g.native_size);
    var out: VOut;
    // Nearer cells (larger x + y + z) get a larger depth; ties go to the higher cell.
    let depth = (f32(x + y + z) + f32(z) / 4096.0 + 1.0) / g.depth_div;
    out.pos = vec4<f32>(px.x / size.x * 2.0 - 1.0, 1.0 - px.y / size.y * 2.0, depth, 1.0);
    out.cell = inst.cell;
    out.anchor = anchor;
    out.albedo = inst.albedo;
    out.light = inst.light;
    out.glow = inst.glow;
    return out;
}

// Brightness steps per stop (doubling) of light.
const LIGHT_STEPS: f32 = 3.0;

// Light as it shows: divided by the exposure, through the tone curve.
fn shown(light: vec3<f32>) -> vec3<f32> {
    // Day and night alike: an eye adapted to firelight still sees the dim
    // ground a few metres off, compressed the way vision compresses
    // brightness (a power law, Stevens), not black.
    let s = pow(max(light * g.inv_exposure, vec3<f32>(0.0)), vec3<f32>(g.light_curve));
    // Pixel art shades in steps, not smooth ramps: snap the brightness to
    // steps of 1/LIGHT_STEPS of a stop (colour kept).
    let l = dot(s, vec3<f32>(0.2126, 0.7152, 0.0722));
    if l <= 0.0 { return s; }
    let snapped = exp2(round(log2(l) * LIGHT_STEPS) / LIGHT_STEPS);
    return s * (snapped / l);
}

fn flag(v: VOut, bit: i32) -> bool { return (v.cell.w & bit) != 0; }
fn pattern(v: VOut) -> i32 { return (v.cell.w >> 8) & 0xff; }

// How a surface pattern changes the brightness of one pixel of a face.
// p = pixel in the sprite, face = 0/1/2.
fn pattern_shade(v: VOut, p: vec2<i32>, face: i32, h: u32) -> f32 {
    let pat = pattern(v);
    let x = v.cell.x;
    let y = v.cell.y;
    let z = v.cell.z;
    var k = 1.0;
    if pat == STONE {
        // Dressed stone: courses 2 cells (20 cm) high, blocks 3 cells long,
        // staggered course to course. Joints are dark pixels on the side faces.
        let course = z / 2;
        let bottom_row = p.y == 3 && (z % 2) == 0;
        let along = select(y, x, face == 1) + (course % 2) * 2;
        let joint_col = select(2, 0, face == 1);
        let vertical = (along % 3) == 0 && p.x == joint_col;
        if face > 0 && (bottom_row || vertical) { k = 0.62; }
        // Faint joints on top too, so outcrops read as blocks.
        if face == 0 && ((x + (y / 3)) % 3) == 0 && p.y == 1 && p.x == 0 { k = 0.8; }
    }
    if pat == WOOD && face > 0 {
        // Grain: alternate columns along the log, lighter and darker.
        k = select(0.9, 1.05, (p.x & 1) == 0);
    }
    if pat == METAL {
        // Smooth, with a highlight along the top edge.
        k = select(1.0, 1.15, face == 0 && p.y == 0);
        return k;
    }
    // Everything else: a per-pixel brightness step from the ramp.
    return k * RAMP[h & 7u];
}

@fragment
fn fs_block(v: VOut) -> @location(0) vec4<f32> {
    let p = vec2<i32>(floor(v.pos.xy)) - v.anchor;
    if p.y < 0 || p.y > 3 || p.x < 0 || p.x > 3 { discard; }
    let face = FACE[p.y * 4 + p.x];
    if face < 0 { discard; }
    let h = hash4(v.cell.x * 16 + p.x + p.y * 4, v.cell.y, v.cell.z, 11);
    // Almost burnt away: only a few grey flecks of ash remain.
    let left = v.albedo.w;
    if left < 0.25 && unit(h >> 8u) > left * 4.0 { discard; }

    // The cut through the ground at the edge of the box.
    if (face == 2 && flag(v, SECTION_X)) || (face == 1 && flag(v, SECTION_Y)) {
        let s = SECTION * (0.9 + 0.2 * unit(h)) * max(g.day, 0.15);
        return vec4<f32>(s, 1.0);
    }
    var shade = FACE_SHADE[face] * pattern_shade(v, p, face, h);
    if face == 0 && flag(v, TOP_AO) { shade *= 0.88; }
    if face == 0 && !flag(v, TOP_AO) && flag(v, LEDGE) && p.y == 0 { shade *= 1.08; }
    var col = v.albedo.rgb * shown(v.light.rgb) * shade;
    // Glowing embers: brighter in the cracks (a fixed pattern, averaging 1).
    // How bright they are, and how it flickers, is the simulation's: the
    // glow is what this cell emitted this tick.
    let crack = unit(hash4(v.cell.x * 16 + p.x + p.y * 4, v.cell.y, v.cell.z, 5));
    col += v.glow.rgb * g.inv_exposure * (0.25 + 2.25 * crack * crack);
    return vec4<f32>(col, 1.0);
}

// Grass and leaves: a sparse tuft of blades, so what's under it shows through.
@fragment
fn fs_tuft(v: VOut) -> @location(0) vec4<f32> {
    let p = vec2<i32>(floor(v.pos.xy)) - v.anchor;
    if p.x < 0 || p.x > 3 || p.y < -1 || p.y > 3 { discard; }
    let h = hash4(v.cell.x * 32 + p.x + (p.y + 1) * 4, v.cell.y, v.cell.z, 23);
    let r = unit(h >> 8u);
    let leaves = pattern(v) == LEAVES;
    // How much of each row the tuft fills: blade tips above, dense on top, sparse at the sides.
    var cover = array<f32, 5>(0.22, 0.62, 0.72, 0.30, 0.18);
    if leaves { cover = array<f32, 5>(0.15, 0.85, 0.95, 0.9, 0.8); }
    // Blade tips only stand in the middle two columns.
    if p.y == -1 && (p.x == 0 || p.x == 3) { discard; }
    if p.y == 0 && (p.x == 0 || p.x == 3) && r > 0.5 * cover[1] { discard; }
    var keep = cover[p.y + 1];
    // Burnt down to almost nothing: thinner still.
    keep *= min(1.0, v.albedo.w * 2.0);
    if r > keep { discard; }
    // Tips catch the light; lower blades sit in the tuft's shade.
    var row_shade = array<f32, 5>(1.18, 1.06, 0.96, 0.80, 0.72);
    let shade = row_shade[p.y + 1] * RAMP[h & 7u];
    var col = v.albedo.rgb * shown(v.light.rgb) * shade;
    // Glowing blades: a fixed pattern averaging 1, brightness from the sim.
    let ember = unit(hash4(v.cell.x * 32 + p.x, v.cell.y + p.y, v.cell.z, 9));
    col += v.glow.rgb * g.inv_exposure * (3.0 * ember * ember);
    return vec4<f32>(col, 1.0);
}

// Smoke: soot (dark) and mist (white) in one cell, as a translucent cube lit by
// the light in that cell. `albedo.w` is the optical depth through the cell.
@fragment
fn fs_smoke(v: VOut) -> @location(0) vec4<f32> {
    let p = vec2<i32>(floor(v.pos.xy)) - v.anchor;
    if p.y < 0 || p.y > 3 || p.x < 0 || p.x > 3 { discard; }
    if FACE[p.y * 4 + p.x] < 0 { discard; }
    // Clumpy texture drifting upward, in screen space so it flows across cells.
    let s = vec2<f32>(floor(v.pos.xy) - vec2<f32>(g.origin)) + vec2<f32>(0.0, f32(g.tick) * 0.35);
    let n = value_noise(s / 5.0, 3) * 0.65 + value_noise(s / 2.0, 4) * 0.35;
    let tau = v.albedo.w * (0.2 + 1.6 * n);
    var a = 1.0 - exp(-tau);
    // Pixel art: opacity in a few steps, dithered.
    a = clamp(floor(a * 6.0 + bayer(vec2<i32>(floor(v.pos.xy)))) / 6.0, 0.0, 0.75);
    if a <= 0.0 { discard; }
    let col = v.albedo.rgb * shown(v.light.rgb) * (0.9 + 0.2 * n);
    return vec4<f32>(col, a);
}

// Flames: hot gas giving off light. Drawn additively as tongues that rise from
// the cell. The tongues' shape moves (a view choice) but their brightness is
// the light the simulation's flame sent out from this cell this tick, so the
// flame and what it lights flicker together.
@fragment
fn fs_flame(v: VOut) -> @location(0) vec4<f32> {
    let p = vec2<i32>(floor(v.pos.xy)) - v.anchor;
    if p.x < 0 || p.x > 3 || p.y > 3 { discard; }
    let sp = vec2<f32>(floor(v.pos.xy) - vec2<f32>(g.origin));
    // Tongues: noise stretched upright and scrolling up fast.
    let n = value_noise(vec2<f32>(sp.x / 1.5, (sp.y + f32(g.tick) * 2.0) / 4.0), 7);
    // Height above the sprite's bottom, 0 … 11; flames thin out with height.
    let up = f32(3 - p.y);
    // How tall this cell's flames reach grows with how bright it is.
    let reach = clamp(2.0 + 2.5 * log2(1.0 + v.glow.w), 2.0, 11.0);
    let f = n - up / reach;
    if f < 0.05 { discard; }
    let w = clamp(f * 2.2, 0.0, 1.6);
    return vec4<f32>(v.glow.rgb * g.inv_exposure * w, 0.0);
}

// ---------------------------------------------------------------------------
// Final pass: scale the native image up, tone-map, outline, encode.

@group(0) @binding(1) var native: texture_2d<f32>;

@vertex
fn vs_full(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    var pts = array<vec2<f32>, 3>(vec2(-1., -1.), vec2(3., -1.), vec2(-1., 3.));
    return vec4<f32>(pts[vi], 0.0, 1.0);
}

// Filmic tone curve (the ACES fit of Narkowicz 2015), per channel: a toe that
// keeps the darks dark, near-linear midtones, and a long shoulder, so a lit
// surface at the exposure shows bright but not white, and the fire, many
// times brighter, rolls off to yellow-white without the rest losing shape.
fn tonemap(c: vec3<f32>) -> vec3<f32> {
    let x = c * 0.9;
    return clamp((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14), vec3<f32>(0.0), vec3<f32>(1.0));
}

// Bloom: light far above the exposure spills into the pixels around it, as
// it does in an eye or a lens (view only). Only what is over BLOOM_FROM
// spreads, with a Gaussian of BLOOM_RADIUS native pixels.
const BLOOM_FROM: f32 = 1.5;
const BLOOM_RADIUS: i32 = 5;
const BLOOM_STRENGTH: f32 = 0.12;
fn bloom(p: vec2<i32>) -> vec3<f32> {
    let s = vec2<i32>(g.native_size);
    var sum = vec3<f32>(0.0);
    var weight = 0.0;
    for (var dy = -BLOOM_RADIUS; dy <= BLOOM_RADIUS; dy += 1) {
        for (var dx = -BLOOM_RADIUS; dx <= BLOOM_RADIUS; dx += 1) {
            let w = exp(-f32(dx * dx + dy * dy) / (0.5 * f32(BLOOM_RADIUS * BLOOM_RADIUS)));
            weight += w;
            let q = p + vec2<i32>(dx, dy);
            if q.x < 0 || q.y < 0 || q.x >= s.x || q.y >= s.y { continue; }
            sum += w * max(textureLoad(native, q, 0).rgb - vec3<f32>(BLOOM_FROM), vec3<f32>(0.0));
        }
    }
    return sum / weight * BLOOM_STRENGTH;
}

fn to_srgb(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3<f32>(0.0031308));
}

fn coverage(p: vec2<i32>) -> f32 {
    let s = vec2<i32>(g.native_size);
    if p.x < 0 || p.y < 0 || p.x >= s.x || p.y >= s.y { return 0.0; }
    return textureLoad(native, p, 0).a;
}

@fragment
fn fs_composite(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let p = vec2<i32>(floor(pos.xy)) / i32(g.scale);
    let c = textureLoad(native, p, 0);
    var rgb = tonemap(max(c.rgb, vec3<f32>(0.0)) + bloom(p));
    // A dark outline where the world meets the backdrop (alpha 1 = solid).
    let edge = c.a < 0.5 && max(max(coverage(p + vec2(1, 0)), coverage(p - vec2(1, 0))),
                                max(coverage(p + vec2(0, 1)), coverage(p - vec2(0, 1)))) > 0.5;
    if edge { rgb = g.outline.rgb; }
    if g.encode_srgb == 1u { rgb = to_srgb(rgb); }
    return vec4<f32>(rgb, 1.0);
}
