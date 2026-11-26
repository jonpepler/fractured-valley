//! The simulation core in the browser.
//!
//! This crate wraps `fv-core` for WebAssembly so a web page can run the world
//! and keep the two multiplayer capabilities the native tests prove:
//!
//! * **Late join from a checkpoint**: `Engine::export` / `Engine::import`.
//! * **Splitting a tick's work between peers**: a tick can be driven in parts.
//!   `begin_tick` runs everything up to the diffusion phase; each peer then
//!   computes its own bricks with `diffuse_part` (bytes it can send), adds its
//!   own and everyone else's results (spot-checking a seeded quarter of the
//!   others'), and `finish_tick` runs the rest. The result is bit-identical to
//!   `Sim::step`, which is itself `step_before_diffuse` + diffusion +
//!   `step_after_diffuse` (the native tests in `tests/split.rs` check it).
//!
//! All the maths is integer, so the hashes here match a native build exactly.

use fv_core::cell::Cell;
use fv_core::rad::Sky;
use fv_core::sim::{Command, Scheduled, Sim};
use fv_core::transport::{apply_parts, diffuse_part, BrickResult};
use fv_core::units::*;
use fv_core::world::{Tally, World, BRICK_CELLS};
use fv_core::checkpoint;
use wasm_bindgen::prelude::*;

#[cfg(all(feature = "threads", target_arch = "wasm32"))]
pub use wasm_bindgen_rayon::init_thread_pool;

// ---------------------------------------------------------------------------
// The shared scenario (native example, native tests and the browser all use it)
// ---------------------------------------------------------------------------

/// The test scenario: a `n`×`n`×`nz` valley under the afternoon sun, with a
/// flame held to the log crib (250 J a tick) for the first `lit_ticks` ticks.
/// The same as `lit()` in `fv-core/tests/peers.rs`.
pub fn scenario(seed: u64, n: usize, nz: usize, lit_ticks: u64) -> Sim {
    let v = fv_core::scene::valley(seed, n, nz, 60);
    let (cx, cy) = (v.crib.0.min(n - 4), v.crib.1.min(n - 4));
    let gz = v.ground_at(cx, cy) as u16 + 1;
    let mut sim = Sim::new(v.world, Sky::afternoon());
    for t in 0..lit_ticks {
        sim.schedule(Scheduled { tick: t, peer: 0, seq: 0, command: Command::Heat { x: cx as u16, y: cy as u16, z: gz, energy: joules(250) } });
    }
    sim
}

/// The state hash as 16 hex digits (a u64 does not cross into JS cleanly).
pub fn hex(h: u64) -> String { format!("{h:016x}") }

// ---------------------------------------------------------------------------
// Driving one tick in parts
// ---------------------------------------------------------------------------

/// Everything in `Sim::step` before the diffusion phase (commands, air
/// motion). fv-core splits `step` at this point for us, so there is one copy
/// of the tick's order and it cannot drift.
pub fn begin_tick(sim: &mut Sim) { sim.step_before_diffuse() }

/// Everything in `Sim::step` after the diffusion phase, given every brick's
/// diffusion result (from all peers): make them the state, then the rest of
/// the tick (chemistry, radiation, structure, the books, the next tick).
pub fn finish_tick(sim: &mut Sim, parts: &[BrickResult]) {
    apply_parts(&mut sim.world, parts);
    sim.step_after_diffuse();
}

/// Is brick `b` in the seeded quarter a peer re-computes to check someone
/// else's work this tick? (The same rule as `fv-core/tests/peers.rs`.)
pub fn checked(w: &World, b: usize) -> bool { fv_core::rng::hash(w.seed, w.tick, b as u64, 0xc4ec) % 4 == 0 }

// ---------------------------------------------------------------------------
// Brick results as bytes (what one peer sends another)
// ---------------------------------------------------------------------------

/// The wire format of a peer's share of one phase. Little-endian:
/// `"FVB1"`, tick (u64), count (u32), then per brick: brick index (u32),
/// its tally (energy in, each gas in, radiation in, radiation out: i64s),
/// then its 512 cells (substance u8, bonds u8, then 14 i64s).
pub mod wire {
    use super::*;
    use fv_core::substance::GASES;

    const MAGIC: &[u8; 4] = b"FVB1";

    pub fn encode(tick: u64, parts: &[BrickResult]) -> Vec<u8> {
        let mut o = Vec::with_capacity(16 + parts.len() * (4 + 8 * (3 + GASES) + BRICK_CELLS * (2 + 8 * (7 + GASES))));
        o.extend_from_slice(MAGIC);
        o.extend_from_slice(&tick.to_le_bytes());
        o.extend_from_slice(&(parts.len() as u32).to_le_bytes());
        let i = |o: &mut Vec<u8>, v: i64| o.extend_from_slice(&v.to_le_bytes());
        for p in parts {
            o.extend_from_slice(&(p.brick as u32).to_le_bytes());
            let t = &p.tally;
            i(&mut o, t.energy_in);
            for g in t.gas_in { i(&mut o, g) }
            i(&mut o, t.radiation_in);
            i(&mut o, t.radiation_out);
            for c in &p.cells {
                o.push(c.substance);
                o.push(c.bonds);
                for m in c.solid { i(&mut o, m.0) }
                i(&mut o, c.water.0);
                for m in c.gas { i(&mut o, m.0) }
                i(&mut o, c.energy.0);
                i(&mut o, c.solid_energy.0);
                i(&mut o, c.nuclear.0);
            }
        }
        o
    }

    /// Reads bytes made by `encode`. Errors (rather than panicking) on
    /// anything malformed, since the bytes come from another machine.
    pub fn decode(b: &[u8], bricks: usize) -> Result<(u64, Vec<BrickResult>), String> {
        let mut r = Reader { b, at: 0 };
        if r.take(4)? != MAGIC { return Err("not a brick result".into()) }
        let tick = u64::from_le_bytes(r.take(8)?.try_into().unwrap());
        let n = u32::from_le_bytes(r.take(4)?.try_into().unwrap()) as usize;
        if n > bricks { return Err("more bricks than the world has".into()) }
        let mut parts = Vec::with_capacity(n);
        for _ in 0..n {
            let brick = r.u32()? as usize;
            if brick >= bricks { return Err(format!("brick {brick} out of range")) }
            let mut tally = Tally { energy_in: r.i64()?, ..Tally::default() };
            for g in tally.gas_in.iter_mut() { *g = r.i64()? }
            tally.radiation_in = r.i64()?;
            tally.radiation_out = r.i64()?;
            let mut cells = Vec::with_capacity(BRICK_CELLS);
            for _ in 0..BRICK_CELLS {
                let mut c = Cell { substance: r.u8()?, bonds: r.u8()?, ..Cell::default() };
                for m in c.solid.iter_mut() { *m = Mass(r.i64()?) }
                c.water = Mass(r.i64()?);
                for m in c.gas.iter_mut() { *m = Mass(r.i64()?) }
                c.energy = Energy(r.i64()?);
                c.solid_energy = Energy(r.i64()?);
                c.nuclear = Energy(r.i64()?);
                cells.push(c);
            }
            parts.push(BrickResult { brick, cells, tally });
        }
        if r.at != b.len() { return Err("trailing bytes".into()) }
        Ok((tick, parts))
    }

    struct Reader<'a> { b: &'a [u8], at: usize }
    impl Reader<'_> {
        fn take(&mut self, n: usize) -> Result<&[u8], String> {
            let s = self.b.get(self.at..self.at + n).ok_or("brick result cut short")?;
            self.at += n;
            Ok(s)
        }
        fn u8(&mut self) -> Result<u8, String> { Ok(self.take(1)?[0]) }
        fn u32(&mut self) -> Result<u32, String> { Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap())) }
        fn i64(&mut self) -> Result<i64, String> { Ok(i64::from_le_bytes(self.take(8)?.try_into().unwrap())) }
    }
}

// ---------------------------------------------------------------------------
// The JS-facing engine
// ---------------------------------------------------------------------------

/// Turn on (n != 1) or off (n == 1) the thread pool. In the single-threaded
/// build this does nothing (there is no pool). In the threaded build, call
/// `initThreadPool(k)` once first to start k Web Workers.
#[wasm_bindgen]
pub fn set_threads(n: u32) { fv_core::world::THREADS.store(n as usize, core::sync::atomic::Ordering::Relaxed) }

/// True in the build with shared memory and rayon on Web Workers.
#[wasm_bindgen]
pub fn threaded() -> bool { cfg!(feature = "threads") }

/// A world, plus the state of a tick being driven in parts.
#[wasm_bindgen]
pub struct Engine {
    sim: Sim,
    /// Between `begin_tick` and `finish_tick`: the brick results gathered so far.
    parts: Option<Vec<BrickResult>>,
}

fn err(e: impl Into<String>) -> JsError { JsError::new(&e.into()) }

#[wasm_bindgen]
impl Engine {
    /// The test scenario (see [`scenario`]): seed, horizontal size, height,
    /// and for how many ticks the crib is lit.
    #[wasm_bindgen(constructor)]
    pub fn new(seed: u32, n: u32, nz: u32, lit_ticks: u32) -> Engine {
        Engine { sim: scenario(seed as u64, n as usize, nz as usize, lit_ticks as u64), parts: None }
    }

    /// A plain valley, nothing lit. `sky` is "afternoon" or "night".
    pub fn valley(seed: u32, n: u32, nz: u32, grass_moisture: i32, sky: &str) -> Result<Engine, JsError> {
        let sky = match sky { "afternoon" => Sky::afternoon(), "night" => Sky::NIGHT, s => return Err(err(format!("unknown sky {s}"))) };
        let v = fv_core::scene::valley(seed as u64, n as usize, nz as usize, grass_moisture as i64);
        Ok(Engine { sim: Sim::new(v.world, sky), parts: None })
    }

    /// Join from a checkpoint made by `export`.
    pub fn import(bytes: &[u8]) -> Result<Engine, JsError> {
        // fv-core's import trusts its input; check the obvious things first so
        // a bad message gives an error instead of a crashed page. (catch_unwind
        // only helps natively: on the web a panic aborts the module.)
        if bytes.len() < 4 || &bytes[..4] != b"FVR1" { return Err(err("not a checkpoint")) }
        let sim = std::panic::catch_unwind(|| checkpoint::import(bytes)).map_err(|_| err("checkpoint is malformed"))?;
        Ok(Engine { sim, parts: None })
    }

    /// Can `export` run now? Not in the middle of a split tick, and not while
    /// pieces are falling (fv-core does not save rigid bodies yet).
    pub fn can_checkpoint(&self) -> bool { self.parts.is_none() && !self.sim.bodies.in_flight() }

    /// The whole simulation state as bytes (only when `can_checkpoint`).
    pub fn export(&self) -> Result<Vec<u8>, JsError> {
        if self.parts.is_some() { return Err(err("cannot checkpoint in the middle of a tick")) }
        if self.sim.bodies.in_flight() { return Err(err("cannot checkpoint while pieces are falling")) }
        Ok(checkpoint::export(&self.sim))
    }

    /// One whole tick, computed alone.
    pub fn step(&mut self) -> Result<(), JsError> {
        if self.parts.is_some() { return Err(err("a split tick is in progress")) }
        self.sim.step();
        Ok(())
    }

    /// Run `n` whole ticks and return each tick's hash, joined by commas
    /// (cheaper than crossing into JS every tick).
    pub fn run(&mut self, n: u32) -> Result<String, JsError> {
        let mut out = Vec::with_capacity(n as usize);
        for _ in 0..n { self.step()?; out.push(self.hash()) }
        Ok(out.join(","))
    }

    /// The next tick to simulate (as a JS number; fine below 2^53).
    pub fn tick(&self) -> f64 { self.sim.world.tick as f64 }
    pub fn hash(&self) -> String { hex(self.sim.hash()) }
    /// Energy made or lost so far (µJ). Must stay "0".
    pub fn energy_gap(&self) -> String { self.sim.energy_gap().to_string() }
    /// Mass made or lost so far (mg). Must stay "0".
    pub fn mass_gap(&self) -> String { self.sim.mass_gap().to_string() }
    pub fn brick_count(&self) -> u32 { self.sim.world.bricks() as u32 }
    pub fn size(&self) -> Vec<u32> { let w = &self.sim.world; vec![w.nx as u32, w.ny as u32, w.nz as u32] }

    /// Queue a flame held to a cell: `energy` joules at `tick`. `peer` and
    /// `seq` give commands for the same tick a fixed order on every peer.
    pub fn schedule_heat(&mut self, tick: f64, peer: u16, seq: u32, x: u16, y: u16, z: u16, energy_joules: i32) -> Result<(), JsError> {
        if tick < self.sim.world.tick as f64 { return Err(err("command for a tick already simulated")) }
        self.sim.schedule(Scheduled { tick: tick as u64, peer, seq, command: Command::Heat { x, y, z, energy: joules(energy_joules as i64) } });
        Ok(())
    }

    // --- one tick in parts -------------------------------------------------

    /// Start a split tick: everything up to the diffusion phase.
    pub fn begin_tick(&mut self) -> Result<(), JsError> {
        if self.parts.is_some() { return Err(err("a split tick is already in progress")) }
        begin_tick(&mut self.sim);
        self.parts = Some(Vec::new());
        Ok(())
    }

    /// Compute these bricks of this tick's diffusion phase, as bytes to send.
    /// Does not add them: call `add_part` with the bytes for our own share too.
    pub fn diffuse_part(&self, bricks: &[u32]) -> Result<Vec<u8>, JsError> {
        if self.parts.is_none() { return Err(err("call begin_tick first")) }
        let n = self.sim.world.bricks();
        if let Some(b) = bricks.iter().find(|b| **b as usize >= n) { return Err(err(format!("brick {b} out of range"))) }
        let ids: Vec<usize> = bricks.iter().map(|b| *b as usize).collect();
        Ok(wire::encode(self.sim.world.tick, &diffuse_part(&self.sim.world, &self.sim.flow, &ids)))
    }

    /// Add a share we trust (our own).
    pub fn add_part(&mut self, bytes: &[u8]) -> Result<(), JsError> {
        let (tick, parts) = wire::decode(bytes, self.sim.world.bricks()).map_err(err)?;
        if tick != self.sim.world.tick { return Err(err(format!("brick results are for tick {tick}, not {}", self.sim.world.tick))) }
        self.parts.as_mut().ok_or_else(|| err("call begin_tick first"))?.extend(parts);
        Ok(())
    }

    /// Add another peer's share after spot-checking it: re-compute the seeded
    /// quarter of its bricks (see [`checked`]) and compare. Any brick that
    /// differs is replaced by our own result, so a wrong value never enters
    /// our world. Returns the indices of the bricks that were wrong (empty if
    /// the peer was honest, as far as the check can tell).
    pub fn check_and_add_part(&mut self, bytes: &[u8]) -> Result<Vec<u32>, JsError> {
        let (tick, mut theirs) = wire::decode(bytes, self.sim.world.bricks()).map_err(err)?;
        if tick != self.sim.world.tick { return Err(err(format!("brick results are for tick {tick}, not {}", self.sim.world.tick))) }
        if self.parts.is_none() { return Err(err("call begin_tick first")) }
        let w = &self.sim.world;
        let check: Vec<usize> = theirs.iter().map(|r| r.brick).filter(|b| checked(w, *b)).collect();
        let redo = diffuse_part(w, &self.sim.flow, &check);
        let mut bad = Vec::new();
        for r in redo {
            let got = theirs.iter_mut().find(|t| t.brick == r.brick).unwrap();
            if *got != r { bad.push(r.brick as u32); *got = r }
        }
        self.parts.as_mut().unwrap().extend(theirs);
        Ok(bad)
    }

    /// How many of a share's bricks `check_and_add_part` would re-compute.
    pub fn checked_in(&self, bytes: &[u8]) -> Result<u32, JsError> {
        let (_, parts) = wire::decode(bytes, self.sim.world.bricks()).map_err(err)?;
        Ok(parts.iter().filter(|p| checked(&self.sim.world, p.brick)).count() as u32)
    }

    /// Finish the split tick from every brick's result. Every brick must have
    /// arrived exactly once.
    pub fn finish_tick(&mut self) -> Result<(), JsError> {
        let parts = self.parts.as_ref().ok_or_else(|| err("call begin_tick first"))?;
        let mut seen = vec![false; self.sim.world.bricks()];
        for p in parts {
            if std::mem::replace(&mut seen[p.brick], true) { return Err(err(format!("brick {} arrived twice", p.brick))) }
        }
        if let Some(b) = seen.iter().position(|s| !s) { return Err(err(format!("brick {b} is missing"))) }
        let parts = self.parts.take().unwrap();
        finish_tick(&mut self.sim, &parts);
        Ok(())
    }

    /// Test helper: a copy of a share with one wrong value, in a brick the
    /// receiver will check this tick (energy of cell 7 off by one).
    pub fn corrupt_for_test(&self, bytes: &[u8]) -> Result<Vec<u8>, JsError> {
        let (tick, mut parts) = wire::decode(bytes, self.sim.world.bricks()).map_err(err)?;
        let w = &self.sim.world;
        let k = parts.iter().position(|r| checked(w, r.brick)).ok_or_else(|| err("no checked brick in this share"))?;
        parts[k].cells[7].energy.0 ^= 1;
        Ok(wire::encode(tick, &parts))
    }
}
