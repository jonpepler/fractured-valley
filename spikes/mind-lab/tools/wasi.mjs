// Runs a wasm32-wasip1 binary under node's built-in WASI: node tools/wasi.mjs path/to/bin.wasm
import { readFile } from "node:fs/promises";
import { WASI } from "node:wasi";
import { argv, env } from "node:process";
const wasi = new WASI({ version: "preview1", args: [argv[2], ...argv.slice(3)], env, returnOnExit: true });
const mod = await WebAssembly.compile(await readFile(argv[2]));
const inst = await WebAssembly.instantiate(mod, wasi.getImportObject());
process.exitCode = wasi.start(inst);
