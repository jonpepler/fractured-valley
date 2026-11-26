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
