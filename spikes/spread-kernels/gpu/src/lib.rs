//! Host side of the GPU kernel (`redef.wgsl`): packs the shared graph and the deltas into flat
//! u32 buffers, uploads one tick of inputs at a time, and dispatches one invocation per
//! instance, in chunks so the scratch buffer stays small.

use sk::{bench::Set, redef};
use mc::spread::Params;
use wgpu::util::DeviceExt;

pub const ST: usize = 513;
pub const IN: usize = 146;
pub const OUT: usize = 10;
pub const SCR: usize = 9216;
pub const MAX_INJECT: usize = 24;

pub fn pack_shared(sh: &sk::shared::Shared) -> Vec<u32> {
    let n = sh.n;
    let mut v = vec![0u32; 5];
    v[0] = n as u32;
    v[1] = v.len() as u32; v.extend_from_slice(&sh.off);
    v[2] = v.len() as u32; for e in &sh.edges { v.push(e.to); v.push(e.w as u16 as u32 | (e.key as u32) << 16); }
    v[3] = v.len() as u32; v.extend(sh.den.iter().map(|&d| d as u32));
    v[4] = v.len() as u32; v.extend_from_slice(&sh.cmask);
    v
}

pub fn pack_deltas(ds: &[sk::delta::Delta]) -> Vec<u32> {
    let mut v = vec![0u32; ds.len() * 8];
    for (i, d) in ds.iter().enumerate() {
        let h = i * 8;
        v[h] = d.base_n;
        v[h + 1] = v.len() as u32; for r in &d.rank { v.extend_from_slice(r); }
        v[h + 2] = v.len() as u32; for (j, &dn) in d.den.iter().enumerate() { v.push(dn as u32); v.push(d.pmask[j]); }
        v[h + 3] = v.len() as u32; v.extend_from_slice(&d.ostart);
        v[h + 4] = v.len() as u32; v.extend_from_slice(&d.xstart);
        v[h + 5] = v.len() as u32; for o in &d.ovr { v.push(o.0 as u32 | (o.1 as u16 as u32) << 16); v.push(o.2 as u32); }
        v[h + 6] = v.len() as u32; for e in &d.extra { v.push(e.to); v.push(e.w as u16 as u32 | (e.key as u32) << 16); }
    }
    v
}

/// One tick of inputs for every instance: knots, damp, injections.
pub fn pack_inputs(set: &Set, p: &Params, tick: usize, classes: u32, out: &mut Vec<u32>) {
    out.clear();
    for tk in set.ticks.iter().map(|t| &t[tick]) {
        let k = redef::knots(&tk.mods, p.hop_decay, classes);
        for c in 0..32 { for j in 0..3 { out.push(k.f[c][j] as u32); } }
        out.push(tk.damp as u32);
        assert!(tk.inject.len() <= MAX_INJECT);
        out.push(tk.inject.len() as u32);
        for j in 0..MAX_INJECT { let (n, a) = tk.inject.get(j).copied().unwrap_or((0, 0)); out.push(n); out.push(a as u32); }
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uni { count: u32, first: u32, hops: u32, frontier: u32, persist: i32, carry: i32, min_act: i32, max_act: i32, budget: i32, active_cap: u32, ws_k: u32, hop_decay: i32, gen: u32, tick: u32, ticks: u32, _pad: u32 }

pub struct Gpu { pub device: wgpu::Device, pub queue: wgpu::Queue, pub info: wgpu::AdapterInfo, pipeline: wgpu::ComputePipeline }

impl Gpu {
    pub fn new() -> Option<Gpu> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor { backends: wgpu::Backends::from_env().unwrap_or_default(), ..Default::default() });
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions { power_preference: wgpu::PowerPreference::HighPerformance, ..Default::default() })).ok()?;
        let info = adapter.get_info();
        let mut limits = adapter.limits();
        limits.max_storage_buffers_per_shader_stage = limits.max_storage_buffers_per_shader_stage.max(6);
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor { label: None, required_features: wgpu::Features::empty(), required_limits: limits, ..Default::default() })).ok()?;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("redef"), source: wgpu::ShaderSource::Wgsl(include_str!("redef.wgsl").into()) });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor { label: None, layout: None, module: &module, entry_point: Some("main"), compilation_options: Default::default(), cache: None });
        Some(Gpu { device, queue, info, pipeline })
    }
}

pub struct Run { pub outputs: Vec<Vec<Vec<u32>>>, pub states: Vec<Vec<(u32, i32)>>, pub overflows: u64, pub gpu_secs: f64, pub pack_secs: f64 }

/// Run all ticks of `set` on the GPU. `chunk` instances per dispatch.
pub fn run(g: &Gpu, set: &Set, p: &Params, chunk: usize, keep_outputs: bool) -> Run {
    let m = set.deltas.len();
    let nt = set.ticks[0].len();
    let classes = redef::classes(&set.sh, &set.deltas);
    let d = &g.device;
    let lim = d.limits().max_storage_buffer_binding_size as usize;
    let buf = |data: &[u32], usage: wgpu::BufferUsages| d.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: None, contents: bytemuck::cast_slice(data), usage });
    let s = wgpu::BufferUsages::STORAGE;
    let shared = pack_shared(&set.sh);
    assert!(shared.len() * 4 <= lim, "shared graph exceeds the binding limit");
    let shb = buf(&shared, s);
    // Chunks: at most `chunk` instances, and instance data within the binding limit.
    let mut chunks: Vec<(usize, usize)> = vec![];
    let mut first = 0;
    while first < m {
        let mut end = first; let mut bytes = 0;
        while end < m && end - first < chunk {
            let b = pack_deltas(&set.deltas[end..end + 1]).len() * 4;
            if end > first && bytes + b > lim { break; }
            bytes += b; end += 1;
        }
        chunks.push((first, end)); first = end;
    }
    let maxc = chunks.iter().map(|c| c.1 - c.0).max().unwrap();
    let scb = d.create_buffer(&wgpu::BufferDescriptor { label: None, size: (maxc * SCR * 4) as u64, usage: s, mapped_at_creation: false });
    let ub = d.create_buffer(&wgpu::BufferDescriptor { label: None, size: 64, usage: s | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
    let rb = d.create_buffer(&wgpu::BufferDescriptor { label: None, size: (maxc * ST * 4) as u64, usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
    let layout = g.pipeline.get_bind_group_layout(0);
    struct C { ib: wgpu::Buffer, st: wgpu::Buffer, inp: wgpu::Buffer, out: wgpu::Buffer, bg: wgpu::BindGroup }
    let cs: Vec<C> = chunks.iter().map(|&(a, b)| {
        let n = b - a;
        let ib = buf(&pack_deltas(&set.deltas[a..b]), s);
        let st = buf(&vec![0u32; n * ST], s | wgpu::BufferUsages::COPY_SRC);
        let inp = d.create_buffer(&wgpu::BufferDescriptor { label: None, size: (n * IN * 4) as u64, usage: s | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let out = d.create_buffer(&wgpu::BufferDescriptor { label: None, size: (n * OUT * 4) as u64, usage: s | wgpu::BufferUsages::COPY_SRC, mapped_at_creation: false });
        let bg = d.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &layout, entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: shb.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: ib.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 2, resource: st.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 3, resource: inp.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 4, resource: out.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 5, resource: scb.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 6, resource: ub.as_entire_binding() },
        ] });
        C { ib, st, inp, out, bg }
    }).collect();
    let read = |src: &wgpu::Buffer, words: usize| -> Vec<u32> {
        let mut enc = d.create_command_encoder(&Default::default());
        enc.copy_buffer_to_buffer(src, 0, &rb, 0, (words * 4) as u64);
        g.queue.submit([enc.finish()]);
        let sl = rb.slice(..(words * 4) as u64);
        sl.map_async(wgpu::MapMode::Read, |_| {});
        d.poll(wgpu::PollType::Wait).unwrap();
        let v = bytemuck::cast_slice(&sl.get_mapped_range()).to_vec();
        rb.unmap();
        v
    };
    let mut inputs = vec![];
    let mut outputs = vec![vec![]; m];
    let (mut gpu_secs, mut pack_secs) = (0.0, 0.0);
    let mut overflows = 0;
    let mut gen = 0u32;
    for t in 0..nt {
        let t0 = std::time::Instant::now();
        pack_inputs(set, p, t, classes, &mut inputs);
        pack_secs += t0.elapsed().as_secs_f64();
        let t0 = std::time::Instant::now();
        for (ci, &(a, b)) in chunks.iter().enumerate() {
            let c = &cs[ci];
            g.queue.write_buffer(&c.inp, 0, bytemuck::cast_slice(&inputs[a * IN..b * IN]));
            gen += 1;
            let u = Uni { count: (b - a) as u32, first: 0, hops: p.hops as u32, frontier: p.frontier as u32, persist: p.persist, carry: p.carry_spread, min_act: p.min_act, max_act: p.max_act, budget: p.budget, active_cap: p.active_cap as u32, ws_k: p.ws_k as u32, hop_decay: p.hop_decay, gen, tick: t as u32, ticks: nt as u32, _pad: 0 };
            g.queue.write_buffer(&ub, 0, bytemuck::bytes_of(&u));
            let mut enc = d.create_command_encoder(&Default::default());
            { let mut pass = enc.begin_compute_pass(&Default::default()); pass.set_pipeline(&g.pipeline); pass.set_bind_group(0, &c.bg, &[]); pass.dispatch_workgroups((b - a).div_ceil(64) as u32, 1, 1); }
            g.queue.submit([enc.finish()]);
            if keep_outputs {
                let o = read(&c.out, (b - a) * OUT);
                for i in 0..b - a { let r = &o[i * OUT..(i + 1) * OUT]; if r[9] != 0 { overflows += 1; } outputs[a + i].push(r[1..1 + r[0] as usize].to_vec()); }
            }
        }
        d.poll(wgpu::PollType::Wait).unwrap();
        gpu_secs += t0.elapsed().as_secs_f64();
    }
    let mut states = vec![];
    for (ci, &(a, b)) in chunks.iter().enumerate() {
        let sv = read(&cs[ci].st, (b - a) * ST);
        for i in 0..b - a { let r = &sv[i * ST..]; states.push((0..r[0] as usize).map(|k| (r[1 + 2 * k], r[2 + 2 * k] as i32)).collect()); }
    }
    let _ = cs.iter().map(|c| &c.ib).count();
    Run { outputs, states, overflows, gpu_secs, pack_secs }
}

/// CPU reference for the same set: per-tick outputs and final states.
pub fn cpu(set: &Set, p: &Params) -> (Vec<Vec<Vec<u32>>>, Vec<Vec<(u32, i32)>>) {
    let classes = redef::classes(&set.sh, &set.deltas);
    let mut sc = redef::Scratch::new();
    let mut outs = vec![]; let mut sts = vec![];
    for (who, d) in set.deltas.iter().enumerate() {
        let mut inst = sk::exact::Inst::new();
        let mut o = vec![];
        for tk in &set.ticks[who] {
            if tk.damp != 1000 { redef::damp(&mut inst, tk.damp); }
            o.push(redef::step(&set.sh, d, &mut inst, &tk.mods, &tk.inject, p, classes, &mut sc).0);
        }
        outs.push(o); sts.push(inst.st.clone());
    }
    (outs, sts)
}
