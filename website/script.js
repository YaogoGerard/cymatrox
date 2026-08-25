/* =================================================================
   HERO — always a JS Chladni approximation (decorative showcase,
   not the simulator). See simulator section below for the real
   cymatrox integration.
   ================================================================= */

function chladni(x, y, n, m) {
  return Math.cos(n * Math.PI * x) * Math.cos(m * Math.PI * y)
       - Math.cos(m * Math.PI * x) * Math.cos(n * Math.PI * y);
}

function drawChladni2D(canvas, n, m, accentHex) {
  const ctx = canvas.getContext("2d");
  const w = canvas.width, h = canvas.height;
  const img = ctx.createImageData(w, h);
  const accent = hexToRgb(accentHex);
  const bg = hexToRgb("#0e1e37");

  for (let py = 0; py < h; py++) {
    const y = (py / h) * 2 - 1;
    for (let px = 0; px < w; px++) {
      const x = (px / w) * 2 - 1;
      const v = Math.abs(chladni(x, y, n, m));
      const t = Math.max(0, 1 - v * 3.2);
      const idx = (py * w + px) * 4;
      img.data[idx] = lerp(bg.r, accent.r, t);
      img.data[idx + 1] = lerp(bg.g, accent.g, t);
      img.data[idx + 2] = lerp(bg.b, accent.b, t);
      img.data[idx + 3] = 255;
    }
  }
  ctx.putImageData(img, 0, 0);
}

function lerp(a, b, t) { return a + (b - a) * t; }
function hexToRgb(hex) {
  const v = parseInt(hex.replace("#", ""), 16);
  return { r: (v >> 16) & 255, g: (v >> 8) & 255, b: v & 255 };
}

(function heroLoop() {
  const canvas = document.getElementById("chladni-hero");
  if (!canvas) return;
  const modes = [[3, 5], [2, 3], [4, 7], [1, 6], [5, 6], [3, 4]];
  let i = 0;
  function tick() {
    const [n, m] = modes[i % modes.length];
    drawChladni2D(canvas, n, m, "#4fd1c5");
    document.getElementById("hero-n").textContent = n;
    document.getElementById("hero-m").textContent = m;
    i++;
  }
  tick();
  if (!window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
    setInterval(tick, 3200);
  }
})();

/* =================================================================
   SIMULATOR — priority order:
     1. real exported frames  (website/data/*.json[.gz], produced by
        `cargo run --release --example export_frames` against the
        published crate — no physics lives in this page)
     2. live WASM bridge      (wasm-wrapper/, experimental)
     3. JS Chladni fallback   (clearly labelled demo mode)
   ================================================================= */

const MODULES = ["granular", "fluid", "acoustic"];
const ACCENTS = { granular: "#4fd1c5", fluid: "#4fd1c5", acoustic: "#f2a65a" };

(async function simulator() {
  const canvas = document.getElementById("sim-canvas");
  if (!canvas) return;

  const statusEl = document.getElementById("wasm-status");
  const freqInput = document.getElementById("freq");
  const nInput = document.getElementById("mode-n");
  const mInput = document.getElementById("mode-m");
  const gridInput = document.getElementById("grid-size");
  const exportCode = document.getElementById("export-code");
  const copyBtn = document.getElementById("copy-btn");
  const viewBtns = document.querySelectorAll(".view-btn");
  const tabBtns = document.querySelectorAll(".tab-btn");
  const playBtn = document.getElementById("data-play");
  const slider = document.getElementById("data-slider");
  const frameLabel = document.getElementById("data-frame");
  const dataControls = document.getElementById("data-controls");
  const fileInput = document.getElementById("data-file");

  let wasm = null;
  let wasmReady = false;
  let currentModule = "granular";
  let view = "2d";
  let three = null;
  const started = { granular: false, fluid: false, acoustic: false };

  // ---- real-data layer -------------------------------------------------
  const DATA = { granular: null, fluid: null, acoustic: null }; // {meta, frames}
  let playing = true;
  let frameIdx = 0;
  let lastTs = 0;
  const FRAME_MS = 1000 / 24;

  function escapeHtml(s) {
    return s.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
  }

  async function decodeBody(buf, isGz) {
    if (!isGz) return new TextDecoder().decode(buf);
    if (!("DecompressionStream" in window)) throw new Error("DecompressionStream unavailable");
    const ds = new DecompressionStream("gzip");
    const stream = new Blob([buf]).stream().pipeThrough(ds);
    return await new Response(stream).text();
  }

  async function loadDataset(name) {
    for (const url of [`data/${name}.json.gz`, `data/${name}.json`]) {
      try {
        const res = await fetch(url);
        if (!res.ok) continue;
        const text = await decodeBody(await res.arrayBuffer(), url.endsWith(".gz"));
        const parsed = JSON.parse(text);
        if (parsed && parsed.frames && parsed.frames.length) return parsed;
      } catch (e) { /* try next source */ }
    }
    return null;
  }

  async function loadAllDatasets() {
    const results = await Promise.all(MODULES.map(loadDataset));
    MODULES.forEach((m, i) => { DATA[m] = results[i]; });
    refreshStatus();
    syncDataControls();
    renderCurrentFrame();
    if (three && three.ready()) pushFrameToThree();
  }

  function hasReal(m) { return !!(DATA[m] && DATA[m].frames.length); }

  function refreshStatus() {
    const loaded = MODULES.filter(hasReal);
    if (loaded.length === MODULES.length) {
      statusEl.innerHTML =
        'Données réelles du crate <span class="wasm-banner is-live">export v0.1.0 — lecture locale</span>' +
        `<br><span style="font-size:12px">${MODULES.map((m) => `${m}: ${DATA[m].frames.length} frames`).join(" · ")} — régénérables via <code class="mono">cargo run --example export_frames</code></span>`;
    } else if (loaded.length) {
      statusEl.innerHTML =
        `Données réelles partielles (${escapeHtml(loaded.join(", "))}) <span class="wasm-banner is-live">lecture locale</span>`;
    } else if (!wasmReady) {
      statusEl.innerHTML =
        'Mode démo — approximation JS, pas le vrai calcul ' +
        '<span class="wasm-banner is-fallback">aucune donnée ni WASM</span>' +
        `<br><span style="font-size:12px">Génère les données : <code class="mono">cargo run --example export_frames</code> puis serve le dossier en HTTP et recharge — ou charge les fichiers ci-dessous.</span>`;
    }
  }

  function syncDataControls() {
    const on = hasReal(currentModule);
    dataControls.style.display = on ? "" : "none";
    if (!on) return;
    const total = DATA[currentModule].frames.length;
    frameIdx = Math.min(frameIdx, total - 1);
    slider.max = String(total - 1);
    slider.value = String(frameIdx);
    updateFrameLabel();
  }

  function updateFrameLabel() {
    const total = hasReal(currentModule) ? DATA[currentModule].frames.length : 0;
    frameLabel.textContent = `${frameIdx + 1} / ${total}`;
  }

  playBtn.addEventListener("click", () => {
    playing = !playing;
    playBtn.textContent = playing ? "Pause" : "Lecture";
  });
  slider.addEventListener("input", () => {
    playing = false;
    playBtn.textContent = "Lecture";
    frameIdx = Number(slider.value);
    updateFrameLabel();
    renderCurrentFrame();
    pushFrameToThree();
  });

  // ---- WASM bridge attempt (experimental; see wasm-wrapper/BUILD.md) ---
  try {
    const mod = await import("./pkg/cymatrox_web.js");
    await mod.default();      // wasm-bindgen web-target init
    await mod.init_gpu();     // GpuContext::new() — needs WebGPU
    wasm = mod;
    wasmReady = true;
    if (!MODULES.some(hasReal)) {
      statusEl.innerHTML =
        'Piloté par le vrai crate <span class="wasm-banner is-live">WASM actif — cymatrox v0.1.0</span>';
    }
  } catch (err) {
    wasmReady = false;
    console.warn("cymatrox WASM not available:", err);
  }

  loadAllDatasets();

  // ---- UI plumbing ------------------------------------------------------
  function showControlsFor(moduleName) {
    document.querySelectorAll("[data-module-only]").forEach((el) => {
      const allowed = el.dataset.moduleOnly.split(" ");
      el.style.display = allowed.includes(moduleName) ? "" : "none";
    });
  }

  function currentValues() {
    return {
      freq: Number(freqInput.value),
      n: Number(nInput.value),
      m: Number(mInput.value),
      grid: Number(gridInput.value),
    };
  }

  function updateExportCode() {
    const { freq, n, m, grid } = currentValues();
    if (currentModule === "granular") {
      exportCode.textContent =
        `PlateSpec::Idealized { side: 0.5 }\n` +
        `Driving { frequency_hz: ${freq}.0, modes: Explicit(vec![(${m}, ${n})]) }`;
    } else if (currentModule === "fluid") {
      exportCode.textContent =
        `SurfaceGrid { width: ${grid}, height: ${grid}, .. }\n` +
        `Driving { frequency_hz: ${freq}.0, amplitude: 2.0 }`;
    } else {
      exportCode.textContent =
        `VolumeGrid { width: ${grid}, height: ${grid}, depth: ${grid}, .. }\n` +
        `Driving { frequency_hz: ${freq}.0, amplitude: 1.0 }`;
    }
  }

  function restartActiveModule() {
    if (hasReal(currentModule)) { syncDataControls(); renderCurrentFrame(); pushFrameToThree(); return; }
    if (!wasmReady) { fallbackRender(); return; }
    const { freq, n, m, grid } = currentValues();
    try {
      if (currentModule === "granular") {
        wasm.granular_start(20000, 0.5, freq, m, n, 42n);
        started.granular = true;
      } else if (currentModule === "fluid") {
        wasm.fluid_start(grid, grid, 0.2, 0.2, freq, true, 0.09, 42n);
        started.fluid = true;
      } else if (currentModule === "acoustic") {
        wasm.acoustic_start(grid, grid, grid, 0.1, freq, 42n);
        started.acoustic = true;
      }
    } catch (err) {
      console.error(`Failed to start ${currentModule}:`, err);
      statusEl.innerHTML =
        `<span class="wasm-banner is-fallback">Erreur au démarrage de ${currentModule} — ${escapeHtml(String(err))}</span>`;
    }
  }

  // ---- 2D rendering -----------------------------------------------------
  function renderGranularFrame(flatXY, side = 0.5) {
    const ctx = canvas.getContext("2d");
    ctx.fillStyle = "#0e1e37";
    ctx.fillRect(0, 0, canvas.width, canvas.height);
    ctx.fillStyle = "#4fd1c5";
    const half = side / 2;
    for (let i = 0; i < flatXY.length; i += 2) {
      const px = ((flatXY[i] + half) / (2 * half)) * canvas.width;
      const py = ((flatXY[i + 1] + half) / (2 * half)) * canvas.height;
      ctx.fillRect(px, py, 1.4, 1.4);
    }
  }

  function heatmapToCtx(values, gridW, accentHex) {
    const rows = Math.floor(values.length / gridW);
    const img = new ImageData(gridW, rows);
    const accent = hexToRgb(accentHex);
    const bg = hexToRgb("#0e1e37");
    let min = Infinity, max = -Infinity;
    for (const v of values) { if (v < min) min = v; if (v > max) max = v; }
    const range = Math.max(max - min, 1e-9);
    for (let i = 0; i < values.length; i++) {
      const t = (values[i] - min) / range;
      img.data[i * 4]     = lerp(bg.r, accent.r, t);
      img.data[i * 4 + 1] = lerp(bg.g, accent.g, t);
      img.data[i * 4 + 2] = lerp(bg.b, accent.b, t);
      img.data[i * 4 + 3] = 255;
    }
    const tmp = document.createElement("canvas");
    tmp.width = gridW; tmp.height = rows;
    tmp.getContext("2d").putImageData(img, 0, 0);
    return tmp;
  }

  function renderHeightmapFrame(values, gridW, accentHex) {
    const ctx = canvas.getContext("2d");
    const tmp = heatmapToCtx(values, gridW, accentHex);
    ctx.imageSmoothingEnabled = true;
    ctx.drawImage(tmp, 0, 0, canvas.width, canvas.height);
  }

  function midZSlice(frame, meta) {
    const plane = meta.out_x * meta.out_y;
    const zMid = meta.out_z >> 1;
    return frame.subarray(zMid * plane, (zMid + 1) * plane);
  }

  function currentFrame() {
    if (!hasReal(currentModule)) return null;
    const d = DATA[currentModule];
    if (!(d.frames[frameIdx] instanceof Float32Array)) {
      d.frames[frameIdx] = Float32Array.from(d.frames[frameIdx]);
    }
    return d.frames[frameIdx];
  }

  function renderCurrentFrame() {
    const f = currentFrame();
    if (!f) { fallbackRender(); return; }
    const meta = DATA[currentModule].meta;
    if (currentModule === "granular") {
      renderGranularFrame(f, meta.side || 0.5);
    } else if (currentModule === "fluid") {
      renderHeightmapFrame(f, meta.out_x, ACCENTS.fluid);
    } else {
      renderHeightmapFrame(midZSlice(f, meta), meta.out_x, ACCENTS.acoustic);
    }
  }

  function pushFrameToThree() {
    if (!three || !three.ready() || view !== "3d") return;
    const f = currentFrame();
    if (f) three.update(currentModule, f, DATA[currentModule].meta);
    else {
      const { n, m } = currentValues();
      three.update(null, null, null, n, m);
    }
  }

  function fallbackRender() {
    const { n, m } = currentValues();
    drawChladni2D(canvas, n, m, "#4fd1c5");
  }

  // ---- main loop ---------------------------------------------------------
  function tick(ts) {
    requestAnimationFrame(tick);
    const dtms = ts - lastTs;
    lastTs = ts;

    if (hasReal(currentModule)) {
      if (playing && dtms >= FRAME_MS - 2) {
        frameIdx = (frameIdx + 1) % DATA[currentModule].frames.length;
        slider.value = String(frameIdx);
        updateFrameLabel();
        renderCurrentFrame();
        pushFrameToThree();
      }
      return;
    }

    // No real data → WASM live mode (2D only), otherwise nothing to do.
    if (!wasmReady || view === "3d") return;
    try {
      if (currentModule === "granular" && started.granular) {
        renderGranularFrame(Float32Array.from(wasm.granular_step()));
      } else if (currentModule === "fluid" && started.fluid) {
        const { grid } = currentValues();
        renderHeightmapFrame(Float32Array.from(wasm.fluid_step()), grid, ACCENTS.fluid);
      } else if (currentModule === "acoustic" && started.acoustic) {
        const { grid } = currentValues();
        const full = Float32Array.from(wasm.acoustic_step());
        const zMid = grid >> 1;
        renderHeightmapFrame(full.subarray(zMid * grid * grid, (zMid + 1) * grid * grid), grid, ACCENTS.acoustic);
      }
    } catch (err) {
      console.error("step() failed:", err);
    }
  }
  requestAnimationFrame(tick);

  // ---- wiring ------------------------------------------------------------
  function onModuleChange(name) {
    currentModule = name;
    tabBtns.forEach((b) => b.classList.toggle("is-active", b.dataset.module === name));
    showControlsFor(name);
    updateExportCode();
    restartActiveModule();
  }

  tabBtns.forEach((btn) => btn.addEventListener("click", () => onModuleChange(btn.dataset.module)));

  [freqInput, nInput, mInput, gridInput].forEach((el) =>
    el.addEventListener("input", () => {
      document.getElementById("freq-val").textContent = freqInput.value;
      document.getElementById("mode-n-val").textContent = nInput.value;
      document.getElementById("mode-m-val").textContent = mInput.value;
      document.getElementById("grid-size-val").textContent = gridInput.value;
      updateExportCode();
      if (hasReal(currentModule)) return; // sliders cannot replay recorded data
      if (wasmReady) {
        if (currentModule === "granular" && started.granular) {
          try { wasm.granular_set_frequency(Number(freqInput.value)); } catch {}
        } else {
          restartActiveModule();
        }
      } else if (view !== "3d") {
        fallbackRender();
      } else if (three && three.ready()) {
        const { n, m } = currentValues();
        three.update(null, null, null, n, m);
      }
    })
  );

  copyBtn.addEventListener("click", async () => {
    try {
      await navigator.clipboard.writeText(exportCode.textContent);
      copyBtn.textContent = "Copié";
      setTimeout(() => (copyBtn.textContent = "Copier"), 1400);
    } catch { /* clipboard unavailable — ignore */ }
  });

  viewBtns.forEach((btn) => {
    btn.addEventListener("click", () => {
      viewBtns.forEach((b) => b.classList.remove("is-active"));
      btn.classList.add("is-active");
      view = btn.dataset.view;
      if (view === "3d") {
        canvas.style.display = "none";
        if (!three) three = initThree(canvas.parentElement);
        three.show();
        pushFrameToThree();
      } else {
        if (three) three.hide();
        canvas.style.display = "block";
        renderCurrentFrame(); // repaint 2D after 3D overlay
      }
    });
  });

  showControlsFor(currentModule);
  updateExportCode();
  restartActiveModule();
})();

/* =================================================================
   3D VIEW — driven by REAL exported frames when available:
     granular → THREE.Points cloud on the plate
     fluid    → displaced vertex-coloured mesh (η height field)
     acoustic → CanvasTexture plane of the mid-z pressure slice
   Falls back to the decorative JS Chladni surface otherwise.
   Objects are built lazily per module and reused across frames.
   ================================================================= */
function initThree(container) {
  const script = document.createElement("script");
  const state = { ready: false };
  let renderer, scene;
  let decoMesh = null;                       // JS-chladni decoration
  let pointsObj = null, fieldMesh = null, texPlane = null;

  script.src = "https://cdnjs.cloudflare.com/ajax/libs/three.js/r128/three.min.js";
  script.onload = () => setup();
  document.head.appendChild(script);

  function setup() {
    const THREE = window.THREE;
    const size = container.clientWidth || 480;

    scene = new THREE.Scene();
    camera = new THREE.PerspectiveCamera(45, 1, 0.01, 100);
    camera.position.set(0, 1.15, 1.6);
    camera.lookAt(0, 0, 0);

    renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true });
    renderer.setSize(size, size);
    renderer.domElement.style.width = "100%";
    renderer.domElement.style.maxWidth = "480px";
    renderer.domElement.style.aspectRatio = "1";
    container.appendChild(renderer.domElement);

    const geo = new THREE.PlaneGeometry(1.4, 1.4, 80, 80);
    geo.rotateX(-Math.PI / 2);
    decoMesh = new THREE.Mesh(
      geo,
      new THREE.MeshBasicMaterial({ color: 0x4fd1c5, wireframe: true })
    );
    scene.add(decoMesh);

    state.ready = true;
    animate();
  }

  let cameraRef = null; // eslint-disable-line no-unused-vars

  function ensurePoints(count) {
    const THREE = window.THREE;
    if (pointsObj && pointsObj.userData.capacity >= count) return pointsObj;
    if (pointsObj) { scene.remove(pointsObj); pointsObj.geometry.dispose(); pointsObj.material.dispose(); }
    const g = new THREE.BufferGeometry();
    g.setAttribute("position", new THREE.BufferAttribute(new Float32Array(count * 3), 3));
    pointsObj = new THREE.Points(g, new THREE.PointsMaterial({
      color: 0x4fd1c5, size: 0.006, sizeAttenuation: true,
    }));
    pointsObj.userData.capacity = count;
    pointsObj.frustumCulled = false;
    scene.add(pointsObj);
    return pointsObj;
  }

  function ensureFieldMesh(nx, ny) {
    const THREE = window.THREE;
    if (fieldMesh && fieldMesh.userData.nx === nx && fieldMesh.userData.ny === ny) return fieldMesh;
    if (fieldMesh) { scene.remove(fieldMesh); fieldMesh.geometry.dispose(); fieldMesh.material.dispose(); }
    const geo = new THREE.PlaneGeometry(1.4, 1.4, nx - 1, ny - 1);
    geo.rotateX(-Math.PI / 2);
    const colors = new Float32Array(nx * ny * 3);
    geo.setAttribute("color", new THREE.BufferAttribute(colors, 3));
    fieldMesh = new THREE.Mesh(
      geo,
      new THREE.MeshBasicMaterial({ vertexColors: true, side: THREE.DoubleSide })
    );
    fieldMesh.userData = { nx, ny };
    fieldMesh.frustumCulled = false;
    scene.add(fieldMesh);
    return fieldMesh;
  }

  function ensureTexPlane() {
    const THREE = window.THREE;
    if (texPlane) return texPlane;
    const cv = document.createElement("canvas");
    cv.width = 256; cv.height = 256;
    const mat = new THREE.MeshBasicMaterial({
      map: new THREE.CanvasTexture(cv), side: THREE.DoubleSide,
    });
    texPlane = new THREE.Mesh(new THREE.PlaneGeometry(1.4, 1.4), mat);
    texPlane.rotation.x = -Math.PI / 2;
    texPlane.userData.canvas = cv;
    texPlane.frustumCulled = false;
    scene.add(texPlane);
    return texPlane;
  }

  function setVisible(obj, on) {
    if (obj) obj.visible = on;
  }

  function updatePoints(frame, meta) {
    const p = ensurePoints(Math.floor(frame.length / 2));
    const half = (meta.side || 0.5) / 2;
    const pos = p.geometry.attributes.position;
    const n = Math.floor(frame.length / 2);
    for (let i = 0; i < n; i++) {
      pos.setXYZ(i, frame[2 * i] - half, 0, frame[2 * i + 1] - half);
    }
    pos.needsUpdate = true;
  }

  function updateFluidMesh(heights, meta) {
    const m = ensureFieldMesh(meta.out_x, meta.out_y);
    const pos = m.geometry.attributes.position;
    const col = m.geometry.attributes.color;
    const bg = hexToRgb("#0e1e37"), ac = hexToRgb("#4fd1c5");
    let mn = Infinity, mx = -Infinity;
    for (const v of heights) { if (v < mn) mn = v; if (v > mx) mx = v; }
    const span = Math.max(mx - mn, 1e-12);
    const scale = 0.22 / span;
    for (let i = 0; i < heights.length; i++) {
      pos.setY(i, -(heights[i] - mn - span / 2) * scale);
      const t = (heights[i] - mn) / span;
      col.setXYZ(i, lerp(bg.r, ac.r, t) / 255, lerp(bg.g, ac.g, t) / 255, lerp(bg.b, ac.b, t) / 255);
    }
    pos.needsUpdate = true;
    col.needsUpdate = true;
  }

  function updateAcousticPlane(slice, meta) {
    const m = ensureTexPlane();
    const gx = meta.out_x;
    const gy = Math.floor(slice.length / gx);
    const cv = m.userData.canvas;
    cv.width = gx; cv.height = gy; // resize clears; cheap at these sizes
    const ctx = cv.getContext("2d");
    const img = ctx.createImageData(gx, gy);
    const bg = hexToRgb("#0e1e37"), ac = hexToRgb("#f2a65a");
    let mn = Infinity, mx = -Infinity;
    for (const v of slice) { if (v < mn) mn = v; if (v > mx) mx = v; }
    const span = Math.max(mx - mn, 1e-9);
    for (let i = 0; i < slice.length; i++) {
      const t = (slice[i] - mn) / span;
      img.data[i * 4]     = lerp(bg.r, ac.r, t);
      img.data[i * 4 + 1] = lerp(bg.g, ac.g, t);
      img.data[i * 4 + 2] = lerp(bg.b, ac.b, t);
      img.data[i * 4 + 3] = 255;
    }
    ctx.putImageData(img, 0, 0);
    m.material.map.needsUpdate = true;
  }

  function deformDeco(n, m) {
    if (!decoMesh) return;
    const pos = decoMesh.geometry.attributes.position;
    for (let i = 0; i < pos.count; i++) {
      const x = pos.getX(i) / 0.7;
      const y = pos.getZ(i) / 0.7;
      pos.setY(i, chladni(x, y, n, m) * 0.12);
    }
    pos.needsUpdate = true;
  }

  function animate() {
    requestAnimationFrame(animate);
    if (scene) {
      scene.rotation.y += 0.003;
      renderer.render(scene, camera);
    }
  }

  return {
    ready: () => state.ready,
    /* mod/frame/meta = real dataset; n/m = chladni fallback numbers */
    update(mod, frame, meta, nFallback = 3, mFallback = 5) {
      if (!state.ready) return;
      const real = mod && frame && meta;
      setVisible(pointsObj, real && mod === "granular");
      setVisible(fieldMesh, real && mod === "fluid");
      setVisible(texPlane, real && mod === "acoustic");
      setVisible(decoMesh, !real);
      if (!real) { deformDeco(nFallback, mFallback); return; }
      if (mod === "granular") updatePoints(frame, meta);
      else if (mod === "fluid") updateFluidMesh(frame, meta);
      else {
        const plane = meta.out_x * meta.out_y;
        const zMid = meta.out_z >> 1;
        updateAcousticPlane(frame.subarray(zMid * plane, (zMid + 1) * plane), meta);
      }
    },
    show() { if (renderer) renderer.domElement.style.display = "block"; },
    hide() { if (renderer) renderer.domElement.style.display = "none"; },
  };
}
