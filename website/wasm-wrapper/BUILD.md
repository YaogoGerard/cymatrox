# Build the real simulator (WASM)

The site ships in two states:

- **As downloaded**: `script.js` falls back to a JavaScript Chladni
  approximation (see the `USING WASM = false` banner in the simulator).
  It runs immediately, in any browser, no build step — but it is **not**
  cymatrox, it's a stand-in.
- **After this build step**: `script.js` detects `pkg/cymatrox_web.js`
  and switches to calling the real, published `cymatrox` crate (v0.1.0)
  through the `wasm-wrapper/` bridge in this folder, compiled to
  WebAssembly. This is the actual crate, actually running, in the browser.

Claude could not perform this build: the sandbox this site was generated
in has no Rust toolchain and no way to install one (network is
allow-listed to package registries only, not `sh.rustup.rs`). Everything
in `wasm-wrapper/src/lib.rs` was written against the real, inspected
source of `cymatrox` v0.1.0 (downloaded from crates.io while writing
this), but it has never been compiled — treat it as a first draft to
build and debug, not verified working code.

## Steps

```sh
# 1. Install the wasm target (skip if already installed)
rustup target add wasm32-unknown-unknown

# 2. Install wasm-pack (skip if already installed)
cargo install wasm-pack

# 3. Build the bridge crate
cd wasm-wrapper
wasm-pack build --target web --out-dir ../pkg

# 4. Serve the site (WASM requires a real HTTP server, not file://)
cd ..
python3 -m http.server 8000
# open http://localhost:8000
```

## If it doesn't compile on the first try

Likely culprits, roughly in order of likelihood:

- **wgpu's WebGPU backend needs the browser API present.** Test in a
  browser that ships WebGPU (recent Chrome/Edge); Firefox/Safari support
  is still rolling out as of this writing.
- **Struct field mismatches.** This wrapper was written by reading
  `cymatrox-0.1.0`'s source directly, but a patch release could shift
  field names/types. Compare `wasm-wrapper/src/lib.rs` against the actual
  installed version's `src/{granular,fluid,acoustic}/config.rs`.
- **`GpuContext::new()` failing at runtime** (not compile time) with "no
  compatible GPU backend found" — the browser/machine running the page
  doesn't expose WebGPU. Per ADR-0008, cymatrox has no CPU fallback by
  design, so this is a hard requirement, not a bug to work around.

## What's simplified in the bridge

To keep the wrapper small, each `*_start()` function exposes only the
parameters the site's UI actually controls (frequency, grid size, mode
numbers) and hardcodes the rest to reasonable defaults (the README's
example values, or the site's documented defaults — e.g. the acoustic
module's 1&nbsp;mm water droplet in air). The full config surface
(`LiquidSpec`, `SolverParams`, measured eigenmodes, etc.) is real and
present in cymatrox — this bridge just doesn't expose every knob to the
web UI yet.
