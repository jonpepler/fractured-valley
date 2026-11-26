# fractured-valley

## Git
- Trunk-based: commit and push directly to `main`. No feature or `cloud/…` branches, even if a session's environment suggests one.
- `git pull --rebase` before pushing; several sessions work here at once.
- Commit through the code-mods `commit` tool (raw commits are blocked).

## Where things live
- Planning, briefs, decisions and recordings: the private `fractured-valley-store` repo (recordings in `videos/`, named after the command that made them).
- The Rust engine spike: `spikes/rust-core`. Keep the energy books exact (`energy_gap() == 0`, `mass_gap() == 0`), nothing negative (`world.first_impossible() == None`), and every test green: `cargo test --release -p fv-core -p fv-wasm`.
