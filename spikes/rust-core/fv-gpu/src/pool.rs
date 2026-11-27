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

/// Words of state per cell in one copy: 7 gases and 3 energies, i64 each.
pub const STATE_WORDS: usize = 2 * (GASES + 3);
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
    pipelines: [wgpu::ComputePipeline; 2],
    bind_groups: [wgpu::BindGroup; 2],
    params: wgpu::Buffer,
    state: wgpu::Buffer,
    sides: wgpu::Buffer,
    vel: wgpu::Buffer,
    table: wgpu::Buffer,
    tallies: wgpu::Buffer,
    read: wgpu::Buffer,
    /// Brick b of the world is in slot `slot_of[b]`.
    pub slot_of: Vec<usize>,
    slots: usize,
    copy_words: usize,
    pub last: Timings,
    pub kernel_ms: [f64; 2],
}

impl PoolDiffusion {
    pub fn from_world(w: &World, slot_of: Vec<usize>) -> Result<PoolDiffusion, String> {
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
        let copy_words = cells * STATE_WORDS;
        use wgpu::BufferUsages as U;
        let buf = |label: &str, words: usize, usage: wgpu::BufferUsages| device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: (words * 4) as u64, usage, mapped_at_creation: false });
        let params = buf("params", 12, U::UNIFORM | U::COPY_DST);
        let state = buf("state", 2 * copy_words, U::STORAGE | U::COPY_DST | U::COPY_SRC);
        let sides_b = buf("sides", cells * SIDE_WORDS, U::STORAGE | U::COPY_DST);
        let vel = buf("vel", slots * 3 * (BRICK_CELLS + 64), U::STORAGE | U::COPY_DST);
        let table = buf("table", slots * 26 + 20 + SIDE_WORDS, U::STORAGE | U::COPY_DST);
        let lim = buf("limits", cells * 2, U::STORAGE);
        let tallies = buf("tallies", slots * 8 * 16, U::STORAGE | U::COPY_SRC);
        let read = buf("read", copy_words + slots * 8 * 16, U::MAP_READ | U::COPY_DST);
        let all = [&params, &state, &sides_b, &vel, &table, &lim, &tallies];
        let names = ["limits_pass", "apply_pass"];
        let uses: [&[u32]; 2] = [&[0, 1, 2, 3, 4, 5], &[0, 1, 2, 3, 4, 5, 6]];
        let pipelines = names.map(|e| device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor { label: Some(e), layout: None, module: &module, entry_point: Some(e), compilation_options: Default::default(), cache: None }));
        let bind_groups = std::array::from_fn(|k| {
            let entries: Vec<wgpu::BindGroupEntry> = uses[k].iter().map(|&b| wgpu::BindGroupEntry { binding: b, resource: all[b as usize].as_entire_binding() }).collect();
            device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some(names[k]), layout: &pipelines[k].get_bind_group_layout(0), entries: &entries })
        });
        let mut p = PoolDiffusion { device, queue, adapter_info: info, pipelines, bind_groups, params, state, sides: sides_b, vel, table, tallies, read, slot_of, slots, copy_words, last: Timings::default(), kernel_ms: [0.0; 2] };
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

    /// One diffusion phase: leaves `w` exactly as `transport::diffuse` would.
    pub fn diffuse(&mut self, w: &mut World, vel: &dyn FaceVelocity) {
        let cells = self.slots * BRICK_CELLS;
        let mut t = Timings::default();
        let t0 = Instant::now();
        // The state into copy 0, field groups struct-of-arrays per slot.
        let mut state = vec![0u32; self.copy_words];
        let heat_base = cells * GASES * 2;
        let put = |v: &mut [u32], o: usize, x: i64| { v[o] = x as u32; v[o + 1] = (x as u64 >> 32) as u32 };
        let ss = sides(w);
        let mut sv = vec![0u32; cells * SIDE_WORDS];
        let mut vv = vec![0i32; self.slots * 3 * (BRICK_CELLS + 64)];
        let vlow = self.slots * 3 * BRICK_CELLS;
        let c32 = |v: i64| i32::try_from(v).expect("face velocity fits in i32");
        for b in 0..self.slots {
            let s = self.slot_of[b];
            let (ox, oy, oz) = w.brick_origin(b);
            for l in 0..BRICK_CELLS {
                let c = &w.cells[b * BRICK_CELLS + l];
                for k in 0..GASES { put(&mut state, 2 * ((s * GASES + k) * BRICK_CELLS + l), c.gas[k].0) }
                for (f, e) in [c.energy, c.solid_energy, c.skin].into_iter().enumerate() { put(&mut state, heat_base + 2 * ((s * 3 + f) * BRICK_CELLS + l), e.0) }
                let mut one = Vec::with_capacity(SIDE_WORDS);
                pack_side(&mut one, &ss[b * BRICK_CELLS + l]);
                sv[(s * BRICK_CELLS + l) * SIDE_WORDS..][..SIDE_WORDS].copy_from_slice(&one);
                let (lx, ly, lz) = World::local_xyz(l);
                let (x, y, z) = ((ox + lx) as i64, (oy + ly) as i64, (oz + lz) as i64);
                for a in 0..3 { vv[(s * 3 + a) * BRICK_CELLS + l] = c32(vel.face(x, y, z, a)) }
                // The brick's lower boundary faces (read only where no brick is below).
                for a in 0..3 {
                    let p = [lx, ly, lz];
                    if p[a] != 0 { continue }
                    let j = match a { 0 => ly + B * lz, 1 => lx + B * lz, _ => lx + B * ly };
                    let mut q = [x, y, z];
                    q[a] -= 1;
                    vv[vlow + (s * 3 + a) * 64 + j] = c32(vel.face(q[0], q[1], q[2], a));
                }
            }
        }
        let tiles = (self.slots * 8) as u32;
        let tiles_x = if tiles <= 65_535 { tiles } else { 65_535 };
        let params: [u32; 12] = [self.slots as u32, 0, 1, self.copy_words as u32, heat_base as u32, vlow as u32, (self.slots * 26) as u32,
            w.ambient.0 as u32, (w.ambient.0 as u64 >> 32) as u32, tiles_x, 0, 0];
        t.prepare = ms(t0);

        let t0 = Instant::now();
        self.queue.write_buffer(&self.params, 0, bytemuck::cast_slice(&params));
        self.queue.write_buffer(&self.state, 0, bytemuck::cast_slice(&state));
        self.queue.write_buffer(&self.sides, 0, bytemuck::cast_slice(&sv));
        self.queue.write_buffer(&self.vel, 0, bytemuck::cast_slice(&vv));
        self.queue.submit([]);
        self.device.poll(wgpu::PollType::wait_indefinitely()).expect("GPU poll");
        t.upload = ms(t0);

        let groups = (tiles_x, tiles.div_ceil(tiles_x));
        for k in 0..2 {
            let t0 = Instant::now();
            let mut enc = self.device.create_command_encoder(&Default::default());
            {
                let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("pool"), timestamp_writes: None });
                pass.set_pipeline(&self.pipelines[k]);
                pass.set_bind_group(0, &self.bind_groups[k], &[]);
                pass.dispatch_workgroups(groups.0, groups.1, 1);
            }
            self.queue.submit([enc.finish()]);
            self.device.poll(wgpu::PollType::wait_indefinitely()).expect("GPU poll");
            self.kernel_ms[k] = ms(t0);
        }
        t.compute = self.kernel_ms[0] + self.kernel_ms[1];

        let t0 = Instant::now();
        let mut enc = self.device.create_command_encoder(&Default::default());
        enc.copy_buffer_to_buffer(&self.state, (self.copy_words * 4) as u64, &self.read, 0, (self.copy_words * 4) as u64);
        enc.copy_buffer_to_buffer(&self.tallies, 0, &self.read, (self.copy_words * 4) as u64, (self.slots * 8 * 16 * 4) as u64);
        self.queue.submit([enc.finish()]);
        let slice = self.read.slice(..);
        slice.map_async(wgpu::MapMode::Read, |r| r.expect("map readback"));
        self.device.poll(wgpu::PollType::wait_indefinitely()).expect("GPU poll");
        let data: Vec<u32> = bytemuck::cast_slice(&slice.get_mapped_range().expect("mapped")).to_vec();
        self.read.unmap();
        t.readback = ms(t0);

        let t0 = Instant::now();
        let get = |o: usize| (data[o] as u64 | (data[o + 1] as u64) << 32) as i64;
        let mut next = core::mem::take(&mut w.next);
        for b in 0..self.slots {
            let s = self.slot_of[b];
            for l in 0..BRICK_CELLS {
                let mut c = w.cells[b * BRICK_CELLS + l];
                for k in 0..GASES { c.gas[k].0 = get(2 * ((s * GASES + k) * BRICK_CELLS + l)) }
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
        t.unpack = ms(t0);
        self.last = t;
    }
}
