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

pub fn pool_shader_source() -> String { constants() + include_str!("common.wgsl") + include_str!("pool.wgsl") }

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
    pipelines: [wgpu::ComputePipeline; 10],
    bind_group: wgpu::BindGroup,
    params: wgpu::Buffer,
    state: wgpu::Buffer,
    sides: wgpu::Buffer,
    vel: wgpu::Buffer,
    table: wgpu::Buffer,
    tallies: wgpu::Buffer,
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
        let state = buf("state", 2 * copy_words, U::STORAGE | U::COPY_DST | U::COPY_SRC);
        let sides_b = buf("sides", cells * SIDE_WORDS, U::STORAGE | U::COPY_DST);
        let vel = buf("vel", slots * 3 * (BRICK_CELLS + 64), U::STORAGE | U::COPY_DST);
        let table = buf("table", slots * 26 + 20 + SIDE_WORDS, U::STORAGE | U::COPY_DST);
        let lim = buf("limits", cells * 2, U::STORAGE);
        // The tallies, then one word flagging a gas mass out of the compact range.
        let tallies = buf("tallies", slots * 8 * 16 + 1, U::STORAGE | U::COPY_SRC | U::COPY_DST);
        let faces = buf("faces", if mode == Mode::Scratch { slots * FACES_PER_SLOT * crate::FLUX_WORDS } else { crate::FLUX_WORDS }, U::STORAGE);
        let read = buf("read", copy_words + slots * 8 * 16 + 1, U::MAP_READ | U::COPY_DST);
        let all = [&params, &state, &sides_b, &vel, &table, &lim, &tallies, &faces];
        // One layout for every kernel: binding 0 uniform, the rest storage
        // (read-only: sides, velocities, table).
        let entries: Vec<wgpu::BindGroupLayoutEntry> = (0..8u32).map(|b| wgpu::BindGroupLayoutEntry {
            binding: b, visibility: wgpu::ShaderStages::COMPUTE, count: None,
            ty: wgpu::BindingType::Buffer {
                ty: if b == 0 { wgpu::BufferBindingType::Uniform } else { wgpu::BufferBindingType::Storage { read_only: matches!(b, 2 | 3 | 4) } },
                has_dynamic_offset: false, min_binding_size: None,
            },
        }).collect();
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("pool"), entries: &entries });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("pool"), bind_group_layouts: &[Some(&bgl)], immediate_size: 0 });
        let names = ["limits_pass", "apply_pass", "faces_scratch", "limits_scratch", "apply_scratch", "carry_prepare", "carry_apply", "carry_prepare2", "carry_faces", "carry_apply2"];
        let pipelines = names.map(|e| device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor { label: Some(e), layout: Some(&layout), module: &module, entry_point: Some(e), compilation_options: Default::default(), cache: None }));
        let bge: Vec<wgpu::BindGroupEntry> = (0..8).map(|b| wgpu::BindGroupEntry { binding: b as u32, resource: all[b].as_entire_binding() }).collect();
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("pool"), layout: &bgl, entries: &bge });
        let mut p = PoolDiffusion { device, queue, adapter_info: info, pipelines, bind_group, params, state, sides: sides_b, vel, table, tallies, faces, read, slot_of, slots, copy_words, mode, compact, last: Timings::default(), kernel_ms: Vec::new(), resident: None, carry_once: true };
        p.write_table(w);
        Ok(p)
    }

    /// The 26-neighbour slot table and the outside cell. (In a sparse world
    /// this is rebuilt at each regrid, in brick-key order.)
    fn write_table(&mut self, w: &World) {
        let mut t = vec![NONE_SLOT; self.slots * 26];
        let steps = neighbour_steps();
        for b in 0..self.slots {
            let (bx, by, bz) = (b % w.bx, b / w.bx % w.by, b / (w.bx * w.by));
            for (k, d) in steps.iter().enumerate() {
                let (x, y, z) = (bx as i64 + d[0], by as i64 + d[1], bz as i64 + d[2]);
                if x >= 0 && y >= 0 && z >= 0 && (x as usize) < w.bx && (y as usize) < w.by && (z as usize) < w.bz {
                    let nb = (z as usize * w.by + y as usize) * w.bx + x as usize;
                    t[self.slot_of[b] * 26 + k] = self.slot_of[nb] as u32;
                }
            }
        }
        let mut words = t;
        crate::pack_cell(&mut words, &w.outside_air);
        pack_side(&mut words, &side(&w.outside_air, &w.subs, w.ambient));
        self.queue.write_buffer(&self.table, 0, bytemuck::cast_slice(&words));
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
    fn read_into(&mut self, w: &mut World, copy: u32, t: &mut Timings) {
        let t0 = Instant::now();
        let mut enc = self.device.create_command_encoder(&Default::default());
        enc.copy_buffer_to_buffer(&self.state, (copy as usize * self.copy_words * 4) as u64, &self.read, 0, (self.copy_words * 4) as u64);
        enc.copy_buffer_to_buffer(&self.tallies, 0, &self.read, (self.copy_words * 4) as u64, ((self.slots * 8 * 16 + 1) * 4) as u64);
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
        assert_eq!(data[self.copy_words + self.slots * 8 * 16], 0, "a gas mass left the compact cell's u32 range");
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
                next[b * BRICK_CELLS + l] = c;
            }
            let mut tally = fv_core::world::Tally::default();
            for tile in 0..8 {
                let o = self.copy_words + ((s * 8 + tile) * 16);
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
        let sv = self.pack_sides(w);
        t.prepare += ms(t0);
        let t0 = Instant::now();
        self.queue.write_buffer(&self.state, 0, bytemuck::cast_slice(&state));
        self.queue.write_buffer(&self.sides, 0, bytemuck::cast_slice(&sv));
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

    /// One diffusion phase: leaves `w` exactly as `transport::diffuse` would.
    /// The result stays on the GPU for carrying in the same tick.
    pub fn diffuse(&mut self, w: &mut World, vel: &dyn FaceVelocity) {
        let mut t = Timings::default();
        self.kernel_ms.clear();
        self.upload(w, &mut t);
        self.upload_vel(w, vel, &mut t);
        let p = self.params(w, 0, 1, false);
        let jobs: Vec<(usize, [u32; 16])> = if self.mode == Mode::Scratch { vec![(2, p), (3, p), (4, p)] } else { vec![(0, p), (1, p)] };
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
        self.last = t;
    }
}
