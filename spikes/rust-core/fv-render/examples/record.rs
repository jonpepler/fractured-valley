//! Build a scene, run the simulation, render frames and pipe them to ffmpeg.
//!
//!   VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json \
//!   cargo run --release -p fv-render --example record -- --scene night-crib --size 64 --ticks 600
//!
//! Options (all optional):
//!   --scene S       tree, dead-tree, campfire, grass, post, puddle, puddle-shade, or the valley
//!   --sky day|night|moon afternoon sun, a clear moonless night, or a clear
//!                   night under a full moon (~0.25 lux)
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
//!   --radiation M   packets (Monte Carlo) or field (discrete ordinates); default: the sim's
//!   --light-ticks N average packet light over about N ticks (view only), default 10;
//!                   more for time-lapses and dim light, where few packets land.
//!                   Frames start once the light has averaged over that long.
//!   --out DIR       output directory
//! Writes DIR/<scene>.mp4 and DIR/<scene>-tNNNN.png (with -crib after the
//! scene name for the close-up).

use fv_core::rad::{Radiation, Sky};
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

const DEFAULT_OUT: &str = "render";

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
    radiation: Option<Radiation>,
    light_ticks: u64,
    out: String,
}

fn args() -> Args {
    let mut a = Args { scene: "tree".into(), sky: "day".into(), size: 64, height: 64, seed: 7, ticks: 600, step: 4, scale: 3, stills: 4, fps: 20, torch_j: 250, torch_ticks: 80, focus: "close".into(), radiation: None, light_ticks: 10, out: DEFAULT_OUT.into() };
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
            "--light-ticks" => a.light_ticks = num().max(1),
            "--radiation" => a.radiation = Some(match value { "field" => Radiation::Field, "packets" => Radiation::Packets, _ => panic!("--radiation is packets or field") }),
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
            let f = fv_core::scene::campfire(a.size, a.height);
            let (cx, cy, _) = f.fire;
            Scene { world: f.world, torch: f.torch, centre: [(cx + 2) as i32, (cy + 2) as i32, 6], frame: (200, 150), char_post: None }
        }
        "grass" => {
            // A field of dry grass on soil, lit along one edge.
            let mut w = fv_core::world::World::new(a.size, a.size, a.height, a.seed, fv_core::units::celsius(15));
            let subs = w.subs.clone();
            let t = fv_core::units::celsius(15);
            let soil = Cell::solid_of(SOIL, 1000, 150, t, &subs);
            let grass = Cell::solid_of(GRASS, 1000, 100, t, &subs);
            let n = a.size;
            for y in 0..n { for x in 0..n { for z in 0..4 { w.set(x, y, z, soil) } w.set(x, y, 4, grass) } }
            fv_core::scene::bond_all(&mut w, fv_core::scene::natural_bonds);
            let torch = (2..n - 2).map(|x| (x, 2, 4)).collect();
            Scene { world: w, torch, centre: [(n / 2) as i32, (n / 2) as i32, 4], frame: (2 * (n * 2) + 16, n * 2 + 24), char_post: None }
        }
        "puddle" | "puddle-shade" => {
            // A thin puddle on stone after rain (air 20 °C, 85 %), deepest in
            // the middle (0.3 mm), thinnest at its rim (0.02 mm), so it dries
            // from the edge inward. "puddle-shade" stands a 1.2 m L-shaped
            // stone wall on the two sunward sides, which shades all of it; the
            // wall is behind the puddle from the camera, so it hides nothing.
            let t = fv_core::units::celsius(20);
            let mut w = fv_core::world::World::new(a.size, a.size, a.height, a.seed, t);
            let subs = w.subs.clone();
            let stone = Cell::solid_of(STONE, 1000, 2, t, &subs);
            let n = a.size;
            for z in 0..4 { for y in 0..n { for x in 0..n { w.set(x, y, z, stone) } } }
            if a.scene == "puddle-shade" {
                for z in 4..16.min(a.height) { for y in 0..n { for x in 0..n { if x < 2 || y < 2 { w.set(x, y, z, stone) } } } }
            }
            fv_core::scene::bond_all(&mut w, fv_core::scene::natural_bonds);
            let humid = |c: &mut Cell| {
                let ml = c.gas_volume_ml(&subs);
                if ml <= 0 || c.gas_mass().0 == 0 { return }
                let sat = fv_core::tables::tables().saturated_at(t);
                c.gas[Gas::Vapour as usize] = fv_core::units::Mass(sat * 85 / 100 * ml / CELL_VOLUME_ML);
                c.set_temperature(t, &subs);
            };
            for c in w.cells.iter_mut() { if c.substance == NONE { humid(c) } }
            let mut outside = w.outside_air;
            humid(&mut outside);
            w.outside_air = outside;
            let (centre, r) = (n as f64 / 2.0 + 1.0, 3.5);
            for y in 0..n { for x in 0..n {
                let d = ((x as f64 + 0.5 - centre).powi(2) + (y as f64 + 0.5 - centre).powi(2)).sqrt();
                if d > r { continue }
                let mut p = Cell::puddle_um((20.0 + 280.0 * (1.0 - d / r)) as i64, t, &subs);
                humid(&mut p);
                w.set(x, y, 4, p);
            }}
            Scene { world: w, torch: vec![], centre: [(n / 2) as i32, (n / 2) as i32, 5], frame: (2 * (n * 2) + 16, n * 2 + 48), char_post: None }
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
    let sky = match a.sky.as_str() { "night" => Sky::NIGHT, "moon" => Sky::moon(), _ => Sky::afternoon() };
    std::fs::create_dir_all(&a.out).expect("make output directory");
    let scene = build_scene(&a);
    let base = scene.world.cells.len();
    let mut sim = Sim::new(scene.world, sky);
    if let Some(r) = a.radiation { sim.radiation = r }
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
    let tag = match a.radiation { Some(r) => format!("{}-{}-{}-{}", a.scene, a.sky, a.focus, format!("{r:?}").to_lowercase()), None => format!("{}-{}-{}", a.scene, a.sky, a.focus) };
    let mut renderer = Renderer::new_headless(width, height).expect("GPU");
    // Tell the camera how bright the sky is, so it knows day from night.
    renderer.exposure.sky_w_m2 = Some((sim.sky.sun_w_m2 + sim.sky.sky_w_m2) as f32);
    renderer.sky_light_w_m2 = Some(sim.sky.sky_w_m2 as f32);
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

    let open_water = |sim: &Sim| -> i64 { sim.world.cells.iter().filter(|c| c.open_water()).map(|c| c.water.0).sum() };
    let water_at_start = open_water(&sim);
    let still_every = if a.stills > 0 { (a.ticks / a.stills).max(1) } else { u64::MAX };
    let mut viewer = Viewer::new();
    viewer.blend = 1.0 / a.light_ticks as f32;
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
        if (tick % a.step != 0 && !still) || tick < a.light_ticks { continue }
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
    if water_at_start > 0 {
        let left = open_water(&sim);
        println!("open water: {:.2} g → {:.2} g ({:.0} % evaporated) over {:.0} simulated minutes",
                 water_at_start as f64 / 1e6, left as f64 / 1e6, 100.0 * (water_at_start - left) as f64 / water_at_start as f64, a.ticks as f64 * 0.05 / 60.0);
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
