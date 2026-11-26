//! Build a scene, run the simulation, render frames and pipe them to ffmpeg.
//!
//!   VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json \
//!   cargo run --release -p fv-render --example record -- --scene night-crib --size 64 --ticks 600
//!
//! Options (all optional):
//!   --scene day-crib | night-crib   afternoon sun or no sun at all; the crib is lit either way
//!   --size N        world is N×N cells (10 cm each), default 64
//!   --height N      world height in cells, default 64
//!   --seed N        terrain seed, default 7
//!   --ticks N       ticks to simulate (50 ms each), default 600
//!   --step N        render every Nth tick, default 4
//!   --scale N       screen pixels per sprite pixel, default 3
//!   --stills N      PNG stills spread over the run, default 4
//!   --fps N         video frame rate, default 20
//!   --torch-j N     joules the torch puts into each lit cell per tick, default 250
//!   --torch-ticks N how many ticks the torch is held, default 80
//!   --focus F       world (frame the whole box, default) or crib (close up on the fire)
//!   --out DIR       output directory
//! Writes DIR/<scene>.mp4 and DIR/<scene>-tNNNN.png (with -crib after the
//! scene name for the close-up).

use fv_core::rad::Sky;
use fv_core::scene::valley;
use fv_core::cell::Cell;
use fv_core::sim::{Command, Scheduled, Sim};
use fv_core::substance::*;
use fv_core::units::joules;
use fv_core::view::Viewer;
use fv_render::{Camera, Renderer};
use std::io::Write;
use std::process::{Command as Process, Stdio};
use std::time::Instant;

const DEFAULT_OUT: &str = "/tmp/claude-0/-home-user/5649d0d7-d4b1-5036-842d-57dfada3ede9/scratchpad/render";

struct Args {
    scene: String,
    sky: String,
    size: usize,
    height: usize,
    seed: u64,
    ticks: u64,
    step: u64,
    scale: u32,
    stills: u64,
    fps: u32,
    torch_j: i64,
    torch_ticks: u64,
    focus: String,
    out: String,
}

fn args() -> Args {
    let mut a = Args { scene: "tree".into(), sky: "day".into(), size: 64, height: 64, seed: 7, ticks: 600, step: 4, scale: 3, stills: 4, fps: 20, torch_j: 250, torch_ticks: 80, focus: "close".into(), out: DEFAULT_OUT.into() };
    let raw: Vec<String> = std::env::args().skip(1).collect();
    for pair in raw.chunks(2) {
        let (key, value) = (pair[0].as_str(), pair.get(1).map(String::as_str).unwrap_or(""));
        let num = || value.parse::<u64>().unwrap_or_else(|_| panic!("{key} needs a number, got {value:?}"));
        match key {
            "--scene" => a.scene = value.into(),
            "--sky" => a.sky = value.into(),
            "--size" => a.size = num() as usize,
            "--height" => a.height = num() as usize,
            "--seed" => a.seed = num(),
            "--ticks" => a.ticks = num(),
            "--step" => a.step = num().max(1),
            "--scale" => a.scale = num().max(1) as u32,
            "--stills" => a.stills = num(),
            "--fps" => a.fps = num() as u32,
            "--torch-j" => a.torch_j = num() as i64,
            "--torch-ticks" => a.torch_ticks = num(),
            "--focus" => a.focus = value.into(),
            "--out" => a.out = value.into(),
            _ => panic!("unknown option {key}"),
        }
    }
    a
}

/// A scene: the world, where the torch goes, the camera's centre (cells)
/// and native image size for a close framing, and an optional stand-in for
/// slow burning (applied before each tick; its matter and energy are booked
/// as leaving).
struct Scene {
    world: fv_core::world::World,
    torch: Vec<(usize, usize, usize)>,
    centre: [i32; 3],
    frame: (usize, usize),
    char_post: Option<(usize, usize, usize)>,
}

fn build_scene(a: &Args) -> Scene {
    let ground_of = |w: &fv_core::world::World, x: usize, y: usize| (0..w.nz).rev().find(|&z| w.get(x, y, z).substance != 0).unwrap_or(0);
    match a.scene.as_str() {
        "tree" | "dead-tree" => {
            let (w, torch) = fv_core::scene::crib_and_tree(a.seed, a.size, a.height, a.scene == "dead-tree");
            let torch = torch.iter().enumerate().filter(|(i, _)| i % 3 == 0).map(|(_, p)| (p[0], p[1], p[2])).collect();
            let (n, h) = (a.size as i32, a.height as i32);
            Scene { world: w, torch, centre: [n / 2, n / 2, h / 3], frame: (2 * (a.size * 2) + 16, a.size * 2 + a.height + 24), char_post: None }
        }
        "campfire" => {
            let mut w = fv_core::world::World::new(a.size, a.size, a.height, a.seed, fv_core::units::celsius(15));
            let subs = w.subs.clone();
            let t = fv_core::units::celsius(15);
            let stone = Cell::solid_of(STONE, 1000, 2, t, &subs);
            let soil = Cell::solid_of(SOIL, 1000, 150, t, &subs);
            let grass = Cell::solid_of(GRASS, 1000, 900, t, &subs);
            let wood = Cell::solid_of(WOOD, 1000, 100, t, &subs);
            let kindling = Cell::solid_of(KINDLING, 1000, 80, t, &subs);
            let n = a.size;
            for y in 0..n { for x in 0..n { for z in 0..4 { w.set(x, y, z, if z < 2 { stone } else { soil }) } w.set(x, y, 4, grass) } }
            let (cx, cy) = (n / 2 - 2, n / 2 - 2);
            for y in cy..cy + 4 { for x in cx..cx + 4 { w.set(x, y, 4, kindling); w.set(x, y, 5, if (x + y) % 2 == 0 { kindling } else { wood }) } }
            for x in cx..cx + 4 { w.set(x, cy, 6, wood); w.set(x, cy + 3, 6, wood) }
            // A stone wall behind the fire, to catch its light and warmth.
            for z in 5..14 { for x in cx - 6..cx + 10 { w.set(x, cy - 4, z, stone) } }
            fv_core::scene::bond_all(&mut w, fv_core::scene::natural_bonds);
            let torch = vec![(cx, cy, 4), (cx + 2, cy + 1, 4), (cx + 1, cy + 3, 4), (cx + 3, cy + 2, 4)];
            Scene { world: w, torch, centre: [(cx + 2) as i32, (cy + 2) as i32, 6], frame: (200, 150), char_post: None }
        }
        "post" => {
            let mut w = fv_core::world::World::new(a.size, a.size, a.height, a.seed, fv_core::units::celsius(18));
            let subs = w.subs.clone();
            let t = fv_core::units::celsius(18);
            let stone = Cell::solid_of(STONE, 1000, 2, t, &subs);
            let grass = Cell::solid_of(GRASS, 1000, 900, t, &subs);
            let wood = Cell::solid_of(WOOD, 1000, 120, t, &subs);
            let iron = Cell::solid_of(IRON, 1000, 0, t, &subs);
            let n = a.size;
            let c = n / 2 - 1;
            for z in 0..4 { for y in 0..n { for x in 0..n { w.set(x, y, z, stone) } } }
            for y in 0..n { for x in 0..n { w.set(x, y, 4, grass) } }
            for z in 4..14 { for y in c..c + 2 { for x in c..c + 2 { w.set(x, y, z, wood) } } }
            for z in 14..17 { for y in c - 1..c + 2 { for x in c..c + 3 { w.set(x, y, z, iron) } } }
            fv_core::scene::bond_all(&mut w, fv_core::scene::natural_bonds);
            let _ = ground_of;
            Scene { world: w, torch: vec![], centre: [c as i32, c as i32, 8], frame: (200, 170), char_post: Some((c, c, 4)) }
        }
        _ => {
            let v = valley(a.seed, a.size, a.height, 120);
            let (cx, cy) = v.crib;
            let torch: Vec<(usize, usize, usize)> = [(2, 2), (6, 3), (3, 7), (8, 8)].iter()
                .filter(|&&(dx, dy)| cx + dx < a.size && cy + dy < a.size)
                .map(|&(dx, dy)| (cx + dx, cy + dy, v.ground_at(cx + dx, cy + dy) + 1))
                .filter(|&(x, y, z)| v.world.get(x, y, z).substance == GRASS)
                .collect();
            let (nx, ny, nz) = (v.world.nx, v.world.ny, v.world.nz);
            Scene { world: v.world, torch, centre: [nx as i32 / 2, ny as i32 / 2, nz as i32 / 4], frame: (2 * (nx + ny) + 16, nx + ny + nz + 24), char_post: None }
        }
    }
}

fn main() {
    let a = args();
    let sky = match a.sky.as_str() { "night" => Sky::NIGHT, _ => Sky::afternoon() };
    std::fs::create_dir_all(&a.out).expect("make output directory");
    let scene = build_scene(&a);
    let base = scene.world.cells.len();
    let mut sim = Sim::new(scene.world, sky);
    let mut seq = 0;
    for tick in 0..a.torch_ticks {
        for &(x, y, z) in &scene.torch {
            sim.schedule(Scheduled { tick, peer: 0, seq, command: Command::Heat { x: x as u16, y: y as u16, z: z as u16, energy: joules(a.torch_j) } });
            seq += 1;
        }
    }
    let (native_w, native_h, centre) = match a.focus.as_str() {
        "close" => (scene.frame.0, scene.frame.1, scene.centre),
        _ => { let (nx, ny, nz) = (sim.world.nx, sim.world.ny, sim.world.nz); (2 * (nx + ny) + 16, nx + ny + nz + 24, [nx as i32 / 2, ny as i32 / 2, nz as i32 / 4]) }
    };
    let width = (native_w as u32 * a.scale).next_multiple_of(2);
    let height = (native_h as u32 * a.scale).next_multiple_of(2);
    let camera = Camera { centre, scale: a.scale };
    let tag = format!("{}-{}-{}", a.scene, a.sky, a.focus);
    let mut renderer = Renderer::new_headless(width, height).expect("GPU");
    // Tell the camera how bright the sky is, so it knows day from night.
    renderer.exposure.sky_w_m2 = Some((sim.sky.sun_w_m2 + sim.sky.sky_w_m2) as f32);
    let info = renderer.device().adapter_info();
    println!("{} cells, image {width}×{height}, GPU: {} ({:?})", base, info.name, info.backend);

    let video = format!("{}/{tag}.mp4", a.out);
    let mut ffmpeg = Process::new("ffmpeg")
        .args(["-y", "-loglevel", "error", "-f", "rawvideo", "-pix_fmt", "rgba", "-s", &format!("{width}x{height}"), "-r", &a.fps.to_string(), "-i", "-",
               "-c:v", "libx264", "-pix_fmt", "yuv420p", "-crf", "16", "-preset", "veryfast", &video])
        .stdin(Stdio::piped())
        .spawn()
        .expect("start ffmpeg");
    let pipe = ffmpeg.stdin.as_mut().unwrap();

    let still_every = if a.stills > 0 { (a.ticks / a.stills).max(1) } else { u64::MAX };
    let mut viewer = Viewer::new();
    let (mut sim_time, mut render_time, mut frames) = (0.0, 0.0, 0);
    for tick in 0..=a.ticks {
        let t = Instant::now();
        // Stand-in for half an hour of fire at the post's foot (one side
        // first), shown in seconds: the burnt matter and its energy are
        // booked as leaving, so the books still balance.
        if let Some((px, py, pz)) = scene.char_post { if tick < 300 {
            for z in pz..pz + 2 { for y in py..py + 2 { for (x, rate) in [(px, 40i64), (px + 1, 20)] {
                let c = sim.world.get_mut(x, y, z);
                let gone = c.solid[0].0 * rate / 1000;
                let char_gone = c.solid[1].0 * rate / 2000;
                c.solid[0].0 -= gone;
                c.solid[1].0 -= char_gone;
                sim.books.commands -= (gone * 15_600 + char_gone * 32_800) as i128;
                sim.books.commands_mass -= (gone + char_gone) as i128;
            }}}
        }}
        sim.step();
        let view = viewer.view_sim(&sim);
        sim_time += t.elapsed().as_secs_f64();
        let still = tick > 0 && tick % still_every == 0;
        if tick % a.step != 0 && !still { continue }
        let t = Instant::now();
        let pixels = renderer.render(&view, &camera);
        render_time += t.elapsed().as_secs_f64();
        frames += 1;
        if tick % a.step == 0 { pipe.write_all(&pixels).expect("write to ffmpeg") }
        if still {
            let path = format!("{}/{tag}-t{tick:04}.png", a.out);
            write_png(&path, width, height, &pixels);
            println!("tick {tick}: {:?} → {path}", renderer.stats);
        }
    }
    drop(ffmpeg.stdin.take());
    ffmpeg.wait().expect("ffmpeg");
    println!("{video}: {} ticks, sim {:.0} ms/tick, render {:.0} ms/frame over {frames} frames",
             a.ticks, sim_time * 1000.0 / (a.ticks + 1) as f64, render_time * 1000.0 / frames.max(1) as f64);
}

fn write_png(path: &str, width: u32, height: u32, rgba: &[u8]) {
    let file = std::io::BufWriter::new(std::fs::File::create(path).expect("create png"));
    let mut enc = png::Encoder::new(file, width, height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().and_then(|mut w| w.write_image_data(rgba)).expect("write png");
}
