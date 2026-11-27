//! The diffusion phase on a brick pool (brief 10, D3), the layout a sparse,
//! refined world needs: bricks in any slot order, a 26-neighbour slot
//! table per brick, field groups struct-of-arrays inside each brick, the
//! state double-buffered, and face fluxes only in workgroup memory.
//!
//! [`PoolDiffusion::from_world`] lays fv-core's dense box out as such a
//! pool, with the bricks in a shuffled slot order, so tests can check the
//! pool against fv-core bit for bit. A missing neighbour brick is the box's
//! edge.

use crate::{constants, ms, pack_side, Timings, SIDE_WORDS};
use fv_core::substance::GASES;
use fv_core::transport::{side, sides, FaceVelocity};
use fv_core::world::{World, B, BRICK_CELLS};
use std::time::Instant;

pub fn pool_shader_source() -> String { constants() + &react_constants() + include_str!("common.wgsl") + include_str!("pool.wgsl") + include_str!("react.wgsl") + include_str!("flow.wgsl") }

/// The reaction's constants (fv-core's chem, cell and substance), as WGSL.
fn react_constants() -> String {
    use fv_core::chem as ch;
    use fv_core::substance as su;
    let c = |v: i64| i32::try_from(v).expect("constant fits in i32");
    let u = |v: u64| u32::try_from(v).expect("constant fits in u32");
    format!("const C_CHAR_SH: i32 = {};\nconst C_WATER_SH: i32 = {};\nconst C_CELL_VOLUME_ML: i32 = {};\nconst C_FACE_MM2: i32 = {};\nconst C_SKIN_PERMILLE: i32 = {};\n\
const C_IGNITION: i32 = {};\nconst C_FRESH_O2: i32 = {};\nconst C_RESIDENCE: i32 = {};\nconst C_CHAR_UM_PER_MIN: i32 = {};\nconst C_O2_PER_FUEL: i32 = {};\n\
const C_GAS_CHEM_FUEL: i32 = {};\nconst C_GAS_CHEM_SOOT: i32 = {};\nconst C_CRITICAL_FLAME: i32 = {};\nconst C_BOIL: i32 = {};\nconst C_FINE_SIGMA: i32 = {};\n\
const C_O2_PER_C_NUM: i32 = {};\nconst C_O2_PER_C_DEN: i32 = {};\nconst C_CHAR_CHEM: i32 = {};\nconst C_CHAR_TO_CO: i32 = {};\nconst C_MIXING: u32 = {}u;\n\
const C_FRESH_TOTAL: i32 = {};\nconst C_AIR_COND: i32 = {};\nconst C_WATER_COND: i32 = {};\nconst C_FLAME_UJ_PER_ML: i32 = {};\nconst C_FLAME_ON_FACE: i32 = {};\nconst C_CO2_SHARE: i32 = {};\nconst C_FEEDBACK_MAX: u32 = {}u;\nconst C_FEEDBACK_HALF: i32 = {};\nconst C_RADIATIVE: u32 = {}u;\nconst C_SURFACE_H_PER_SIGMA: i32 = {};\n",
        c(su::CHAR_SPECIFIC_HEAT.0), c(su::WATER_SPECIFIC_HEAT.0), c(su::CELL_VOLUME_ML), c(su::FACE_MM2), c(fv_core::cell::SKIN_PERMILLE),
        c(ch::IGNITION.0), c(su::FRESH_AIR[su::Gas::Oxygen as usize].0), c(ch::RESIDENCE_S_PER_SIGMA), c(ch::CHAR_UM_PER_MIN), c(ch::O2_PER_FUEL),
        c(su::GAS_CHEMICAL[su::Gas::Fuel as usize].0), c(su::GAS_CHEMICAL[su::Gas::Soot as usize].0), c(ch::CRITICAL_FLAME.0), c(ch::BOIL.0), c(ch::FINE_SIGMA),
        c(ch::O2_PER_CARBON_NUM), c(ch::O2_PER_CARBON_DEN), c(su::CHAR_CHEMICAL.0), c(ch::CHAR_TO_CO_HEAT.0), u(ch::MIXING.0),
        c(su::FRESH_AIR.iter().map(|m| m.0).sum()), c(fv_core::transport::AIR_CONDUCTIVITY), c(fv_core::transport::WATER_CONDUCTIVITY),
        c(fv_core::transport::FLAME_UJ_PER_ML), c(fv_core::transport::FLAME_ON_FACE), c(ch::CO2_SHARE), u(ch::FEEDBACK_MAX.0), c(ch::FEEDBACK_HALF_MM2 as i64), u(ch::RADIATIVE_FRACTION.0), c(ch::SURFACE_H_PER_SIGMA))
}

/// Words per substance in the table buffer (see react.wgsl's `subst`).
const SUB_WORDS: usize = 16;
/// Solid-group words per cell (single-buffered; see react.wgsl).
pub const SOLID_WORDS: usize = 14;

/// Words of state per cell in one copy: 7 gases and 3 energies, i64 each
/// (the compact cell keeps each gas mass in one u32 word).
pub const STATE_WORDS: usize = 2 * (GASES + 3);
pub const COMPACT_STATE_WORDS: usize = GASES + 2 * 3;
/// Faces kept per brick slot with a global face scratch.
pub const FACES_PER_SLOT: usize = 3 * BRICK_CELLS + 3 * 64;

/// How the pool runs the phase.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Faces only in workgroup memory, worked out again in each of two passes.
    Workgroup,
    /// Faces worked out once into a global scratch, then the limiters, then apply.
    Scratch,
}
const NONE_SLOT: u32 = u32::MAX;

/// A permutation of `n` brick slots (deterministic), so that slot order is
/// shown not to matter.
pub fn shuffled(n: usize, seed: u64) -> Vec<usize> {
    let mut v: Vec<usize> = (0..n).collect();
    let mut s = fv_core::rng::Stream(seed);
    for i in (1..n).rev() { let j = s.below(i as u64 + 1) as usize; v.swap(i, j) }
    v
}

/// The step (dx, dy, dz) for each of the 26 neighbour slots, in the shader's order.
pub fn neighbour_steps() -> Vec<[i64; 3]> {
    let mut v = Vec::new();
    for dz in -1..=1 { for dy in -1..=1 { for dx in -1..=1 { if (dx, dy, dz) != (0, 0, 0) { v.push([dx, dy, dz]) } } } }
    v
}

/// The pool as the GPU holds it, built from the dense world.
pub struct PoolDiffusion {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub adapter_info: wgpu::AdapterInfo,
    pipelines: [wgpu::ComputePipeline; 20],
    bind_group: wgpu::BindGroup,
    params: wgpu::Buffer,
    state: wgpu::Buffer,
    sides: wgpu::Buffer,
    vel: wgpu::Buffer,
    table: wgpu::Buffer,
    tallies: wgpu::Buffer,
    limits: wgpu::Buffer,
    read: wgpu::Buffer,
    /// Brick b of the world is in slot `slot_of[b]`.
    pub slot_of: Vec<usize>,
    /// Held by the bind group; kept here so its size is visible.
    #[allow(dead_code)]
    faces: wgpu::Buffer,
    slots: usize,
    copy_words: usize,
    pub mode: Mode,
    /// Gas masses as u32 (the compact cell).
    pub compact: bool,
    pub last: Timings,
    /// The kernels' times, ms (two for Workgroup, three for Scratch).
    pub kernel_ms: Vec<f64>,
    /// The copy holding the world as it stands after the last diffusion
    /// phase, and the tick it was for: carrying the same tick starts from
    /// it without uploading the state again.
    resident: Option<(u64, u32)>,
    /// Carrying works out each face once (else each cell works out its own six).
    pub carry_once: bool,
    aux: wgpu::Buffer,
    rparams: wgpu::Buffer,
    fparams: wgpu::Buffer,
    /// Work out each cell's side on the GPU (else the CPU's are uploaded).
    pub gpu_sides: bool,
    rp: [u32; 12],
    /// Rounds the last flame-reach fill took.
    pub reach_rounds: u32,
    /// Read back only the tallies after diffusion and carrying, and the
    /// whole state only after the reaction. The CPU's world is then stale
    /// between those phases, so set it only when all four (diffusion, the
    /// flow, carrying, reaction) run here, every tick.
    pub lazy: bool,
}

impl PoolDiffusion {
    pub fn from_world(w: &World, slot_of: Vec<usize>) -> Result<PoolDiffusion, String> { Self::with_mode(w, slot_of, Mode::Workgroup, false) }

    pub fn with_mode(w: &World, slot_of: Vec<usize>, mode: Mode, compact: bool) -> Result<PoolDiffusion, String> {
        let slots = w.bricks();
        assert_eq!(slot_of.len(), slots);
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance, force_fallback_adapter: false, compatible_surface: None, ..Default::default()
        })).map_err(|e| format!("no GPU adapter: {e}"))?;
        let info = adapter.get_info();
        let mut limits = wgpu::Limits::default();
        let al = adapter.limits();
        limits.max_storage_buffer_binding_size = al.max_storage_buffer_binding_size;
        limits.max_buffer_size = al.max_buffer_size;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor { label: Some("fv-gpu pool"), required_limits: limits, ..Default::default() }))
            .map_err(|e| format!("no GPU device: {e}"))?;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("pool"), source: wgpu::ShaderSource::Wgsl(pool_shader_source().into()) });

        let cells = slots * BRICK_CELLS;
        let copy_words = cells * if compact { COMPACT_STATE_WORDS } else { STATE_WORDS };
        use wgpu::BufferUsages as U;
        let buf = |label: &str, words: usize, usage: wgpu::BufferUsages| device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: (words * 4) as u64, usage, mapped_at_creation: false });
        let params = buf("params", 16, U::UNIFORM | U::COPY_DST);
        // Two copies of the gas and heat groups, then the solid group.
        let state = buf("state", 2 * copy_words + cells * SOLID_WORDS, U::STORAGE | U::COPY_DST | U::COPY_SRC);
        let sides_b = buf("sides", cells * SIDE_WORDS, U::STORAGE | U::COPY_DST);
        let vel = buf("vel", slots * 3 * (BRICK_CELLS + 64), U::STORAGE | U::COPY_DST);
        let table = buf("table", Self::table_words(w, slots, &slot_of).0.len(), U::STORAGE | U::COPY_DST);
        // The pilot bits, then the brick → slot map (for the flow's dense block grid).
        let aux = buf("aux", cells / 32 + 1 + slots, U::STORAGE | U::COPY_DST);
        let fparams = buf("flow params", 16, U::UNIFORM | U::COPY_DST);
        let rparams = buf("react params", 12, U::UNIFORM | U::COPY_DST);
        let lim = buf("limits", cells * 2, U::STORAGE | U::COPY_SRC);
        // The face scratch also holds the flow's block arrays (25 words per block).
        let flow_words = w.cells.len() / 8 * 25;
        // The tallies, then one word flagging a gas mass out of the compact range.
        let tallies = buf("tallies", slots * 8 * 16 + 1, U::STORAGE | U::COPY_SRC | U::COPY_DST);
        let faces = buf("faces", (if mode == Mode::Scratch { slots * FACES_PER_SLOT * crate::FLUX_WORDS } else { crate::FLUX_WORDS }).max(flow_words), U::STORAGE | U::COPY_DST | U::COPY_SRC);
        let read = buf("read", copy_words + cells * SOLID_WORDS + slots * 8 * 16 + 1, U::MAP_READ | U::COPY_DST);
        let all = [&params, &state, &sides_b, &vel, &table, &lim, &tallies, &faces, &aux, &rparams, &fparams];
        // One layout for every kernel: binding 0 uniform, the rest storage
        // (read-only: sides, velocities, table).
        let entries: Vec<wgpu::BindGroupLayoutEntry> = (0..11u32).map(|b| wgpu::BindGroupLayoutEntry {
            binding: b, visibility: wgpu::ShaderStages::COMPUTE, count: None,
            ty: wgpu::BindingType::Buffer {
                ty: if b == 0 || b >= 9 { wgpu::BufferBindingType::Uniform } else { wgpu::BufferBindingType::Storage { read_only: matches!(b, 3 | 4 | 8) } },
                has_dynamic_offset: false, min_binding_size: None,
            },
        }).collect();
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("pool"), entries: &entries });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("pool"), bind_group_layouts: &[Some(&bgl)], immediate_size: 0 });
        let names = ["limits_pass", "apply_pass", "faces_scratch", "limits_scratch", "apply_scratch", "carry_prepare", "carry_apply", "carry_prepare2", "carry_faces", "carry_apply2", "react_main",
            "flow_gather", "flow_carried", "flow_forced", "flow_div", "flow_pressure", "flow_project", "sides_main", "reach_init", "reach_round"];
        let pipelines = names.map(|e| device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor { label: Some(e), layout: Some(&layout), module: &module, entry_point: Some(e), compilation_options: Default::default(), cache: None }));
        let bge: Vec<wgpu::BindGroupEntry> = (0..11).map(|b| wgpu::BindGroupEntry { binding: b as u32, resource: all[b].as_entire_binding() }).collect();
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("pool"), layout: &bgl, entries: &bge });
        let mut p = PoolDiffusion { device, queue, adapter_info: info, pipelines, bind_group, params, state, sides: sides_b, vel, table, tallies, limits: lim.clone(), faces, read, slot_of, slots, copy_words, mode, compact, last: Timings::default(), kernel_ms: Vec::new(), resident: None, carry_once: true, aux, rparams, fparams, gpu_sides: true, rp: [0; 12], reach_rounds: 0, lazy: false };
        p.write_table(w);
        Ok(p)
    }

    /// The 26-neighbour slot table, the outside cell, then the substances
    /// and the reaction's tables. (In a sparse world the slot table is
    /// rebuilt at each regrid, in brick-key order.)
    fn write_table(&mut self, w: &World) {
        let (words, mut rp) = Self::table_words(w, self.slots, &self.slot_of);
        rp[9] = (2 * self.copy_words) as u32;
        self.queue.write_buffer(&self.table, 0, bytemuck::cast_slice(&words));
        self.queue.write_buffer(&self.rparams, 0, bytemuck::cast_slice(&rp));
        self.rp = rp;
    }

    fn table_words(w: &World, slots: usize, slot_of: &[usize]) -> (Vec<u32>, [u32; 12]) {
        let mut t = vec![NONE_SLOT; slots * 26];
        let steps = neighbour_steps();
        for b in 0..slots {
            let (bx, by, bz) = (b % w.bx, b / w.bx % w.by, b / (w.bx * w.by));
            for (k, d) in steps.iter().enumerate() {
                let (x, y, z) = (bx as i64 + d[0], by as i64 + d[1], bz as i64 + d[2]);
                if x >= 0 && y >= 0 && z >= 0 && (x as usize) < w.bx && (y as usize) < w.by && (z as usize) < w.bz {
                    let nb = (z as usize * w.by + y as usize) * w.bx + x as usize;
                    t[slot_of[b] * 26 + k] = slot_of[nb] as u32;
                }
            }
        }
        let mut words = t;
        crate::pack_cell(&mut words, &w.outside_air);
        pack_side(&mut words, &side(&w.outside_air, &w.subs, w.ambient));
        let p64 = |v: &mut Vec<u32>, x: i64| { v.push(x as u32); v.push((x as u64 >> 32) as u32) };
        let mut rp = [0u32; 12];
        rp[0] = words.len() as u32;
        for s in &w.subs {
            let start = words.len();
            p64(&mut words, s.full_mass.0); p64(&mut words, s.porosity.0 as i64); words.push(s.permeable as u32);
            p64(&mut words, s.surface_per_volume); p64(&mut words, s.composition[0]); p64(&mut words, s.specific_heat.0);
            p64(&mut words, s.conductivity); p64(&mut words, s.volatile_chemical.0);
            words.resize(start + SUB_WORDS, 0);
        }
        let t = fv_core::tables::tables();
        rp[1] = words.len() as u32;
        words.extend(t.heating_permille.iter().map(|&v| u32::try_from(v).unwrap()));
        for (k, table) in [&t.pyrolysis, &t.char_burn, &t.gas_burn, &t.soot_burn].into_iter().enumerate() {
            rp[2 + k] = words.len() as u32;
            for f in table { p64(&mut words, f.0 as i64) }
        }
        rp[6] = words.len() as u32;
        for &v in &t.saturated_ug_per_l { p64(&mut words, v) }
        rp[7] = words.len() as u32;
        for (sigma, row) in &t.surface_g { p64(&mut words, *sigma); words.extend(row.iter().copied()) }
        rp[8] = t.surface_g.len() as u32;
        (words, rp)
    }

    fn gas_words(&self) -> usize { if self.compact { 1 } else { 2 } }
    fn heat_base(&self) -> usize { self.slots * BRICK_CELLS * GASES * self.gas_words() }

    /// The state as one copy's words: field groups struct-of-arrays per slot.
    fn pack_state(&self, w: &World) -> Vec<u32> {
        let mut state = vec![0u32; self.copy_words];
        let (gw, heat_base) = (self.gas_words(), self.heat_base());
        let put = |v: &mut [u32], o: usize, x: i64| { v[o] = x as u32; v[o + 1] = (x as u64 >> 32) as u32 };
        for b in 0..self.slots {
            let s = self.slot_of[b];
            for l in 0..BRICK_CELLS {
                let c = &w.cells[b * BRICK_CELLS + l];
                for k in 0..GASES {
                    let o = gw * ((s * GASES + k) * BRICK_CELLS + l);
                    if self.compact { state[o] = u32::try_from(c.gas[k].0).expect("a gas mass outside the compact cell's u32 range") } else { put(&mut state, o, c.gas[k].0) }
                }
                for (f, e) in [c.energy, c.solid_energy, c.skin].into_iter().enumerate() { put(&mut state, heat_base + 2 * ((s * 3 + f) * BRICK_CELLS + l), e.0) }
            }
        }
        state
    }

    /// The solid group (single copy): per slot, 14 fields × 512 cells.
    fn pack_solid(&self, w: &World) -> Vec<u32> {
        let mut v = vec![0u32; self.slots * BRICK_CELLS * SOLID_WORDS];
        for b in 0..self.slots {
            let s = self.slot_of[b];
            for l in 0..BRICK_CELLS {
                let c = &w.cells[b * BRICK_CELLS + l];
                let mut put = |f: usize, x: u32| v[(s * SOLID_WORDS + f) * BRICK_CELLS + l] = x;
                put(0, c.substance as u32 | (c.bonds as u32) << 8 | (c.front as u32) << 16);
                put(1, c.lit as u32);
                for (j, x) in [c.solid[0].0, c.solid[1].0, c.solid[2].0, c.water.0, c.nuclear.0, c.flame.0].into_iter().enumerate() {
                    put(2 + 2 * j, x as u32);
                    put(3 + 2 * j, (x as u64 >> 32) as u32);
                }
            }
        }
        v
    }

    fn pack_sides(&self, w: &World) -> Vec<u32> {
        let ss = sides(w);
        let mut sv = vec![0u32; self.slots * BRICK_CELLS * SIDE_WORDS];
        let mut one = Vec::with_capacity(SIDE_WORDS);
        for b in 0..self.slots {
            for l in 0..BRICK_CELLS {
                one.clear();
                pack_side(&mut one, &ss[b * BRICK_CELLS + l]);
                sv[(self.slot_of[b] * BRICK_CELLS + l) * SIDE_WORDS..][..SIDE_WORDS].copy_from_slice(&one);
            }
        }
        sv
    }

    /// Face velocities: each cell's three + faces, then each slot's lower
    /// boundary faces (read only where no brick lies below).
    fn pack_vel(&self, w: &World, vel: &dyn FaceVelocity) -> Vec<i32> {
        let mut vv = vec![0i32; self.slots * 3 * (BRICK_CELLS + 64)];
        let vlow = self.slots * 3 * BRICK_CELLS;
        let c32 = |v: i64| i32::try_from(v).expect("face velocity fits in i32");
        for b in 0..self.slots {
            let s = self.slot_of[b];
            let (ox, oy, oz) = w.brick_origin(b);
            for l in 0..BRICK_CELLS {
                let (lx, ly, lz) = World::local_xyz(l);
                let (x, y, z) = ((ox + lx) as i64, (oy + ly) as i64, (oz + lz) as i64);
                for a in 0..3 { vv[(s * 3 + a) * BRICK_CELLS + l] = c32(vel.face(x, y, z, a)) }
                for a in 0..3 {
                    if [lx, ly, lz][a] != 0 { continue }
                    let j = match a { 0 => ly + B * lz, 1 => lx + B * lz, _ => lx + B * ly };
                    let mut q = [x, y, z];
                    q[a] -= 1;
                    vv[vlow + (s * 3 + a) * 64 + j] = c32(vel.face(q[0], q[1], q[2], a));
                }
            }
        }
        vv
    }

    fn params(&self, w: &World, in_copy: u32, steps: i64, accumulate: bool) -> [u32; 16] {
        let tiles = (self.slots * 8) as u32;
        let ot = w.outside_air.gas_temperature().0;
        [self.slots as u32, in_copy, 1 - in_copy, self.copy_words as u32, self.heat_base() as u32, (self.slots * 3 * BRICK_CELLS) as u32, (self.slots * 26) as u32,
            w.ambient.0 as u32, (w.ambient.0 as u64 >> 32) as u32, tiles.min(65_535), self.compact as u32, (self.slots * 8 * 16) as u32,
            steps as u32, ot as u32, (ot as u64 >> 32) as u32, accumulate as u32]
    }

    /// Runs kernels in order, each with its own parameters, timing each.
    fn run(&mut self, jobs: &[(usize, [u32; 16])]) {
        let tiles = (self.slots * 8) as u32;
        let tiles_x = tiles.min(65_535);
        let groups = (tiles_x, tiles.div_ceil(tiles_x));
        for (k, params) in jobs {
            let t0 = Instant::now();
            self.queue.write_buffer(&self.params, 0, bytemuck::cast_slice(params));
            let mut enc = self.device.create_command_encoder(&Default::default());
            {
                let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("pool"), timestamp_writes: None });
                pass.set_pipeline(&self.pipelines[*k]);
                pass.set_bind_group(0, &self.bind_group, &[]);
                pass.dispatch_workgroups(groups.0, groups.1, 1);
            }
            self.queue.submit([enc.finish()]);
            self.device.poll(wgpu::PollType::wait_indefinitely()).expect("GPU poll");
            self.kernel_ms.push(ms(t0));
        }
    }

    fn wait(&self) {
        self.queue.submit([]);
        self.device.poll(wgpu::PollType::wait_indefinitely()).expect("GPU poll");
    }

    /// Reads back copy `copy` and the tallies, and writes them into the world
    /// as `World::run_phase` would (the tallies are added).
    /// After diffusion or carrying: the state (unless lazy) and the tallies.
    fn read_into(&mut self, w: &mut World, copy: u32, t: &mut Timings) {
        if self.lazy { self.read_tallies(w, t) } else { self.read_back(w, copy, t, true, false) }
    }

    /// Only the tallies (added to the world's) and the compact-range flag.
    fn read_tallies(&mut self, w: &mut World, t: &mut Timings) {
        let t0 = Instant::now();
        let words = self.slots * 8 * 16 + 1;
        let mut enc = self.device.create_command_encoder(&Default::default());
        enc.copy_buffer_to_buffer(&self.tallies, 0, &self.read, 0, (words * 4) as u64);
        self.queue.submit([enc.finish()]);
        let slice = self.read.slice(..(words * 4) as u64);
        slice.map_async(wgpu::MapMode::Read, |r| r.expect("map readback"));
        self.device.poll(wgpu::PollType::wait_indefinitely()).expect("GPU poll");
        let data: Vec<u32> = bytemuck::cast_slice(&slice.get_mapped_range().expect("mapped")).to_vec();
        self.read.unmap();
        t.readback += ms(t0);
        assert_eq!(data[self.slots * 8 * 16], 0, "a gas mass left the compact cell's u32 range");
        let get = |o: usize| (data[o] as u64 | (data[o + 1] as u64) << 32) as i64;
        for b in 0..self.slots {
            let s = self.slot_of[b];
            let mut tally = fv_core::world::Tally::default();
            for tile in 0..8 {
                let o = (s * 8 + tile) * 16;
                tally.energy_in += get(o);
                for k in 0..GASES { tally.gas_in[k] += get(o + 2 + 2 * k) }
            }
            w.tallies[b].add(&tally);
        }
    }

    /// Reads back copy `copy`, the tallies (added to the world's when
    /// `tallies`), and when `solid` the solid group too.
    fn read_back(&mut self, w: &mut World, copy: u32, t: &mut Timings, tallies: bool, solid: bool) {
        let t0 = Instant::now();
        let solid_words = self.slots * BRICK_CELLS * SOLID_WORDS;
        let tb = self.copy_words + solid_words;
        let mut enc = self.device.create_command_encoder(&Default::default());
        enc.copy_buffer_to_buffer(&self.state, (copy as usize * self.copy_words * 4) as u64, &self.read, 0, (self.copy_words * 4) as u64);
        if solid { enc.copy_buffer_to_buffer(&self.state, (2 * self.copy_words * 4) as u64, &self.read, (self.copy_words * 4) as u64, (solid_words * 4) as u64); }
        enc.copy_buffer_to_buffer(&self.tallies, 0, &self.read, (tb * 4) as u64, ((self.slots * 8 * 16 + 1) * 4) as u64);
        self.queue.submit([enc.finish()]);
        let slice = self.read.slice(..);
        slice.map_async(wgpu::MapMode::Read, |r| r.expect("map readback"));
        self.device.poll(wgpu::PollType::wait_indefinitely()).expect("GPU poll");
        let data: Vec<u32> = bytemuck::cast_slice(&slice.get_mapped_range().expect("mapped")).to_vec();
        self.read.unmap();
        t.readback += ms(t0);

        let t0 = Instant::now();
        let (gw, heat_base) = (self.gas_words(), self.heat_base());
        let get = |o: usize| (data[o] as u64 | (data[o + 1] as u64) << 32) as i64;
        assert_eq!(data[tb + self.slots * 8 * 16], 0, "a gas mass left the compact cell's u32 range");
        let mut next = core::mem::take(&mut w.next);
        for b in 0..self.slots {
            let s = self.slot_of[b];
            for l in 0..BRICK_CELLS {
                let mut c = w.cells[b * BRICK_CELLS + l];
                for k in 0..GASES {
                    let o = gw * ((s * GASES + k) * BRICK_CELLS + l);
                    c.gas[k].0 = if self.compact { data[o] as i64 } else { get(o) };
                }
                c.energy.0 = get(heat_base + 2 * (s * 3 * BRICK_CELLS + l));
                c.solid_energy.0 = get(heat_base + 2 * ((s * 3 + 1) * BRICK_CELLS + l));
                c.skin.0 = get(heat_base + 2 * ((s * 3 + 2) * BRICK_CELLS + l));
                if solid {
                    let f = |k: usize| data[self.copy_words + (s * SOLID_WORDS + k) * BRICK_CELLS + l];
                    let f64_ = |k: usize| (f(k) as u64 | (f(k + 1) as u64) << 32) as i64;
                    let meta = f(0);
                    c.substance = meta as u8; c.bonds = (meta >> 8) as u8; c.front = (meta >> 16) as u16;
                    c.lit = f(1) != 0;
                    for j in 0..3 { c.solid[j].0 = f64_(2 + 2 * j) }
                    c.water.0 = f64_(8); c.nuclear.0 = f64_(10); c.flame.0 = f64_(12);
                }
                next[b * BRICK_CELLS + l] = c;
            }
            if !tallies { continue }
            let mut tally = fv_core::world::Tally::default();
            for tile in 0..8 {
                let o = tb + ((s * 8 + tile) * 16);
                tally.energy_in += get(o);
                for k in 0..GASES { tally.gas_in[k] += get(o + 2 + 2 * k) }
            }
            w.tallies[b].add(&tally);
        }
        core::mem::swap(&mut w.cells, &mut next);
        w.next = next;
        t.unpack += ms(t0);
    }

    /// Uploads the state (into copy 0) and the sides.
    fn upload(&mut self, w: &World, t: &mut Timings) {
        let t0 = Instant::now();
        let state = self.pack_state(w);
        let solid = self.pack_solid(w);
        let sv = if self.gpu_sides && self.mode == Mode::Scratch { Vec::new() } else { self.pack_sides(w) };
        t.prepare += ms(t0);
        let t0 = Instant::now();
        self.queue.write_buffer(&self.state, 0, bytemuck::cast_slice(&state));
        self.queue.write_buffer(&self.state, (2 * self.copy_words * 4) as u64, bytemuck::cast_slice(&solid));
        if !sv.is_empty() { self.queue.write_buffer(&self.sides, 0, bytemuck::cast_slice(&sv)) }
        self.wait();
        t.upload += ms(t0);
    }

    fn upload_vel(&mut self, w: &World, vel: &dyn FaceVelocity, t: &mut Timings) {
        let t0 = Instant::now();
        let vv = self.pack_vel(w, vel);
        t.prepare += ms(t0);
        let t0 = Instant::now();
        self.queue.write_buffer(&self.vel, 0, bytemuck::cast_slice(&vv));
        self.queue.write_buffer(&self.tallies, (self.slots * 8 * 16 * 4) as u64, bytemuck::cast_slice(&[0u32]));
        self.wait();
        t.upload += ms(t0);
    }

    /// `chem::flame_reach` on the GPU, from copy `copy`: seeds, then rounds of
    /// propagation (each tile to a standstill) until no tile changes. Leaves
    /// each cell's flag where sides_main and react_main read it.
    fn reach(&mut self, w: &World, copy: u32, t: &mut Timings) {
        let t0 = Instant::now();
        let p = self.params(w, copy, 1, false);
        self.run(&[(18, p)]);
        let flags = self.slots * 8 * 16;
        let mut round = 0u32;
        loop {
            self.rp[10] = round % 2;
            self.queue.write_buffer(&self.rparams, 0, bytemuck::cast_slice(&self.rp));
            self.run(&[(19, p)]);
            round += 1;
            // Did any tile change? (One word per workgroup, every 16th of the tallies.)
            let mut enc = self.device.create_command_encoder(&Default::default());
            enc.copy_buffer_to_buffer(&self.tallies, 0, &self.read, 0, (flags * 4) as u64);
            self.queue.submit([enc.finish()]);
            let slice = self.read.slice(..(flags * 4) as u64);
            slice.map_async(wgpu::MapMode::Read, |r| r.expect("map readback"));
            self.device.poll(wgpu::PollType::wait_indefinitely()).expect("GPU poll");
            let changed = bytemuck::cast_slice::<u8, u32>(&slice.get_mapped_range().expect("mapped")).iter().step_by(16).any(|&v| v != 0);
            self.read.unmap();
            if !changed { break }
        }
        // The last round wrote word 1 − its input, unchanged from it.
        self.rp[11] = 1 - (round - 1) % 2;
        self.queue.write_buffer(&self.rparams, 0, bytemuck::cast_slice(&self.rp));
        self.reach_rounds = round;
        t.compute += ms(t0);
    }

    /// `chem::flame_reach` worked out on the GPU from `w` (uploaded), as one
    /// flag per cell in world order: for checking against the CPU's.
    pub fn flame_reach(&mut self, w: &World) -> Vec<bool> {
        let mut t = Timings::default();
        self.upload(w, &mut t);
        self.resident = None;
        self.reach(w, 0, &mut t);
        let words = self.slots * BRICK_CELLS * 2;
        let mut enc = self.device.create_command_encoder(&Default::default());
        enc.copy_buffer_to_buffer(&self.limits, 0, &self.read, 0, (words * 4) as u64);
        self.queue.submit([enc.finish()]);
        let slice = self.read.slice(..(words * 4) as u64);
        slice.map_async(wgpu::MapMode::Read, |r| r.expect("map readback"));
        self.device.poll(wgpu::PollType::wait_indefinitely()).expect("GPU poll");
        let data: Vec<u32> = bytemuck::cast_slice(&slice.get_mapped_range().expect("mapped")).to_vec();
        self.read.unmap();
        let word = self.rp[11] as usize;
        (0..w.cells.len()).map(|i| {
            let c = self.slot_of[i / BRICK_CELLS] * BRICK_CELLS + i % BRICK_CELLS;
            data[2 * c + word] & 1 != 0
        }).collect()
    }

    /// One diffusion phase: leaves `w` exactly as `transport::diffuse` would.
    /// The result stays on the GPU for carrying in the same tick.
    pub fn diffuse(&mut self, w: &mut World, vel: &dyn FaceVelocity) {
        let mut t = Timings::default();
        self.kernel_ms.clear();
        self.upload(w, &mut t);
        self.upload_vel(w, vel, &mut t);
        let p = self.params(w, 0, 1, false);
        let mut jobs: Vec<(usize, [u32; 16])> = if self.mode == Mode::Scratch { vec![(2, p), (3, p), (4, p)] } else { vec![(0, p), (1, p)] };
        if self.gpu_sides && self.mode == Mode::Scratch {
            self.reach(w, 0, &mut t);
            jobs.insert(0, (17, p));
        }
        self.run(&jobs);
        t.compute = self.kernel_ms.iter().sum();
        self.read_into(w, 1, &mut t);
        self.resident = Some((w.tick, 1));
        self.last = t;
    }

    /// Carrying (`transport::advect`) in `steps` sub-steps: leaves `w`
    /// exactly as `advect` would. If this tick's diffusion ran here, it
    /// starts from the state already on the GPU: only the new face
    /// velocities go up. (Needs the face-scratch mode: its scratch holds
    /// the outflow totals.)
    pub fn advect(&mut self, w: &mut World, vel: &dyn FaceVelocity, steps: i64) {
        assert_eq!(self.mode, Mode::Scratch, "carrying runs on the face-scratch pool");
        let mut t = Timings::default();
        self.kernel_ms.clear();
        let mut cur = match self.resident.take() {
            Some((tick, copy)) if tick == w.tick => copy,
            _ => { self.upload(w, &mut t); 0 }
        };
        self.upload_vel(w, vel, &mut t);
        let mut jobs = Vec::new();
        for i in 0..steps {
            let p = self.params(w, cur, steps, i > 0);
            if self.carry_once { jobs.push((7, p)); jobs.push((8, p)); jobs.push((9, p)) } else { jobs.push((5, p)); jobs.push((6, p)) }
            cur = 1 - cur;
        }
        self.run(&jobs);
        t.compute = self.kernel_ms.iter().sum();
        self.read_into(w, cur, &mut t);
        self.resident = Some((w.tick, cur));
        self.last = t;
    }

    /// One tick of the flow (`Flow::step`): leaves `flow` exactly as it
    /// would. Reads the cells this tick's diffusion left on the GPU (or
    /// uploads them); the flow's block velocities go up and come back
    /// (12 B a block), so `Flow` stays the copy the CPU phases read.
    pub fn flow_step(&mut self, w: &World, flow: &mut fv_core::flow::Flow) {
        assert_eq!(self.mode, Mode::Scratch, "the flow runs on the face-scratch pool");
        let mut t = Timings::default();
        self.kernel_ms.clear();
        let cur = match self.resident {
            Some((tick, copy)) if tick == w.tick => copy,
            _ => { self.upload(w, &mut t); self.resident = Some((w.tick, 0)); 0 }
        };
        // The wind and its gusts, exactly as Flow::step works them out.
        let g = (fv_core::rng::hash(w.seed, w.tick / 40, 0, 0x91) % 801) as i64 - 400;
        flow.wind = [flow.wind_mean[0] + flow.wind_mean[0] * g / 1000, flow.wind_mean[1] + flow.wind_mean[1] * g / 1000];
        let nb = flow.vel.len();
        let t0 = Instant::now();
        let vel: Vec<i32> = flow.vel.iter().flat_map(|v| v.iter().copied()).collect();
        let map_at = self.slots * BRICK_CELLS / 32 + 1;
        let map: Vec<u32> = self.slot_of.iter().map(|&s| s as u32).collect();
        self.queue.write_buffer(&self.faces, 0, bytemuck::cast_slice(&vel));
        // Last tick's pressure, where this tick's solve starts (Flow::step).
        if flow.pressure.len() != nb { flow.pressure = vec![0; nb] }
        self.queue.write_buffer(&self.faces, (21 * nb * 4) as u64, bytemuck::cast_slice(&flow.pressure));
        self.queue.write_buffer(&self.aux, (map_at * 4) as u64, bytemuck::cast_slice(&map));
        let p = self.params(w, cur, 1, false);
        self.queue.write_buffer(&self.params, 0, bytemuck::cast_slice(&p));
        let tick3 = w.tick / 3;
        let fp = |p_in: u32| -> [u32; 16] {
            [flow.cx as u32, flow.cy as u32, flow.cz as u32, nb as u32, flow.wind[0] as i32 as u32, flow.wind[1] as i32 as u32, w.bx as u32, w.by as u32,
             w.seed as u32, (w.seed >> 32) as u32, tick3 as u32, (tick3 >> 32) as u32, map_at as u32, p_in, 0, 0]
        };
        t.upload = ms(t0);
        // gather, carried, forced, div, the pressure rounds, project.
        let rounds = fv_core::flow::PRESSURE_ROUNDS as u32;
        assert!(rounds % 2 == 0, "the pressure ping-pong must end in o_p(0)");
        let mut jobs: Vec<(usize, u32)> = vec![(11, 0), (12, 0), (13, 0), (14, 0)];
        for r in 0..rounds { jobs.push((15, r % 2)) }
        jobs.push((16, 0));
        let t0 = Instant::now();
        let groups = (nb as u32).div_ceil(64);
        for (k, p_in) in jobs {
            self.queue.write_buffer(&self.fparams, 0, bytemuck::cast_slice(&fp(p_in)));
            let mut enc = self.device.create_command_encoder(&Default::default());
            {
                let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("flow"), timestamp_writes: None });
                pass.set_pipeline(&self.pipelines[k]);
                pass.set_bind_group(0, &self.bind_group, &[]);
                pass.dispatch_workgroups(groups, 1, 1);
            }
            self.queue.submit([enc.finish()]);
        }
        self.wait();
        t.compute = ms(t0);
        self.kernel_ms.push(t.compute);
        // The project step read the pressure from p_in = 0 (an even number of
        // rounds ends there) and wrote velocities B; both come back.
        let t0 = Instant::now();
        let bytes = (3 * nb * 4) as u64;
        let pbytes = (2 * nb * 4) as u64;
        let mut enc = self.device.create_command_encoder(&Default::default());
        enc.copy_buffer_to_buffer(&self.faces, bytes, &self.read, 0, bytes);
        enc.copy_buffer_to_buffer(&self.faces, (21 * nb * 4) as u64, &self.read, bytes, pbytes);
        self.queue.submit([enc.finish()]);
        let slice = self.read.slice(..bytes + pbytes);
        slice.map_async(wgpu::MapMode::Read, |r| r.expect("map readback"));
        self.device.poll(wgpu::PollType::wait_indefinitely()).expect("GPU poll");
        let raw = slice.get_mapped_range().expect("mapped").to_vec();
        self.read.unmap();
        let data: Vec<i32> = bytemuck::cast_slice(&raw[..bytes as usize]).to_vec();
        let pressure: Vec<i64> = bytemuck::pod_collect_to_vec(&raw[bytes as usize..]);
        for (i, v) in flow.vel.iter_mut().enumerate() { *v = [data[3 * i], data[3 * i + 1], data[3 * i + 2]] }
        flow.pressure = pressure;
        t.readback = ms(t0);
        self.last = t;
    }

    /// The reaction phase (`chem::react_in`): leaves `w` exactly as it
    /// would. Starts from the state carrying left on the GPU this tick when
    /// there is one; only the pilot flags (`chem::flame_reach`, still on
    /// the CPU) go up.
    pub fn react(&mut self, w: &mut World, vel: &dyn FaceVelocity) {
        assert_eq!(self.mode, Mode::Scratch, "reaction runs on the face-scratch pool");
        let mut t = Timings::default();
        self.kernel_ms.clear();
        let resident = self.resident.take();
        let cur = match resident {
            Some((tick, copy)) if tick == w.tick => copy,
            _ => { self.upload(w, &mut t); self.upload_vel(w, vel, &mut t); 0 }
        };
        self.reach(w, cur, &mut t);
        let p = self.params(w, cur, 1, false);
        self.run(&[(10, p)]);
        t.compute = self.kernel_ms.iter().sum();
        self.read_back(w, 1 - cur, &mut t, false, true);
        self.last = t;
    }
}
