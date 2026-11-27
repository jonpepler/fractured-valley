//! How many 32-bit integer operations a second this adapter does: a kernel
//! with a known count (independent multiply-adds), to turn kernel times into
//! operations per face.
use std::time::Instant;
fn main() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default())).unwrap();
    for (name, body) in [("multiply-add", "a = a * 1664525u + 1013904223u; b = b * 22695477u + 1u; c = c * 1103515245u + 12345u; d = d * 134775813u + 1u;"), ("add/xor/shift", "a = (a ^ (a << 5u)) + b; b = (b ^ (b >> 3u)) + c; c = (c ^ (c << 7u)) + d; d = (d ^ (d >> 11u)) + a;")] {
    let iters = 4096u32;
    let src = format!("@group(0) @binding(0) var<storage, read_write> o: array<u32>;
@compute @workgroup_size(64) fn main(@builtin(global_invocation_id) id: vec3<u32>) {{
  var a = id.x; var b = id.x ^ 0x9e37u; var c = id.x + 7u; var d = id.x * 3u;
  for (var i = 0u; i < {iters}u; i++) {{ {body} }}
  o[id.x] = a ^ b ^ c ^ d; }}");
    let m = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: None, source: wgpu::ShaderSource::Wgsl(src.into()) });
    let p = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor { label: None, layout: None, module: &m, entry_point: Some("main"), compilation_options: Default::default(), cache: None });
    let threads = 1u32 << 18;
    let buf = device.create_buffer(&wgpu::BufferDescriptor { label: None, size: threads as u64 * 4, usage: wgpu::BufferUsages::STORAGE, mapped_at_creation: false });
    let bg = device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &p.get_bind_group_layout(0), entries: &[wgpu::BindGroupEntry { binding: 0, resource: buf.as_entire_binding() }] });
    let mut best = f64::MAX;
    for _ in 0..5 {
        let t0 = Instant::now();
        let mut e = device.create_command_encoder(&Default::default());
        { let mut pass = e.begin_compute_pass(&Default::default()); pass.set_pipeline(&p); pass.set_bind_group(0, &bg, &[]); pass.dispatch_workgroups(threads / 64, 1, 1); }
        queue.submit([e.finish()]);
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        best = best.min(t0.elapsed().as_secs_f64());
    }
    let ops = threads as f64 * iters as f64 * if name == "multiply-add" { 8.0 } else { 12.0 };
    println!("{} {name}: {:.1} G u32 ops/s ({:.1} ms for {:.2e} ops)", adapter.get_info().name, ops / best / 1e9, best * 1e3, ops);
}
}
