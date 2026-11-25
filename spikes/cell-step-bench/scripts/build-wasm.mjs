// Builds the Rust step kernel into several wasm variants (raw extern "C" exports, imported memory).
import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const crate = join(root, "rust");
const out = join(root, "wasm");
mkdirSync(out, { recursive: true });

const common = ["-C", "link-arg=--import-memory", "-C", "link-arg=--export=__heap_base"];
const shared = [
  "-C", "target-feature=+atomics,+bulk-memory,+mutable-globals",
  "-C", "link-arg=--shared-memory",
  "-C", "link-arg=--max-memory=1073741824",
];
const variants = {
  plain: [...common],
  simd: [...common, "-C", "target-feature=+simd128"],
  shared: [...common, ...shared],
  "shared-simd": [...common, ...shared, "-C", "target-feature=+simd128"],
};

for (const [name, flags] of Object.entries(variants)) {
  const targetDir = join(crate, "target", name);
  execFileSync("cargo", ["build", "--release", "--target", "wasm32-unknown-unknown", "--target-dir", targetDir], {
    cwd: crate,
    stdio: ["ignore", "inherit", "inherit"],
    env: { ...process.env, RUSTFLAGS: flags.join(" ") },
  });
  copyFileSync(join(targetDir, "wasm32-unknown-unknown", "release", "cell_step.wasm"), join(out, `cell_step.${name}.wasm`));
  console.log(`built wasm/cell_step.${name}.wasm`);
}
