# Spike: units in Rust

Dimension-checked, fixed-point quantities, after Gonogo's units discipline. A design probe for brief 05, decision 9. Not the engine.

```bash
cargo test
```

## What it shows

- **Dimensions live in the type** (powers of m, kg, s and K as `typenum` integers). It works on stable Rust; arithmetic on const generics would need nightly.
  - `joules / seconds` is `Power` and `watts * seconds` is `Energy`, worked out by the compiler.
  - `metres + seconds` doesn't compile.
- **Same dimension, different meaning:**
  - An absolute `Temperature` is a point: points subtract to a `TempDelta`, take a delta added, and never add to each other or scale.
  - `Moisture` (a fraction of dry mass) is dimensionless but doesn't add to a plain ratio.
- **Fixed point:** Q32.32 in `i64`, with products through `i128`, so the bits are identical everywhere. Overflow panics instead of wrapping.
- **A derived threshold:** `heat_to_ignite` warms the fuel to its decomposition temperature, warms its water to boiling and boils it off. With nothing but substance constants it lands within 1–17% of Rothermel's measured heat of pre-ignition (`Q_ig = 250 + 1116·Mf`) for moisture from 0 to 200%. A living trunk cell needs about 4× the energy of a dry post cell.

| Moisture | Ours (kJ/kg) | Rothermel | Apart |
|---|---|---|---|
| 0% | 484.5 | 581.5 | 17% |
| 10% | 745.8 | 841.1 | 11% |
| 30% | 1268.3 | 1360.2 | 7% |
| 100% | 3097.3 | 3177.3 | 3% |
| 200% | 5710.1 | 5773.1 | 1% |

## Findings

1. **Stable rustdoc ignores `compile_fail,E0308` error codes.** A doc test pinned to the wrong error still passed. The refusals are therefore guarded by `trybuild` (`tests/refusals.rs`), which compares each failure with its saved compiler error. A planted typo fails the suite. This is Gonogo's "a scan must prove it can see", one layer down.
2. **A single fixed-point scale doesn't fit physics.**
   - T⁴ for radiation overflows Q32.32 at 1,000 K.
   - The Stefan-Boltzmann constant (5.7e-8) has only a few significant bits.
   - The engine needs a scale per quantity *kind*, carried in the type like the dimension, or radiation done through a table. That's a design decision for brief 05.
3. **Compiler errors are verbose** (`Quantity<Dim<Z0, Z0, Z0, PInt<UInt<UTerm, B1>>>>`). Named aliases help in signatures but not in errors. Gonogo's readable messages came from TypeScript; a Rust version may want a small proc macro.
4. **Moisture × mass needed a named method** (`water_in`). The general `*` gives a `Plain` dimensionless mass, which is right dimensionally but loses the meaning. Choosing which kind combinations get names is a real design question.
