//! Render a small valley headless and check the picture has something in it.
//! Needs a GPU adapter; on a machine without one, point Vulkan at Mesa's
//! software driver: VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json

use fv_core::rad::Sky;
use fv_core::scene::valley;
use fv_core::sim::Sim;
use fv_core::view::Viewer;
use fv_render::{Camera, Renderer};

#[test]
fn renders_a_small_valley() {
    let v = valley(3, 24, 32, 120);
    let mut sim = Sim::new(v.world, Sky::afternoon());
    let mut viewer = Viewer::new();
    let mut view = viewer.view(&sim.world, &sim.light);
    for _ in 0..4 {
        sim.step();
        view = viewer.view(&sim.world, &sim.light);
    }
    let (w, h) = (160, 120);
    let mut r = Renderer::new_headless(w, h).expect("a GPU adapter (try lavapipe, see the file header)");
    let px = r.render(&view, &Camera { centre: [12, 12, 8], scale: 1 });
    assert_eq!(px.len(), (w * h * 4) as usize);
    // Not one flat colour: count distinct pixels.
    let mut colours: Vec<[u8; 4]> = px.chunks(4).map(|c| [c[0], c[1], c[2], c[3]]).collect();
    colours.sort();
    colours.dedup();
    assert!(colours.len() > 20, "image looks uniform: {} colours", colours.len());
    assert!(r.stats.blocks > 100, "few sprites: {:?}", r.stats);
}

#[test]
fn a_puddle_darkens_the_stone_it_lies_on() {
    // Wet things look darker (Lekner & Dorf 1988). One sunlit view of stone
    // with a thin puddle on it, drawn as it is and with the water taken out
    // of the snapshot: only the pixels under the water change, and they are
    // darker on average.
    use fv_core::cell::Cell;
    use fv_core::substance::STONE;
    use fv_core::units::celsius;
    use fv_core::world::World;
    let t = celsius(20);
    let mut w = World::new(8, 8, 8, 1, t);
    let subs = w.subs.clone();
    let stone = Cell::solid_of(STONE, 1000, 2, t, &subs);
    for z in 0..3 { for y in 0..8 { for x in 0..8 { w.set(x, y, z, stone) } } }
    for y in 3..5 { for x in 3..5 { w.set(x, y, 3, Cell::puddle_um(200, t, &subs)) } }
    let mut sim = Sim::new(w, Sky::afternoon());
    let mut viewer = Viewer::new();
    for _ in 0..20 { sim.step(); viewer.view(&sim.world, &sim.light); }
    let wet_view = viewer.view(&sim.world, &sim.light);
    assert!(wet_view.cells.iter().filter(|c| c.water_mm > 0.0).count() == 4, "the view reports the water");
    let mut dry_view = viewer.view(&sim.world, &sim.light);
    dry_view.cells = wet_view.cells.clone();
    for c in &mut dry_view.cells { c.water_mm = 0.0 }
    let mut r = Renderer::new_headless(64, 48).expect("a GPU adapter (try lavapipe, see the file header)");
    r.exposure.fixed = Some(300.0);
    let camera = Camera { centre: [4, 4, 3], scale: 1 };
    let (dry, wet) = (r.render(&dry_view, &camera), r.render(&wet_view, &camera));
    let lum = |p: &[u8]| p[0] as f64 * 0.2126 + p[1] as f64 * 0.7152 + p[2] as f64 * 0.0722;
    let changed: Vec<(f64, f64)> = dry.chunks(4).zip(wet.chunks(4)).filter(|(a, b)| a != b).map(|(a, b)| (lum(a), lum(b))).collect();
    // Four cells' top faces are 4 × 14 pixels.
    assert!((10..=56).contains(&changed.len()), "only the wet faces change: {} pixels", changed.len());
    let (d, w): (f64, f64) = changed.iter().fold((0.0, 0.0), |(d, w), (a, b)| (d + a, w + b));
    assert!(w < d * 0.95, "and they look darker: {:.0} against {:.0} dry", w / changed.len() as f64, d / changed.len() as f64);
}
