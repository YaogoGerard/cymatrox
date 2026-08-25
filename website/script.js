/* =================================================================
   CYMATROX SITE — pure replay of REAL crate exports.
   Every pixel shown here comes from website/data/*.json[.gz],
   produced by `cargo run --release --example export_frames`
   against the published cymatrox API. No physics lives in this
   page: no WASM stepping, no JS approximation, no fallback demo.
   ================================================================= */

const MODULES = ["granular", "fluid", "acoustic"];
const ACCENTS = { granular: "#4fd1c5", fluid: "#4fd1c5", acoustic: "#f2a65a" };

/* ---------- shared helpers ---------- */

function lerp(a, b, t) { return a + (b - a) * t; }
function hexToRgb(hex) {
  const v = parseInt(hex.replace("#", ""), 16);
  return { r: (v >> 16) & 255, g: (v >> 8) & 255, b: v & 255 };
}
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
  const data = {};
  MODULES.forEach((m, i) => { data[m] = results[i]; });
  return data;
}

function hasReal(data, m) { return !!(data && data[m] && data[m].frames.length); }

function currentFrameOf(dataset, idx) {
  if (!(dataset.frames[idx] instanceof Float32Array)) {
    dataset.frames[idx] = Float32Array.from(dataset.frames[idx]);
  }
  return dataset.frames[idx];
}

function heatmapCanvas(values, gridW, accentHex) {
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

function midZSlice(frame, meta) {
  const plane = meta.out_x * meta.out_y;
  const zMid = meta.out_z >> 1;
  return frame.subarray(zMid * plane, (zMid + 1) * plane);
}

/* =================================================================
   HERO — animates the REAL fluid height field exported by cymatrox
   (Faraday waves, 60 Hz). Same data file as the simulator panel.
   ================================================================= */
(async function hero() {
  const canvas = document.getElementById("hero-canvas");
  if (!canvas) return;
  const ctx = canvas.getContext("2d");

  const fluid = await loadDataset("fluid");
  if (!fluid) {
    ctx.fillStyle = "#0e1e37";
    ctx.fillRect(0, 0, canvas.width, canvas.height);
    return;
  }

  let i = 0;
  function draw() {
    const f = currentFrameOf(fluid, i % fluid.frames.length);
    const tmp = heatmapCanvas(f, fluid.meta.out_x, "#4fd1c5");
    ctx.imageSmoothingEnabled = true;
    ctx.drawImage(tmp, 0, 0, canvas.width, canvas.height);
    i++;
  }
  draw();
  if (!window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
    setInterval(draw, 1000 / 8);
  }
})();

/* =================================================================
   SIMULATOR PANEL — replay controls over real exported frames,
   in 2D (canvas) and 3D (Three.js views built from the same data).
   ================================================================= */
(async function simulator() {
  const canvas = document.getElementById("sim-canvas");
  if (!canvas) return;

  const statusEl = document.getElementById("data-status");
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
  const liveControls = document.getElementById("live-controls");
  const liveBusyEl = document.getElementById("live-busy");
  const liveNote = document.getElementById("live-note");

  let DATA = null;
  let liveServer = false;
  let currentModule = "granular";
  let view = "2d";
  let three = null;
  let playing = true;
  let frameIdx = 0;
  let lastTs = 0;
  const FRAME_MS = 1000 / 24;

  /* ---------- status ---------- */

  function refreshStatus() {
    const loaded = MODULES.filter((m) => hasReal(DATA, m));
    if (liveServer) {
      statusEl.innerHTML =
        'Serveur cymatrox local <span class="data-banner is-live">connecté — résultats réels à la demande</span>' +
        `<br><span style="font-size:12px">Bougez les curseurs : chaque changement relance un vrai calcul · datasets embarqués : ${MODULES.map((m) => `${m}: ${DATA && DATA[m] ? DATA[m].frames.length : 0} frames`).join(" · ")}</span>`;
    } else if (loaded.length === MODULES.length) {
      statusEl.innerHTML =
        'Données réelles du crate <span class="data-banner is-live">export v0.1.0 — lecture locale</span>' +
        `<br><span style="font-size:12px">${MODULES.map((m) => `${m}: ${DATA[m].frames.length} frames`).join(" · ")} — régénérables via <code class="mono">cargo run --example export_frames</code></span>`;
    } else if (loaded.length) {
      statusEl.innerHTML =
        `Données réelles partielles (${escapeHtml(loaded.join(", "))}) <span class="data-banner is-live">lecture locale</span>`;
    } else {
      statusEl.innerHTML =
        'Aucune donnée chargée ' +
        '<span class="data-banner is-missing">lecture impossible</span>' +
        `<br><span style="font-size:12px">Ce site n'affiche que des sorties réelles du crate : génère les frames avec <code class="mono">cargo run --example export_frames</code>, ou démarre le serveur live <code class="mono">tools/cymatrox-live</code>, puis recharge.</span>`;
    }
  }

  /* ---------- live server (real on-demand GPU results) ---------- */

  async function probeLiveServer() {
    try {
      const res = await fetch("/api/ping", { cache: "no-store" });
      if (!res.ok) return;
      const info = await res.json();
      if (!info.ok || info.crate !== "cymatrox") return;
      liveServer = true;
      liveControls.hidden = false;
      liveNote.textContent =
        `cymatrox v${info.version} — tout changement de curseur ou d'onglet relance un vrai calcul GPU`;
      refreshStatus();
      scheduleLiveRun(0); // compute the current module right away
    } catch { /* no local server — replay-only mode */ }
  }

  function liveQuery() {
    const p = new URLSearchParams();
    const v = currentValues();
    p.set("f", String(v.freq));
    if (currentModule === "granular") {
      p.set("n", String(v.n));
      p.set("m", String(v.m));
    } else {
      p.set("grid", String(v.grid));
    }
    return p.toString();
  }

  let liveBusy = false;
  let liveAbort = null;
  let liveSeq = 0;
  let liveTimer = null;
  const LIVE_DEBOUNCE_MS = 450;

  function setLiveBusy(on) {
    liveBusy = on;
    if (liveBusyEl) liveBusyEl.hidden = !on;
  }

  function scheduleLiveRun(delay = LIVE_DEBOUNCE_MS) {
    if (!liveServer) return;
    clearTimeout(liveTimer);
    liveTimer = setTimeout(runLiveNow, delay);
  }

  async function runLiveNow() {
    if (!liveServer) return;
    const seq = ++liveSeq;
    if (liveAbort) liveAbort.abort(); // supersede any in-flight compute
    const ctrl = new AbortController();
    liveAbort = ctrl;
    setLiveBusy(true);
    try {
      const res = await fetch(`/api/${currentModule}?${liveQuery()}`, {
        cache: "no-store",
        signal: ctrl.signal,
      });
      const payload = await res.json();
      if (!res.ok || payload.error) throw new Error(payload.detail || payload.error || res.status);
      if (seq !== liveSeq) return; // a newer request already took over
      if (!DATA) DATA = {};
      DATA[currentModule] = payload;
      frameIdx = 0;
      playing = true;
      playBtn.textContent = "Pause";
      refreshStatus();
      syncDataControls();
      renderCurrentFrame();
      pushFrameToThree();
    } catch (err) {
      if ((err && err.name === "AbortError") || seq !== liveSeq) return;
      statusEl.innerHTML =
        `<span class="data-banner is-missing">Calcul échoué — ${escapeHtml(String(err))}</span>`;
    } finally {
      if (seq === liveSeq) setLiveBusy(false); // only the latest run owns the indicator
    }
  }

  /* ---------- playback ---------- */

  function syncDataControls() {
    const on = hasReal(DATA, currentModule);
    dataControls.style.display = on ? "" : "none";
    if (!on) return;
    const total = DATA[currentModule].frames.length;
    frameIdx = Math.min(frameIdx, total - 1);
    slider.max = String(total - 1);
    slider.value = String(frameIdx);
    updateFrameLabel();
  }

  function updateFrameLabel() {
    const total = hasReal(DATA, currentModule) ? DATA[currentModule].frames.length : 0;
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

  // Offline convenience: load exported .json/.json.gz manually (file:// usage).
  fileInput.addEventListener("change", async () => {
    if (!DATA) DATA = {};
    for (const file of fileInput.files) {
      const name = MODULES.find((m) => file.name.startsWith(m));
      if (!name) continue;
      try {
        const text = await decodeBody(await file.arrayBuffer(), file.name.endsWith(".gz"));
        const parsed = JSON.parse(text);
        if (parsed && parsed.frames && parsed.frames.length) DATA[name] = parsed;
      } catch (e) { console.warn(`invalid dataset file ${file.name}:`, e); }
    }
    refreshStatus();
    syncDataControls();
    renderCurrentFrame();
    if (three && three.ready()) pushFrameToThree();
  });

  /* ---------- rendering (2D) ---------- */

  function renderNoData() {
    const ctx = canvas.getContext("2d");
    ctx.fillStyle = "#0e1e37";
    ctx.fillRect(0, 0, canvas.width, canvas.height);
    ctx.fillStyle = "#8aa2c0";
    ctx.font = "16px monospace";
    ctx.textAlign = "center";
    ctx.fillText(`aucune donnée réelle pour « ${currentModule} »`, canvas.width / 2, canvas.height / 2 - 12);
    ctx.font = "13px monospace";
    ctx.fillText("cargo run --example export_frames", canvas.width / 2, canvas.height / 2 + 16);
  }

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

  function renderHeightmapFrame(values, gridW, accentHex) {
    const ctx = canvas.getContext("2d");
    const tmp = heatmapCanvas(values, gridW, accentHex);
    ctx.imageSmoothingEnabled = true;
    ctx.drawImage(tmp, 0, 0, canvas.width, canvas.height);
  }

  function renderCurrentFrame() {
    if (!hasReal(DATA, currentModule)) { renderNoData(); return; }
    const f = currentFrameOf(DATA[currentModule], frameIdx);
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
    if (!hasReal(DATA, currentModule)) return;
    three.update(currentModule, currentFrameOf(DATA[currentModule], frameIdx), DATA[currentModule].meta);
  }

  /* ---------- main loop ---------- */

  function tick(ts) {
    requestAnimationFrame(tick);
    const dtms = ts - lastTs;
    lastTs = ts;
    if (!hasReal(DATA, currentModule) || !playing || dtms < FRAME_MS - 2) return;
    frameIdx = (frameIdx + 1) % DATA[currentModule].frames.length;
    slider.value = String(frameIdx);
    updateFrameLabel();
    renderCurrentFrame();
    pushFrameToThree();
  }
  requestAnimationFrame(tick);

  /* ---------- config snippet (copy-paste into your own code) ---------- */

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

  /* ---------- wiring ---------- */

  function onModuleChange(name) {
    currentModule = name;
    tabBtns.forEach((b) => b.classList.toggle("is-active", b.dataset.module === name));
    document.querySelectorAll("[data-module-only]").forEach((el) => {
      el.style.display = el.dataset.moduleOnly.split(" ").includes(name) ? "" : "none";
    });
    playing = true;
    playBtn.textContent = "Pause";
    syncDataControls();
    updateExportCode();
    renderCurrentFrame();
    pushFrameToThree();
    scheduleLiveRun(); // fresh real results for the newly selected module
  }

  tabBtns.forEach((btn) => btn.addEventListener("click", () => onModuleChange(btn.dataset.module)));

  [freqInput, nInput, mInput, gridInput].forEach((el) =>
    el.addEventListener("input", () => {
      document.getElementById("freq-val").textContent = freqInput.value;
      document.getElementById("mode-n-val").textContent = nInput.value;
      document.getElementById("mode-m-val").textContent = mInput.value;
      document.getElementById("grid-size-val").textContent = gridInput.value;
      updateExportCode();
      scheduleLiveRun(); // debounced real recompute with the new parameters
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
        if (!hasReal(DATA, currentModule)) { renderNoData(); return; }
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

  /* ---------- boot ---------- */

  refreshStatus();
  updateExportCode();

  DATA = await loadAllDatasets();
  refreshStatus();
  syncDataControls();
  renderCurrentFrame();
  probeLiveServer();
})();

/* =================================================================
   3D VIEW — built strictly from REAL exported frames:
     granular → THREE.Points cloud on the plate
     fluid    → displaced vertex-coloured mesh (η height field)
     acoustic → CanvasTexture plane of the mid-z pressure slice
   Objects are created lazily per module and reused across frames.
   ================================================================= */
function initThree(container) {
  const script = document.createElement("script");
  const state = { ready: false };
  let renderer, scene, camera;
  let pointsObj = null, fieldMesh = null, texPlane = null;

  script.src = "https://cdnjs.cloudflare.com/ajax/libs/three.js/r128/three.min.js";
  script.onload = () => setup();
  script.onerror = () => console.warn("three.js CDN unreachable — 3D view disabled");
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

    state.ready = true;
    animate();
  }

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

  function animate() {
    requestAnimationFrame(animate);
    if (scene) {
      scene.rotation.y += 0.003;
      renderer.render(scene, camera);
    }
  }

  return {
    ready: () => state.ready,
    update(mod, frame, meta) {
      if (!state.ready || !mod || !frame || !meta) return;
      setVisible(pointsObj, mod === "granular");
      setVisible(fieldMesh, mod === "fluid");
      setVisible(texPlane, mod === "acoustic");
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
