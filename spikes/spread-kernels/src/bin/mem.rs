//! Memory per instance: raw overlay vs compiled delta, plus shared graph size.
use fv_spread_kernels::{harness::species, shared::Shared, delta::Delta};
fn main() {
    for &n in &[10_000usize, 100_000, 1_000_000] {
        let sp = species(n, 7);
        let sh = Shared::new(&sp.base);
        for (num, den) in [(1u32, 1u32), (1, 10)] {
            let spec = mc::gen::OverlaySpec::DEFAULT.scaled(num, den);
            let (mut raw, mut comp, mut ps, mut ov, mut ex) = (0, 0, 0, 0, 0);
            for who in 0..8 {
                let o = mc::gen::overlay(&sp.base, &sp.info, spec, 0x5eed, who);
                let d = Delta::new(&sh, &o);
                raw += o.bytes(); comp += d.bytes(); ps += d.den.len(); ov += d.ovr.len(); ex += d.extra.len();
            }
            println!("N {n} overlay {num}/{den}: raw overlay {} KB, compiled delta {} KB (patched sources {}, overrides {}, extra edges {}); shared {} MB (kernel-read {} MB)",
                raw / 8 >> 10, comp / 8 >> 10, ps / 8, ov / 8, ex / 8, sh.bytes() >> 20, sh.kernel_bytes() >> 20);
        }
    }
}
