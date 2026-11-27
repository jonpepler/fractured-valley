//! Host side of think v2 on the GPU (`think2.wgsl`). The bases (one per species) and the minds
//! are packed by `fv_mind_lab::think2` (`pack_base`, `pack_mind`) and held resident in two
//! buffers; each batch uploads only the thinks' inputs (modulators, params, state, injections)
//! and reads back their outputs. One dispatch per batch, every hop inside it, one workgroup per
//! think. Thinks are ordered by species base before dispatch, so workgroups running side by side
//! read the same base.

use fv_mind_lab::think::{Mods, Params};
use fv_mind_lab::think2::{self, Out};
use std::cell::RefCell;
use wgpu::util::DeviceExt;

pub struct Gpu { pub device: wgpu::Device, pub queue: wgpu::Queue, pub info: wgpu::AdapterInfo, pipeline: wgpu::ComputePipeline, bufs: RefCell<Option<Bufs>>, pub workgroups: u32 }

/// One think to run: where its mind and base are in the resident buffers, and its inputs.
#[derive(Clone, Debug)]
pub struct Job { pub mind: u32, pub base: u32, pub n: u32, pub mods: Mods, pub params: Params, pub state: Vec<(u32, i32)>, pub inject: Vec<(u32, i32)> }

/// Resident graphs: every base and every mind, packed and uploaded once (or when they change).
pub struct Resident { pub bases: wgpu::Buffer, pub minds: wgpu::Buffer, pub base_words: usize, pub mind_words: usize }

/// What one batch cost: bytes up and down, seconds uploading, computing and reading back.
#[derive(Clone, Copy, Debug, Default)]
pub struct Cost { pub dispatches: u64, pub bytes_up: u64, pub bytes_down: u64, pub up_s: f64, pub run_s: f64, pub down_s: f64 }
impl std::ops::AddAssign for Cost { fn add_assign(&mut self, o: Cost) { self.dispatches += o.dispatches; self.bytes_up += o.bytes_up; self.bytes_down += o.bytes_down; self.up_s += o.up_s; self.run_s += o.run_s; self.down_s += o.down_s; } }

struct Buf { b: wgpu::Buffer, words: usize }
/// `stride`: words of node arrays per scratch slot. Fixed for the life of the scratch buffer, so a
/// slot's activation, sums and marks always sit at the same place (the shader clears those three
/// after each think; its lists are always written before they are read).
struct Bufs { inp: Buf, idx: Buf, out: Buf, read: Buf, scr: Buf, uni: wgpu::Buffer, stride: usize }

impl Gpu {
    /// The adapter `WGPU_BACKEND` (and, for Vulkan, `VK_ICD_FILENAMES`) points at, else the default.
    pub fn new() -> Result<Gpu, String> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor { backends: wgpu::Backends::from_env().unwrap_or_default(), ..Default::default() });
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions { power_preference: wgpu::PowerPreference::HighPerformance, ..Default::default() })).map_err(|e| format!("no GPU adapter: {e}"))?;
        let info = adapter.get_info();
        let limits = adapter.limits();
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor { label: None, required_features: wgpu::Features::empty(), required_limits: limits, ..Default::default() })).map_err(|e| format!("no device: {e}"))?;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("think2"), source: wgpu::ShaderSource::Wgsl(include_str!("think2.wgsl").into()) });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor { label: None, layout: None, module: &module, entry_point: Some("main"), compilation_options: Default::default(), cache: None });
        let workgroups = std::env::var("FV_GPU_WORKGROUPS").ok().and_then(|x| x.parse().ok()).unwrap_or(512);
        Ok(Gpu { device, queue, info, pipeline, bufs: RefCell::new(None), workgroups })
    }

    pub fn resident(&self, bases: &[u32], minds: &[u32]) -> Resident {
        let s = wgpu::BufferUsages::STORAGE;
        let mk = |d: &[u32]| self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: bytemuck::cast_slice(if d.is_empty() { &[0u32] } else { d }), usage: s });
        Resident { bases: mk(bases), minds: mk(minds), base_words: bases.len(), mind_words: minds.len() }
    }

    /// Returns the scratch stride (nodes per slot) to use.
    fn ensure(&self, inp: usize, idx: usize, out: usize, nwg: usize, nmax: usize) -> usize {
        let mut g = self.bufs.borrow_mut();
        let d = &self.device;
        let s = wgpu::BufferUsages::STORAGE;
        let mk = |words: usize, usage: wgpu::BufferUsages| Buf { b: d.create_buffer(&wgpu::BufferDescriptor { label: None, size: (words.max(1) * 4) as u64, usage, mapped_at_creation: false }), words: words.max(1) };
        let grow = |w: usize| w.next_power_of_two().max(1024);
        if g.is_none() {
            *g = Some(Bufs { inp: mk(grow(inp), s | wgpu::BufferUsages::COPY_DST), idx: mk(grow(idx), s | wgpu::BufferUsages::COPY_DST), out: mk(grow(out), s | wgpu::BufferUsages::COPY_SRC),
                read: mk(grow(out), wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST), scr: mk(1, s), uni: d.create_buffer(&wgpu::BufferDescriptor { label: None, size: 16, usage: s | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false }), stride: 0 });
        }
        let b = g.as_mut().unwrap();
        if b.inp.words < inp { b.inp = mk(grow(inp), s | wgpu::BufferUsages::COPY_DST); }
        if b.idx.words < idx { b.idx = mk(grow(idx), s | wgpu::BufferUsages::COPY_DST); }
        if b.out.words < out { b.out = mk(grow(out), s | wgpu::BufferUsages::COPY_SRC); b.read = mk(grow(out), wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST); }
        // (Review: a stride that followed each batch's largest mind put a later batch's
        // activations over an earlier batch's lists. The stride now only grows, and a new stride
        // comes with a new, zeroed buffer.)
        if nmax > b.stride || nwg * 9 * b.stride > b.scr.words {
            b.stride = b.stride.max(nmax.next_power_of_two().max(256));
            b.scr = mk(self.workgroups as usize * 9 * b.stride, s);
        }
        b.stride
    }

    /// Run `jobs` in one dispatch. Returns each think's output and state, in the jobs' order.
    pub fn think(&self, res: &Resident, jobs: &[Job]) -> (Vec<(Out, Vec<(u32, i32)>)>, Cost) {
        if jobs.is_empty() { return (vec![], Cost::default()); }
        let t0 = std::time::Instant::now();
        // Batching by species base: dispatch order sorted by base (stable), results put back.
        let mut order: Vec<usize> = (0..jobs.len()).collect();
        order.sort_by_key(|&i| jobs[i].base);
        let mut inp = vec![];
        let mut idx = Vec::with_capacity(jobs.len());
        let mut outs = Vec::with_capacity(jobs.len());
        let mut out_words = 0usize;
        let mut nmax = 0u32;
        for &i in &order {
            let j = &jobs[i];
            let p = &j.params;
            assert!(p.frontier <= 64, "the GPU's frontier holds at most 64 senders");
            assert!(p.min_act >= 1 && p.min_act <= p.max_act && p.max_act <= think2::A_CAP && (0..=1000).contains(&p.persist) && (0..=1000).contains(&p.carry_spread)
                && (0..=1000).contains(&p.hop_decay) && (0..=131_071).contains(&p.budget), "params outside v2's ranges (SPEC-think.md §2)");
            idx.push(inp.len() as u32);
            think2::pack_input(&mut inp, j.mind, j.base, out_words as u32, &j.mods, &j.params, &j.state, &j.inject);
            outs.push(out_words);
            out_words += think2::out_words(&j.params);
            nmax = nmax.max(j.n);
        }
        let nwg = (jobs.len() as u32).min(self.workgroups);
        let stride = self.ensure(inp.len(), idx.len(), out_words, nwg as usize, nmax as usize);
        let scr_words = nwg as usize * 9 * stride;
        let g = self.bufs.borrow();
        let b = g.as_ref().unwrap();
        let q = &self.queue;
        q.write_buffer(&b.inp.b, 0, bytemuck::cast_slice(&inp));
        q.write_buffer(&b.idx.b, 0, bytemuck::cast_slice(&idx));
        q.write_buffer(&b.uni, 0, bytemuck::cast_slice(&[jobs.len() as u32, nwg, stride as u32, 0]));
        let up_s = t0.elapsed().as_secs_f64();
        let t1 = std::time::Instant::now();
        let layout = self.pipeline.get_bind_group_layout(0);
        let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &layout, entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: res.bases.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: res.minds.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 2, resource: b.inp.b.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 3, resource: b.idx.b.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 4, resource: b.out.b.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &b.scr.b, offset: 0, size: std::num::NonZeroU64::new((scr_words.max(1) * 4) as u64) }) },
            wgpu::BindGroupEntry { binding: 6, resource: b.uni.as_entire_binding() },
        ] });
        let mut enc = self.device.create_command_encoder(&Default::default());
        { let mut pass = enc.begin_compute_pass(&Default::default()); pass.set_pipeline(&self.pipeline); pass.set_bind_group(0, &bg, &[]); pass.dispatch_workgroups(nwg, 1, 1); }
        enc.copy_buffer_to_buffer(&b.out.b, 0, &b.read.b, 0, (out_words * 4) as u64);
        q.submit([enc.finish()]);
        let sl = b.read.b.slice(..(out_words * 4) as u64);
        sl.map_async(wgpu::MapMode::Read, |_| {});
        self.device.poll(wgpu::PollType::Wait).unwrap();
        let run_s = t1.elapsed().as_secs_f64();
        let t2 = std::time::Instant::now();
        let words: Vec<u32> = bytemuck::cast_slice(&sl.get_mapped_range()).to_vec();
        b.read.b.unmap();
        let mut res_v: Vec<Option<(Out, Vec<(u32, i32)>)>> = vec![None; jobs.len()];
        for (k, &i) in order.iter().enumerate() { res_v[i] = Some(think2::unpack_output(&words[outs[k]..])); }
        let down_s = t2.elapsed().as_secs_f64();
        let cost = Cost { dispatches: 1, bytes_up: (inp.len() + idx.len() + 4) as u64 * 4, bytes_down: out_words as u64 * 4, up_s, run_s, down_s };
        (res_v.into_iter().map(|x| x.unwrap()).collect(), cost)
    }
}

/// Replays recorded thinks (`think2::record_start`) on the GPU and compares every one with the
/// CPU's result. Snapshots of minds are uploaded with the batch that first needs them.
pub struct Replay<'g> {
    pub gpu: &'g Gpu,
    bases: Vec<u32>, base_at: std::collections::HashMap<u64, u32>,
    snaps: std::collections::HashMap<u64, std::sync::Arc<Vec<u32>>>, latest: std::collections::HashMap<u64, u64>,
    pub thinks: u64, pub mismatches: u64, pub first_mismatch: Option<String>,
    pub cost: Cost, pub snap_bytes: u64, pub flags_seen: u32,
}
impl<'g> Replay<'g> {
    pub fn new(gpu: &'g Gpu) -> Self { Replay { gpu, bases: vec![], base_at: Default::default(), snaps: Default::default(), latest: Default::default(), thinks: 0, mismatches: 0, first_mismatch: None, cost: Cost::default(), snap_bytes: 0, flags_seen: 0 } }
    pub fn batch(&mut self, b: think2::Batch) {
        for (id, w) in b.bases { self.base_at.insert(id, self.bases.len() as u32); self.bases.extend_from_slice(&w); }
        for s in b.snaps { self.snap_bytes += s.words.len() as u64 * 4; self.latest.insert(s.uid, s.id); self.snaps.insert(s.id, s.words); }
        // The minds this batch needs, packed into one buffer for it.
        let mut minds = vec![];
        let mut at = std::collections::HashMap::new();
        for r in &b.recs { at.entry(r.snap).or_insert_with(|| { let o = minds.len() as u32; minds.extend_from_slice(&self.snaps[&r.snap]); o }); }
        let res = self.gpu.resident(&self.bases, &minds);
        let jobs: Vec<Job> = b.recs.iter().map(|r| Job { mind: at[&r.snap], base: self.base_at[&r.base], n: r.n as u32, mods: r.mods, params: r.params, state: r.state_in.clone(), inject: r.inject.clone() }).collect();
        let (outs, cost) = self.gpu.think(&res, &jobs);
        self.cost += cost;
        self.cost.bytes_up += (minds.len() * 4) as u64;
        for (r, (o, s)) in b.recs.iter().zip(outs) {
            self.thinks += 1;
            self.flags_seen |= r.out.flags;
            if o != r.out || s != r.state_out {
                self.mismatches += 1;
                if self.first_mismatch.is_none() { self.first_mismatch = Some(format!("think {}: cpu {:?} state {:?}\n gpu {:?} state {:?}", self.thinks, r.out, &r.state_out[..r.state_out.len().min(12)], o, &s[..s.len().min(12)])); }
            }
        }
        // Keep only the latest snapshot of each mind (older ones are never referenced again).
        let keep: std::collections::HashSet<u64> = self.latest.values().copied().collect();
        self.snaps.retain(|id, _| keep.contains(id));
    }
}
