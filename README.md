# fractured-valley

An isometric pixel-art world simulation that runs in the browser: voxel materials, natural processes, living things, and peers playing together without a server.

Early days: this repo currently holds spikes, small experiments that inform the design.

```bash
pnpm install
pnpm dev
```

| Spike | What it explores |
|---|---|
| `spikes/voxel-sandbox` | 10 cm sparse voxel world, iso pixel-art renderer, deterministic fire, regrowth and falling blocks, lockstep play between peers |
| `spikes/cell-step-bench` | The same cell rules in TypeScript and Rust/WebAssembly, timed and checked for identical results |
| `spikes/brains-appetite` | One person whose appetite habit lags their activity after an injury: body model, utility brain, nightly retrospection; headless and exact (`cargo test --release`) |

Open the sandbox in two tabs to play together (`?net=broadcast`, the default), or on two machines with `?net=trystero&room=<name>`. `?net=solo` plays alone.
