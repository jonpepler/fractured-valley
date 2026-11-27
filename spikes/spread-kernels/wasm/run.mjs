// Run a wasm32-wasip1 build under node: node wasm/run.mjs <file.wasm> [args...]
import { readFile } from 'node:fs/promises';
import { WASI } from 'node:wasi';
import { argv } from 'node:process';
const wasi = new WASI({ version: 'preview1', args: argv.slice(2), env: {}, preopens: {} });
const mod = await WebAssembly.compile(await readFile(argv[2]));
const inst = await WebAssembly.instantiate(mod, wasi.getImportObject());
wasi.start(inst);
