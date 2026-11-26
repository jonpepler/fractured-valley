//! What the renderer sees: a plain snapshot of the world, built from the
//! simulation each frame. Everything here is *view*: smoothing, scaling and
//! colours for drawing. Nothing flows back into the simulation.
//!
//! Light is physical: `light` is the visible energy that arrived at each
//! cell (smoothed over a few ticks to hide packet noise), and `glow` is the
//! visible energy the cell itself gave off. A renderer draws a surface as its
//! colour times the light that reached it, plus its own glow, scaled by an
//! exposure (the eye or camera, a view choice). So a scene at night is lit
//! only by what the simulation emitted.

use crate::cell::{Cell, CHAR, MINERAL, VOLATILE};
use crate::rad::Light;
use crate::substance::{Gas, Substance, NONE};
use crate::world::World;

/// What kind of thing a cell looks like.
pub const AIR: u8 = 0;
/// A surface light stops at (stone, wood, soil): drawn as a block.
pub const SOLID: u8 = 1;
/// Light passes partly through (grass, leaves): drawn as a sparse tuft.
pub const POROUS: u8 = 2;

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct ViewCell {
    pub kind: u8,
    pub substance: u8,
    /// How charred, 0 (fresh) … 255 (all its burnable gas gone).
    pub char: u8,
    /// How much of the solid is left, 0 … 255 (burnt to ash: small).
    pub left: u8,
    /// Visible light arriving, per band [red, green, blue], in watts per
    /// square metre of face (smoothed).
    pub light: [f32; 3],
    /// Visible light given off, per band, in watts per cell (smoothed).
    pub glow: [f32; 3],
    /// Soot in the air here, mg per litre (dark smoke).
    pub soot: f32,
    /// Water drops in the air here, mg per litre (white smoke, mist).
    pub mist: f32,
    /// The temperature that faces the world, °C.
    pub temp_c: f32,
}

/// A snapshot for drawing, cells in x-fastest order (index = (z·ny + y)·nx + x).
pub struct View {
    pub nx: usize,
    pub ny: usize,
    pub nz: usize,
    pub cells: Vec<ViewCell>,
    /// Display colour per substance id (sRGB 0–255).
    pub colours: Vec<[u8; 3]>,
    pub tick: u64,
    /// Cells riding falling pieces: where each cell's centre is (in cells,
    /// so (x + 0.5, y + 0.5, z + 0.5) is the middle of grid cell x, y, z).
    pub bodies: Vec<([f32; 3], ViewCell)>,
}

/// Keeps the smoothed light between frames (view only).
pub struct Viewer {
    light: Vec<[f32; 3]>,
    glow: Vec<[f32; 3]>,
    /// Share of the new tick blended into the running average (1 = no smoothing).
    pub blend: f32,
}

impl Viewer {
    pub fn new() -> Viewer { Viewer { light: Vec::new(), glow: Vec::new(), blend: 0.25 } }

    /// Build a snapshot of the world and the light it made this tick.
    pub fn view(&mut self, w: &World, light: &Light) -> View {
        let n = w.nx * w.ny * w.nz;
        if self.light.len() != n { self.light = vec![[0.0; 3]; n]; self.glow = vec![[0.0; 3]; n] }
        // µJ per tick on one face → W/m²: ×10⁻⁶ / 0.05 s / face (0.002 at 10 cm)
        const TO_W_M2: f32 = 2e-5 * 1e6 / crate::substance::FACE_MM2 as f32;
        // µJ per tick → W: ×10⁻⁶ / 0.05 s = ×2×10⁻⁵
        const TO_W: f32 = 2e-5;
        let mut cells = vec![ViewCell::default(); n];
        let b = self.blend;
        for z in 0..w.nz { for y in 0..w.ny { for x in 0..w.nx {
            let i = w.idx(x, y, z);
            let o = (z * w.ny + y) * w.nx + x;
            let c = &w.cells[i];
            let l = light.incident[i];
            let g = light.glow[i];
            for k in 0..3 {
                self.light[o][k] += (l[k] as f32 * TO_W_M2 - self.light[o][k]) * b;
                self.glow[o][k] += (g[k] as f32 * TO_W - self.glow[o][k]) * b;
            }
            cells[o] = view_cell(c, &w.subs, self.light[o], self.glow[o]);
        }}}
        View { nx: w.nx, ny: w.ny, nz: w.nz, cells, colours: w.subs.iter().map(|s| s.colour).collect(), tick: w.tick, bodies: Vec::new() }
    }

    /// The same, with the pieces that are falling.
    pub fn view_sim(&mut self, sim: &crate::sim::Sim) -> View {
        let mut v = self.view(&sim.world, &sim.light);
        for (pos, _rot, c) in sim.bodies.cells() {
            let vc = view_cell(c, &sim.world.subs, [0.0; 3], [0.0; 3]);
            v.bodies.push(([pos[0] * 10.0, pos[1] * 10.0, pos[2] * 10.0], vc));
        }
        v
    }
}

impl Default for Viewer { fn default() -> Self { Self::new() } }

fn view_cell(c: &Cell, subs: &[Substance], light: [f32; 3], glow: [f32; 3]) -> ViewCell {
    let s = &subs[c.substance as usize];
    let kind = if c.substance == NONE || c.solid_mass().0 == 0 { AIR } else if c.opaque(subs) { SOLID } else { POROUS };
    let full_volatile = s.full_mass.0 * s.composition[VOLATILE] / 1000;
    let char = if full_volatile > 0 { (255 - c.solid[VOLATILE].0 * 255 / full_volatile).clamp(0, 255) as u8 } else { 0 };
    let left = if s.full_mass.0 > 0 { ((c.solid[VOLATILE].0 + c.solid[CHAR].0 + c.solid[MINERAL].0) * 255 / s.full_mass.0).clamp(0, 255) as u8 } else { 0 };
    let ml = c.gas_volume_ml(subs).max(1) as f32;
    ViewCell {
        kind, substance: c.substance, char, left, light, glow,
        soot: c.gas[Gas::Soot as usize].0 as f32 / 1000.0 / ml * 1000.0,
        mist: c.gas[Gas::Droplets as usize].0 as f32 / 1000.0 / ml * 1000.0,
        temp_c: c.temperature(subs).celsius_f64() as f32,
    }
}
