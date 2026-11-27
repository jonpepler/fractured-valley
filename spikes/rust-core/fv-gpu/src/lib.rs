//! fv-core's diffusion phase on the GPU (wgpu natively, WebGPU in the
//! browser), bit-identical to [`fv_core::transport::diffuse`].
//!
//! The CPU still works out each cell's [`Side`] (it uses floating-point
//! libm calls and a flood fill, see the research notes) and the face
//! velocities; the GPU does the rest of the phase: every face's flux
//! (`diffuse_face`), the flux limiter (`gas_limits`), applying the fluxes to
//! every cell (`diffuse_brick`) and each brick's edge tally. All arithmetic
//! is 64/128-bit integer, emulated with 32-bit words in `diffuse.wgsl`.

use fv_core::cell::Cell;
use fv_core::substance::*;
use fv_core::transport::{side, sides, FaceVelocity, Side};
use fv_core::world::{World, BRICK_CELLS, TICK};
use std::time::Instant;

/// transport.rs's private constants, from the same sources.
const fn parse_permille(v: Option<&str>, default: i64) -> i64 {
    let Some(v) = v else { return default };
    let b = v.as_bytes();
    let (mut i, mut n) = (0, 0i64);
    while i < b.len() { n = n * 10 + (b[i] - b'0') as i64; i += 1 }
    n
}
const FLAME_LEAN_PERMILLE: i64 = parse_permille(option_env!("FV_FLAME_LEAN_PERMILLE"), 500);
const FLAME_CONTACT: i64 = 1_173_000;
const FLAME_ON_FACE: i64 = 80_000;
const FLAME_FACE_G: i64 = FLAME_ON_FACE * FACE_MM2 / 1_000;
const FILM: i64 = 15 * FACE_MM2;
const STILL_MIX_UL: i64 = 50 * CELL_MM;
const TURBULENT_MIX_NL_PER_MM_S: i64 = 50_000 * FACE_MM2 / 10_000;
const MIX_QUANTUM_UL: i64 = CELL_VOLUME_ML;
const SEEP_DIVISOR: i64 = 20;

/// The shader with fv-core's constants in front of it.
pub fn shader_source() -> String { constants() + include_str!("common.wgsl") + include_str!("diffuse.wgsl") }

/// fv-core's constants as WGSL.
pub fn constants() -> String {
    let i32c = |v: i64| -> i32 { i32::try_from(v).expect("constant fits in i32") };
    let sh: Vec<String> = GAS_SPECIFIC_HEAT.iter().map(|c| i32c(c.0).to_string()).collect();
    format!(
        "const C_GAS_SH = array<i32, 7>({});\nconst C_VAPOUR: u32 = {}u;\nconst C_LATENT: i32 = {};\nconst C_TICK: i32 = {};\nconst C_CELL_MM: i32 = {};\n\
         const C_FLAME_LEAN: i32 = {};\nconst C_FLAME_CONTACT: i32 = {};\nconst C_FLAME_FACE_G: i32 = {};\nconst C_FILM: i32 = {};\n\
         const C_STILL_MIX: i32 = {};\nconst C_TURB_MIX: i32 = {};\nconst C_MIX_QUANTUM: i32 = {};\nconst C_SEEP: i32 = {};\n",
        sh.join(", "), Gas::Vapour as usize, i32c(LATENT_HEAT_VAPOUR.0), i32c(TICK.0), i32c(CELL_MM),
        i32c(FLAME_LEAN_PERMILLE), i32c(FLAME_CONTACT), i32c(FLAME_FACE_G), i32c(FILM),
        i32c(STILL_MIX_UL), i32c(TURBULENT_MIX_NL_PER_MM_S), i32c(MIX_QUANTUM_UL), i32c(SEEP_DIVISOR),
    )
}

pub const CELL_WORDS: usize = 20;
pub const SIDE_WORDS: usize = 25;
pub const FLUX_WORDS: usize = 21;
pub const EDGE_WORDS: usize = 16;

fn push64(v: &mut Vec<u32>, x: i64) { v.push(x as u32); v.push((x as u64 >> 32) as u32) }

/// A cell's gas and energy stores, as the shader reads them.
pub fn pack_cell(v: &mut Vec<u32>, c: &Cell) {
    for k in 0..GASES { push64(v, c.gas[k].0) }
    push64(v, c.energy.0);
    push64(v, c.solid_energy.0);
    push64(v, c.skin.0);
}

pub fn pack_side(v: &mut Vec<u32>, s: &Side) {
    for x in [s.t.0, s.t_cond.0, s.t_skin.0, s.gas_ml, s.half.0, s.excess.0, s.flame_t.0, s.flame_heat.0, s.flame_fill, s.fuel_g, s.reached, s.water_g] { push64(v, x) }
    v.push(s.permeable as u32 | (s.wet as u32) << 1);
}

/// Everything the shader reads, packed: the input of one diffusion phase.
pub struct Inputs { pub params: [u32; 16], pub cells: Vec<u32>, pub sides: Vec<u32>, pub vel: Vec<i32> }

/// Packs the world (and the CPU's per-cell sides) for the shader.
pub fn inputs(w: &World, vel: &dyn FaceVelocity) -> Inputs {
    let n = w.cells.len();
    let ss = sides(w);
    let outside = side(&w.outside_air, &w.subs, w.ambient);
    let mut cells = Vec::with_capacity((n + 1) * CELL_WORDS);
    for c in &w.cells { pack_cell(&mut cells, c) }
    pack_cell(&mut cells, &w.outside_air);
    let mut sv = Vec::with_capacity((n + 1) * SIDE_WORDS);
    for s in &ss { pack_side(&mut sv, s) }
    pack_side(&mut sv, &outside);
    Inputs { params: params(w), cells, sides: sv, vel: velocities(w, vel) }
}

fn params(w: &World) -> [u32; 16] {
    let n = w.cells.len() as u32;
    let (nx, ny, nz) = (w.nx as u32, w.ny as u32, w.nz as u32);
    let l0 = 3 * n;
    let l1 = l0 + ny * nz;
    let l2 = l1 + nx * nz;
    let nlower = ny * nz + nx * nz + nx * ny;
    [nx, ny, nz, w.bx as u32, w.by as u32, n, l0, l1, l2, nlower, w.ambient.0 as u32, (w.ambient.0 as u64 >> 32) as u32, w.bricks() as u32, 0, 0, 0]
}

/// Face velocities as the shader reads them: the three + faces of each cell
/// (cell index order), then the faces on the box's lower edges (as
/// `transport::Faces`).
pub fn velocities(w: &World, vel: &dyn FaceVelocity) -> Vec<i32> {
    let (nx, ny, nz) = (w.nx, w.ny, w.nz);
    let n = w.cells.len();
    let c = |v: i64| i32::try_from(v).expect("face velocity fits in i32");
    let mut out = vec![0i32; 3 * n + ny * nz + nx * nz + nx * ny];
    for i in 0..n {
        let (ox, oy, oz) = w.brick_origin(i / BRICK_CELLS);
        let (lx, ly, lz) = World::local_xyz(i % BRICK_CELLS);
        let (x, y, z) = ((ox + lx) as i64, (oy + ly) as i64, (oz + lz) as i64);
        for a in 0..3 { out[3 * i + a] = c(vel.face(x, y, z, a)) }
    }
    let mut o = 3 * n;
    for k in 0..ny * nz { out[o + k] = c(vel.face(-1, (k % ny) as i64, (k / ny) as i64, 0)) }
    o += ny * nz;
    for k in 0..nx * nz { out[o + k] = c(vel.face((k % nx) as i64, -1, (k / nx) as i64, 1)) }
    o += nx * nz;
    for k in 0..nx * ny { out[o + k] = c(vel.face((k % nx) as i64, (k / nx) as i64, -1, 2)) }
    out
}

/// Time spent in one GPU diffusion phase, ms.
#[derive(Copy, Clone, Debug, Default)]
pub struct Timings {
    /// Sides, velocities and packing on the CPU.
    pub prepare: f64,
    /// Writing the inputs to the GPU.
    pub upload: f64,
    /// The four dispatches, waited for.
    pub compute: f64,
    /// Copying the results back and mapping them.
    pub readback: f64,
    /// Writing the results into the world.
    pub unpack: f64,
}
impl Timings {
    pub fn total(&self) -> f64 { self.prepare + self.upload + self.compute + self.readback + self.unpack }
    pub fn add(&mut self, o: &Timings) { self.prepare += o.prepare; self.upload += o.upload; self.compute += o.compute; self.readback += o.readback; self.unpack += o.unpack }
}

/// The GPU diffusion phase for worlds of one size.
pub struct GpuDiffusion {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub adapter_info: wgpu::AdapterInfo,
    pipelines: [wgpu::ComputePipeline; 4],
    bind_groups: [wgpu::BindGroup; 4],
    params: wgpu::Buffer,
    cells: wgpu::Buffer,
    sides: wgpu::Buffer,
    vel: wgpu::Buffer,
    out: wgpu::Buffer,
    edge: wgpu::Buffer,
    read: wgpu::Buffer,
    n: usize,
    bricks: usize,
    nlower: usize,
    pub last: Timings,
}

const WG: u32 = 64;

fn dispatch(pass: &mut wgpu::ComputePass, threads: usize) {
    let groups = (threads as u32).div_ceil(WG);
    let (x, y) = if groups <= 65_535 { (groups, 1) } else { (65_535, groups.div_ceil(65_535)) };
    pass.dispatch_workgroups(x, y, 1);
}

impl GpuDiffusion {
    /// A device (a real GPU if there is one, else a software one) and the
    /// buffers for worlds the size of `w`.
    pub fn new(w: &World) -> Result<GpuDiffusion, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: None,
            ..Default::default()
        })).map_err(|e| format!("no GPU adapter: {e}"))?;
        let info = adapter.get_info();
        let mut limits = wgpu::Limits::default();
        let al = adapter.limits();
        limits.max_storage_buffer_binding_size = al.max_storage_buffer_binding_size;
        limits.max_buffer_size = al.max_buffer_size;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("fv-gpu"),
            required_limits: limits,
            ..Default::default()
        })).map_err(|e| format!("no GPU device: {e}"))?;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("diffuse"), source: wgpu::ShaderSource::Wgsl(shader_source().into()) });

        let n = w.cells.len();
        let nlower = w.ny * w.nz + w.nx * w.nz + w.nx * w.ny;
        let bricks = w.bricks();
        let buf = |label: &str, words: usize, usage: wgpu::BufferUsages| device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: (words * 4) as u64, usage, mapped_at_creation: false });
        use wgpu::BufferUsages as U;
        let params = buf("params", 16, U::UNIFORM | U::COPY_DST);
        let cells = buf("cells", (n + 1) * CELL_WORDS, U::STORAGE | U::COPY_DST);
        let sides = buf("sides", (n + 1) * SIDE_WORDS, U::STORAGE | U::COPY_DST);
        let vel = buf("vel", 3 * n + nlower, U::STORAGE | U::COPY_DST);
        let faces = buf("faces", (3 * n + nlower) * FLUX_WORDS, U::STORAGE);
        let lim = buf("limits", 2 * n, U::STORAGE);
        let out = buf("out", n * CELL_WORDS, U::STORAGE | U::COPY_SRC);
        let edge = buf("edge", (n + bricks) * EDGE_WORDS, U::STORAGE | U::COPY_SRC);
        let read = buf("read", n * CELL_WORDS + bricks * EDGE_WORDS, U::MAP_READ | U::COPY_DST);

        let all = [&params, &cells, &sides, &vel, &faces, &lim, &out, &edge];
        let names = ["faces_main", "limits_main", "brick_main", "tally_main"];
        // Which bindings each entry point uses (the default layout has only those).
        let uses: [&[u32]; 4] = [&[0, 1, 2, 3, 4], &[0, 1, 4, 5], &[0, 1, 2, 4, 5, 6, 7], &[0, 7]];
        let pipelines = names.map(|e| device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(e), layout: None, module: &module, entry_point: Some(e), compilation_options: Default::default(), cache: None,
        }));
        let bind_groups = std::array::from_fn(|k| {
            let entries: Vec<wgpu::BindGroupEntry> = uses[k].iter().map(|&b| wgpu::BindGroupEntry { binding: b, resource: all[b as usize].as_entire_binding() }).collect();
            device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some(names[k]), layout: &pipelines[k].get_bind_group_layout(0), entries: &entries })
        });
        Ok(GpuDiffusion { device, queue, adapter_info: info, pipelines, bind_groups, params, cells, sides, vel, out, edge, read, n, bricks, nlower, last: Timings::default() })
    }

    /// Upload, compute and read back one phase from packed inputs: each
    /// cell's 20 output words, then each brick's 16 tally words.
    pub fn run_packed(&mut self, inp: &Inputs) -> (Vec<u32>, Timings) {
        let mut t = Timings::default();
        let t0 = Instant::now();
        self.queue.write_buffer(&self.params, 0, bytemuck::cast_slice(&inp.params));
        self.queue.write_buffer(&self.cells, 0, bytemuck::cast_slice(&inp.cells));
        self.queue.write_buffer(&self.sides, 0, bytemuck::cast_slice(&inp.sides));
        self.queue.write_buffer(&self.vel, 0, bytemuck::cast_slice(&inp.vel));
        self.queue.submit([]);
        self.device.poll(wgpu::PollType::wait_indefinitely()).expect("GPU poll");
        t.upload = ms(t0);

        let t0 = Instant::now();
        let mut enc = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("diffuse"), timestamp_writes: None });
            let threads = [3 * self.n + self.nlower, self.n, self.n, self.bricks];
            for k in 0..4 {
                pass.set_pipeline(&self.pipelines[k]);
                pass.set_bind_group(0, &self.bind_groups[k], &[]);
                dispatch(&mut pass, threads[k]);
            }
        }
        self.queue.submit([enc.finish()]);
        self.device.poll(wgpu::PollType::wait_indefinitely()).expect("GPU poll");
        t.compute = ms(t0);

        let t0 = Instant::now();
        let cell_bytes = (self.n * CELL_WORDS * 4) as u64;
        let mut enc = self.device.create_command_encoder(&Default::default());
        enc.copy_buffer_to_buffer(&self.out, 0, &self.read, 0, cell_bytes);
        enc.copy_buffer_to_buffer(&self.edge, (self.n * EDGE_WORDS * 4) as u64, &self.read, cell_bytes, (self.bricks * EDGE_WORDS * 4) as u64);
        self.queue.submit([enc.finish()]);
        let slice = self.read.slice(..);
        slice.map_async(wgpu::MapMode::Read, |r| r.expect("map readback"));
        self.device.poll(wgpu::PollType::wait_indefinitely()).expect("GPU poll");
        let data: Vec<u32> = bytemuck::cast_slice(&slice.get_mapped_range().expect("mapped")).to_vec();
        self.read.unmap();
        t.readback = ms(t0);
        (data, t)
    }

    /// Each of the four kernels timed on its own (inputs already uploaded
    /// by [`GpuDiffusion::run_packed`]), ms: faces, limits, bricks, tallies.
    pub fn kernel_times(&mut self) -> [f64; 4] {
        let threads = [3 * self.n + self.nlower, self.n, self.n, self.bricks];
        std::array::from_fn(|k| {
            let t0 = Instant::now();
            let mut enc = self.device.create_command_encoder(&Default::default());
            {
                let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("one"), timestamp_writes: None });
                pass.set_pipeline(&self.pipelines[k]);
                pass.set_bind_group(0, &self.bind_groups[k], &[]);
                dispatch(&mut pass, threads[k]);
            }
            self.queue.submit([enc.finish()]);
            self.device.poll(wgpu::PollType::wait_indefinitely()).expect("GPU poll");
            ms(t0)
        })
    }

    /// The diffusion phase on the GPU: leaves `w` exactly as
    /// [`fv_core::transport::diffuse`] would.
    pub fn diffuse(&mut self, w: &mut World, vel: &dyn FaceVelocity) {
        assert_eq!(w.cells.len(), self.n, "GpuDiffusion was made for another world size");
        let t0 = Instant::now();
        let inp = inputs(w, vel);
        let prepare = ms(t0);
        let (data, mut t) = self.run_packed(&inp);
        t.prepare = prepare;
        let t0 = Instant::now();
        unpack(w, &data);
        t.unpack = ms(t0);
        self.last = t;
    }
}

fn ms(t0: Instant) -> f64 { t0.elapsed().as_secs_f64() * 1e3 }

fn get64(d: &[u32], o: usize) -> i64 { (d[o] as u64 | (d[o + 1] as u64) << 32) as i64 }

/// Writes a phase's results into the world as `World::run_phase` would:
/// the new cells become the state, and each brick's tally is added.
pub fn unpack(w: &mut World, data: &[u32]) {
    let n = w.cells.len();
    let mut next = core::mem::take(&mut w.next);
    for i in 0..n {
        let o = i * CELL_WORDS;
        let mut c = w.cells[i];
        for k in 0..GASES { c.gas[k].0 = get64(data, o + 2 * k) }
        c.energy.0 = get64(data, o + 14);
        c.solid_energy.0 = get64(data, o + 16);
        c.skin.0 = get64(data, o + 18);
        next[i] = c;
    }
    core::mem::swap(&mut w.cells, &mut next);
    w.next = next;
    let base = n * CELL_WORDS;
    for b in 0..w.bricks() {
        let o = base + b * EDGE_WORDS;
        let mut t = fv_core::world::Tally::default();
        t.energy_in = get64(data, o);
        for k in 0..GASES { t.gas_in[k] = get64(data, o + 2 + 2 * k) }
        w.tallies[b].add(&t);
    }
}
