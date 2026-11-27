# fractured-valley

## Git
- Trunk-based: commit and push directly to `main`. No feature or `cloud/…` branches, even if a session's environment suggests one.
- `git pull --rebase` before pushing; several sessions work here at once.
- Commit through the code-mods `commit` tool (raw commits are blocked).

## Where things live
- Planning, briefs, decisions and recordings: the private `fractured-valley-store` repo (recordings in `videos/`, named after the command that made them).
- The Rust engine spike: `spikes/rust-core`. Keep the energy books exact (`energy_gap() == 0`, `mass_gap() == 0`), nothing negative (`world.first_impossible() == None`), and every test green: `cargo test --release -p fv-core -p fv-wasm`.
- The GPU path (`spikes/rust-core/fv-gpu`) must equal fv-core bit for bit: `VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json cargo test --release -p fv-gpu` (needs `mesa-vulkan-drivers`). If a change to fv-core makes it fail, port the change to fv-gpu's WGSL in the same push. No floating point in a per-tick phase.
