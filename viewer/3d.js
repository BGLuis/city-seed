const $ = (id) => document.getElementById(id);

const VERTEX_BYTES = 28;
const FIELD_OF_VIEW = (50 * Math.PI) / 180;
const DISTRICT_AREA = 3.5e6;
const DETAIL_RANGE_MARGIN = 1.4;
const EVICTION_MARGIN = 1.25;
const MAX_REQUESTS_IN_FLIGHT = 2;
const STREAM_REFRESH_FRAMES = 6;
const MIN_NEAR = 1.5;
const NEAR_PER_HEIGHT = 0.15;
const SKY = [0.62, 0.74, 0.86];
const UNDERGROUND_SKY = [0.07, 0.08, 0.09];
const COVERING_LAYERS = ["ground", "streets", "buildings", "rivers"];
const SURFACE_DEFAULTS = { pitch: 0.55, yaw: -1.0, distance: 2_500 };

const VERTEX_SHADER = `#version 300 es
layout(location=0) in vec3 position;
layout(location=1) in vec3 normal;
layout(location=2) in vec4 color;
uniform mat4 viewProjection;
uniform vec3 offset;
out vec3 surfaceColor;
out float distanceToEye;
void main() {
  vec3 relative = position + offset;
  float sun = max(dot(normal, normalize(vec3(0.45, 0.3, 0.84))), 0.0);
  float sky = 0.5 + 0.5 * normal.z;
  surfaceColor = color.rgb * (0.32 * sky + 0.25 + 0.62 * sun);
  distanceToEye = length(relative);
  gl_Position = viewProjection * vec4(relative, 1.0);
}`;

const FRAGMENT_SHADER = `#version 300 es
precision highp float;
in vec3 surfaceColor;
in float distanceToEye;
uniform vec3 fogColor;
uniform float fogDensity;
out vec4 result;
void main() {
  float amount = 1.0 / (1.0 + pow(distanceToEye * fogDensity, 2.0));
  result = vec4(mix(fogColor, surfaceColor, amount), 1.0);
}`;

const canvas = $("view");
const gl = canvas.getContext("webgl2", { antialias: true, powerPreference: "high-performance" });

const state = {
  token: 0,
  macro: null,
  tileCenters: [],
  detailed: new Map(),
  requested: new Set(),
  hidden: new Set(),
  view: { x: 0, y: 0, z: 0, ...SURFACE_DEFAULTS },
  maxDetailed: 24,
  showSurface: true,
  frame: 0,
  lastFrameAt: 0,
  frameMs: 0,
  rendered: { triangles: 0, draws: 0 },
  gpuBytes: 0,
  buildTimes: [],
  uploadTimes: [],
  bench: null,
};

let program;
let uniforms;
let regionsById = new Map();

const syncPixel = new Uint8Array(4);
const worker = new Worker("3d-worker.js");

function compile(type, source) {
  const shader = gl.createShader(type);
  gl.shaderSource(shader, source);
  gl.compileShader(shader);
  if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(shader));
  return shader;
}

function setupGl() {
  program = gl.createProgram();
  gl.attachShader(program, compile(gl.VERTEX_SHADER, VERTEX_SHADER));
  gl.attachShader(program, compile(gl.FRAGMENT_SHADER, FRAGMENT_SHADER));
  gl.linkProgram(program);
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(program));
  uniforms = Object.fromEntries(
    ["viewProjection", "offset", "fogColor", "fogDensity"].map((name) => [name, gl.getUniformLocation(program, name)]),
  );
  gl.useProgram(program);
  gl.enable(gl.DEPTH_TEST);
  gl.enable(gl.CULL_FACE);
}

function uploadLayer(layer) {
  const vao = gl.createVertexArray();
  gl.bindVertexArray(vao);
  const vertexBuffer = gl.createBuffer();
  gl.bindBuffer(gl.ARRAY_BUFFER, vertexBuffer);
  gl.bufferData(gl.ARRAY_BUFFER, layer.vertices, gl.STATIC_DRAW);
  gl.enableVertexAttribArray(0);
  gl.vertexAttribPointer(0, 3, gl.FLOAT, false, VERTEX_BYTES, 0);
  gl.enableVertexAttribArray(1);
  gl.vertexAttribPointer(1, 3, gl.FLOAT, false, VERTEX_BYTES, 12);
  gl.enableVertexAttribArray(2);
  gl.vertexAttribPointer(2, 4, gl.UNSIGNED_BYTE, true, VERTEX_BYTES, 24);
  const indexBuffer = gl.createBuffer();
  gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, indexBuffer);
  gl.bufferData(gl.ELEMENT_ARRAY_BUFFER, layer.indices, gl.STATIC_DRAW);
  gl.bindVertexArray(null);
  const bytes = layer.vertices.byteLength + layer.indices.byteLength;
  state.gpuBytes += bytes;
  return {
    id: layer.id,
    label: layer.label,
    underground: layer.underground,
    indexCount: layer.index_count,
    bounds: layer.bounds,
    tiles: layer.tiles,
    vao,
    vertexBuffer,
    indexBuffer,
    bytes,
  };
}

function uploadMesh(mesh) {
  return { origin: mesh.origin, hash: mesh.hash, layers: mesh.layers.filter((l) => l.index_count).map(uploadLayer) };
}

function releaseMesh(mesh) {
  for (const layer of mesh.layers) {
    gl.deleteVertexArray(layer.vao);
    gl.deleteBuffer(layer.vertexBuffer);
    gl.deleteBuffer(layer.indexBuffer);
    state.gpuBytes -= layer.bytes;
  }
}

function eyePosition(view) {
  const flat = Math.cos(view.pitch) * view.distance;
  return [
    view.x - Math.cos(view.yaw) * flat,
    view.y - Math.sin(view.yaw) * flat,
    view.z + Math.sin(view.pitch) * view.distance,
  ];
}

function normalize([x, y, z]) {
  const length = Math.hypot(x, y, z);
  return [x / length, y / length, z / length];
}

function cross(a, b) {
  return [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
}

function dot(a, b) {
  return a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
}

// The eye stays at the origin: chunks are offset by double-precision JS numbers,
// so the f32 matrices never see large coordinates.
function viewProjection(view, eye, aspect) {
  const forward = normalize([view.x - eye[0], view.y - eye[1], view.z - eye[2]]);
  const side = normalize(cross(forward, [0, 0, 1]));
  const up = cross(side, forward);
  const height = Math.max(eye[2] - view.z, 1);
  const near = Math.max(MIN_NEAR, Math.min(height, view.distance) * NEAR_PER_HEIGHT);
  const far = Math.max(cityRadius() * 6, view.distance * 10, near * 100);
  const f = 1 / Math.tan(FIELD_OF_VIEW / 2);
  const depth = 1 / (near - far);
  const projection = [f / aspect, f, (far + near) * depth, 2 * far * near * depth];
  return new Float32Array([
    side[0] * projection[0], up[0] * projection[1], -forward[0] * projection[2], forward[0],
    side[1] * projection[0], up[1] * projection[1], -forward[1] * projection[2], forward[1],
    side[2] * projection[0], up[2] * projection[1], -forward[2] * projection[2], forward[2],
    0, 0, projection[3], 0,
  ]);
}

function frustumPlanes(m) {
  const row = (i) => [m[i], m[i + 4], m[i + 8], m[i + 12]];
  const [a, b, c, d] = [row(0), row(1), row(2), row(3)];
  return [
    d.map((v, i) => v + a[i]),
    d.map((v, i) => v - a[i]),
    d.map((v, i) => v + b[i]),
    d.map((v, i) => v - b[i]),
    d.map((v, i) => v + c[i]),
    d.map((v, i) => v - c[i]),
  ];
}

function boxVisible(planes, bounds, offset) {
  for (const plane of planes) {
    const corner = [0, 1, 2].map((axis) => (plane[axis] >= 0 ? bounds.max[axis] : bounds.min[axis]) + offset[axis]);
    if (dot(plane, corner) + plane[3] < 0) return false;
  }
  return true;
}

function cityRadius() {
  const ground = state.macro?.layers.find((layer) => layer.id === "ground");
  return ground ? Math.max(ground.bounds.max[0], ground.bounds.max[1], 1_000) : 1_000;
}

function isLayerVisible(layer) {
  if (state.hidden.has(layer.id)) return false;
  return !(layer.underground && state.showSurface && !state.hidden.has("ground"));
}

function drawLayer(layer, planes, offset, counters) {
  if (!isLayerVisible(layer) || !boxVisible(planes, layer.bounds, offset)) return;
  gl.bindVertexArray(layer.vao);
  gl.uniform3f(uniforms.offset, offset[0], offset[1], offset[2]);
  if (!layer.tiles.length) {
    gl.drawElements(gl.TRIANGLES, layer.indexCount, gl.UNSIGNED_INT, 0);
    counters.triangles += layer.indexCount / 3;
    counters.draws += 1;
    return;
  }
  let first = -1;
  let count = 0;
  const flush = () => {
    if (!count) return;
    gl.drawElements(gl.TRIANGLES, count, gl.UNSIGNED_INT, first * 4);
    counters.triangles += count / 3;
    counters.draws += 1;
    count = 0;
  };
  for (const tile of layer.tiles) {
    if (state.detailed.has(tile.district) || !boxVisible(planes, tile.bounds, offset)) {
      flush();
      continue;
    }
    if (!count) first = tile.first_index;
    count += tile.index_count;
  }
  flush();
}

function resize() {
  const ratio = Math.min(window.devicePixelRatio || 1, 2);
  const width = Math.max(1, Math.floor(canvas.clientWidth * ratio));
  const height = Math.max(1, Math.floor(canvas.clientHeight * ratio));
  if (canvas.width !== width || canvas.height !== height) {
    canvas.width = width;
    canvas.height = height;
  }
  gl.viewport(0, 0, width, height);
}

function render() {
  resize();
  const counters = { triangles: 0, draws: 0 };
  const eye = eyePosition(state.view);
  const seeUnderground = !state.showSurface || state.hidden.has("ground");
  const fog = seeUnderground ? UNDERGROUND_SKY : SKY;
  gl.clearColor(fog[0], fog[1], fog[2], 1);
  gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT);
  if (state.macro) {
    const matrix = viewProjection(state.view, eye, canvas.width / canvas.height);
    const planes = frustumPlanes(matrix);
    gl.uniformMatrix4fv(uniforms.viewProjection, false, matrix);
    gl.uniform3f(uniforms.fogColor, fog[0], fog[1], fog[2]);
    gl.uniform1f(uniforms.fogDensity, 1 / (cityRadius() * 8));
    const offsetOf = (mesh) => [mesh.origin[0] - eye[0], mesh.origin[1] - eye[1], -eye[2]];
    const macroOffset = offsetOf(state.macro);
    for (const layer of state.macro.layers) drawLayer(layer, planes, macroOffset, counters);
    for (const mesh of state.detailed.values()) {
      const offset = offsetOf(mesh);
      for (const layer of mesh.layers) drawLayer(layer, planes, offset, counters);
    }
    gl.bindVertexArray(null);
  }
  state.rendered = counters;
}

// Wide enough to hold the requested number of districts around the camera.
function detailRange() {
  return Math.sqrt((state.maxDetailed * DISTRICT_AREA) / Math.PI) * DETAIL_RANGE_MARGIN;
}

function updateStreaming() {
  if (!state.macro) return;
  const range = detailRange();
  const eye = eyePosition(state.view);
  const nearby = state.tileCenters
    .map((tile) => ({ id: tile.id, distance: Math.hypot(tile.x - eye[0], tile.y - eye[1], tile.z - eye[2]) }))
    .filter((tile) => tile.distance < range * EVICTION_MARGIN)
    .sort((a, b) => a.distance - b.distance);
  const wanted = nearby.filter((tile) => tile.distance < range).slice(0, state.maxDetailed);
  const keep = new Set(wanted.map((tile) => tile.id));
  for (const tile of nearby.slice(0, state.maxDetailed)) keep.add(tile.id);

  for (const [id, mesh] of state.detailed) {
    if (!keep.has(id)) {
      releaseMesh(mesh);
      state.detailed.delete(id);
    }
  }
  for (const tile of wanted) {
    if (state.requested.size >= MAX_REQUESTS_IN_FLIGHT) break;
    if (!state.detailed.has(tile.id) && !state.requested.has(tile.id)) {
      state.requested.add(tile.id);
      worker.postMessage({ kind: "district", id: tile.id, token: state.token });
    }
  }
  state.pendingDetail = wanted.some((tile) => !state.detailed.has(tile.id));
}

function statRow(label, value) {
  const row = document.createElement("div");
  row.className = "stat";
  const name = document.createElement("span");
  name.textContent = label;
  const amount = document.createElement("b");
  amount.textContent = value;
  row.append(name, amount);
  return row;
}

function formatNumber(value, digits = 0) {
  return value.toLocaleString("pt-BR", { maximumFractionDigits: digits, minimumFractionDigits: digits });
}

function refreshStats() {
  const fps = state.frameMs ? 1000 / state.frameMs : 0;
  $("stats").replaceChildren(
    statRow("Quadros por segundo", formatNumber(fps)),
    statRow("Triângulos", formatNumber(state.rendered.triangles)),
    statRow("Chamadas de desenho", formatNumber(state.rendered.draws)),
    statRow("Distritos em detalhe", formatNumber(state.detailed.size)),
    statRow("Memória de vídeo", `${formatNumber(state.gpuBytes / 1e6)} MB`),
  );
}

function frame(now) {
  const started = performance.now();
  if (state.lastFrameAt) state.frameMs = state.frameMs * 0.9 + (now - state.lastFrameAt) * 0.1;
  const interval = state.lastFrameAt ? now - state.lastFrameAt : 0;
  state.lastFrameAt = now;
  state.bench?.beforeFrame();
  if (state.frame % STREAM_REFRESH_FRAMES === 0 || state.bench) updateStreaming();
  render();
  const cpu = performance.now() - started;
  if (state.bench) {
    // Without this the browser only queues the work and frames look free.
    gl.readPixels(0, 0, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, syncPixel);
    state.bench.afterFrame(interval, cpu, performance.now() - started);
  }
  if (state.frame % 20 === 0) refreshStats();
  state.frame += 1;
  requestAnimationFrame(frame);
}

function resetCity() {
  for (const mesh of state.detailed.values()) releaseMesh(mesh);
  state.detailed.clear();
  state.requested.clear();
  if (state.macro) releaseMesh(state.macro);
  state.macro = null;
  state.tileCenters = [];
}

function renderLayerControls() {
  const seen = new Map();
  const add = (layer) => seen.set(layer.id, layer.label);
  state.macro.layers.forEach(add);
  add({ id: "buildings", label: "Prédios" });
  $("layers").innerHTML = "";
  for (const [id, label] of seen) {
    const row = document.createElement("label");
    row.className = "check";
    const box = document.createElement("input");
    box.type = "checkbox";
    box.checked = !state.hidden.has(id);
    box.dataset.layer = id;
    box.addEventListener("change", () => {
      if (box.checked) state.hidden.delete(id);
      else state.hidden.add(id);
    });
    row.append(box, label);
    $("layers").append(row);
  }
}

function syncLayerControls() {
  for (const box of $("layers").querySelectorAll("input")) box.checked = !state.hidden.has(box.dataset.layer);
}

const onCityReady = [];
const urlParams = new URLSearchParams(location.search);
let urlViewPending = true;

function applyUrlView() {
  if (!urlViewPending) return;
  urlViewPending = false;
  if (urlParams.get("view") === "underground") $("underground").click();
  for (const key of ["distance", "pitch", "yaw"]) if (urlParams.has(key)) state.view[key] = Number(urlParams.get(key));
}

const messages = {
  ready({ regions }) {
    regionsById = new Map(regions.map((region) => [region.id, region]));
    for (const region of regions) $("region").add(new Option(`${region.label} (${region.id})`, region.id));
    $("region").value = "JP-KT";
    for (const key of ["seed", "population", "region"]) if (urlParams.has(key)) $(key).value = urlParams.get(key);
    start();
  },
  city(message) {
    if (message.token !== state.token) return;
    resetCity();
    state.macro = uploadMesh(message.mesh);
    const ground = state.macro.layers.find((layer) => layer.id === "ground");
    const middle = (tile, axis) => (tile.bounds.min[axis] + tile.bounds.max[axis]) / 2;
    state.tileCenters = ground.tiles.map((tile) => ({
      id: tile.district,
      x: state.macro.origin[0] + middle(tile, 0),
      y: state.macro.origin[1] + middle(tile, 1),
      z: middle(tile, 2),
    }));
    state.view = { ...state.view, x: state.macro.origin[0], y: state.macro.origin[1], ...SURFACE_DEFAULTS };
    state.view.z = groundHeightAt(state.view.x, state.view.y);
    $("loading").hidden = true;
    $("hash").textContent = `cidade ${message.cityHash} · malha ${message.mesh.hash}`;
    renderLayerControls();
    applyUrlView();
    onCityReady.splice(0).forEach((resolve) => resolve(message));
  },
  district(message) {
    if (message.token !== state.token) return;
    state.requested.delete(message.id);
    const started = performance.now();
    state.detailed.set(message.id, uploadMesh(message.mesh));
    state.uploadTimes.push(performance.now() - started);
    state.buildTimes.push(message.buildMs);
  },
  error(message) {
    $("error").textContent = message.message;
    $("loading").hidden = true;
    state.bench?.fail(message.message);
  },
};

function groundHeightAt(x, y) {
  let best = 0;
  let bestDistance = Infinity;
  for (const tile of state.tileCenters) {
    const distance = Math.hypot(tile.x - x, tile.y - y);
    if (distance < bestDistance) {
      bestDistance = distance;
      best = tile.z;
    }
  }
  return best;
}

worker.onmessage = ({ data }) => messages[data.kind](data);

function cityInput() {
  const region = $("region").value;
  return {
    seed: Number($("seed").value),
    population: Number($("population").value),
    region,
    profile: regionsById.get(region).profile,
  };
}

function start() {
  $("loading").hidden = false;
  $("error").textContent = "";
  state.token += 1;
  state.requested.clear();
  worker.postMessage({ kind: "generate", input: cityInput(), token: state.token });
  return new Promise((resolve) => onCityReady.push(resolve));
}

function setupControls() {
  $("generate").addEventListener("click", start);
  $("detail").addEventListener("input", () => {
    state.maxDetailed = Number($("detail").value);
  });
  const preset = (showSurface, changes) => {
    state.showSurface = showSurface;
    for (const layer of COVERING_LAYERS) {
      if (showSurface) state.hidden.delete(layer);
      else state.hidden.add(layer);
    }
    Object.assign(state.view, changes);
    syncLayerControls();
  };
  $("surface").addEventListener("click", () => preset(true, {}));
  $("underground").addEventListener("click", () => preset(false, { distance: 500, pitch: 0.5 }));
  $("overview").addEventListener("click", () => {
    Object.assign(state.view, { distance: cityRadius() * 2.4, pitch: 0.9 });
  });
  $("benchmark").addEventListener("click", () => runBenchmark());

  let drag = null;
  canvas.addEventListener("contextmenu", (event) => event.preventDefault());
  canvas.addEventListener("pointerdown", (event) => {
    canvas.setPointerCapture(event.pointerId);
    drag = { x: event.clientX, y: event.clientY, pan: event.shiftKey || event.button === 2 };
  });
  canvas.addEventListener("pointerup", () => {
    drag = null;
  });
  canvas.addEventListener("pointermove", (event) => {
    if (!drag) return;
    const dx = event.clientX - drag.x;
    const dy = event.clientY - drag.y;
    drag.x = event.clientX;
    drag.y = event.clientY;
    const view = state.view;
    if (drag.pan) {
      const scale = (view.distance * 2 * Math.tan(FIELD_OF_VIEW / 2)) / canvas.clientHeight;
      const [cos, sin] = [Math.cos(view.yaw), Math.sin(view.yaw)];
      view.x += (-dx * sin + dy * cos) * scale;
      view.y += (dx * cos + dy * sin) * scale;
      view.z = groundHeightAt(view.x, view.y);
    } else {
      view.yaw -= dx * 0.005;
      view.pitch = Math.min(1.5, Math.max(0.05, view.pitch + dy * 0.005));
    }
  });
  canvas.addEventListener("wheel", (event) => {
    event.preventDefault();
    const view = state.view;
    view.distance = Math.min(cityRadius() * 4, Math.max(30, view.distance * 1.0015 ** event.deltaY));
  }, { passive: false });
}

function percentile(values, fraction) {
  if (!values.length) return 0;
  const sorted = [...values].sort((a, b) => a - b);
  return sorted[Math.min(sorted.length - 1, Math.floor(sorted.length * fraction))];
}

function rendererName() {
  const extension = gl.getExtension("WEBGL_debug_renderer_info");
  return extension ? gl.getParameter(extension.UNMASKED_RENDERER_WEBGL) : gl.getParameter(gl.RENDERER);
}

function summarize(samples) {
  const percentiles = (key) => {
    const values = samples.map((sample) => sample[key]);
    return { p50: percentile(values, 0.5), p95: percentile(values, 0.95), p99: percentile(values, 0.99), max: Math.max(...values) };
  };
  return { frames: samples.length, intervalMs: percentiles("interval"), cpuMs: percentiles("cpu"), renderedMs: percentiles("rendered") };
}

async function runBenchmark(options = {}) {
  const frames = options.frames ?? 600;
  $("benchmark").disabled = true;
  $("bench").textContent = "Medindo…";
  state.buildTimes = [];
  state.uploadTimes = [];
  state.hidden.clear();
  state.showSurface = true;
  state.maxDetailed = options.detail ?? 24;
  $("detail").value = state.maxDetailed;

  const city = await start();
  syncLayerControls();
  const center = { x: state.view.x, y: state.view.y };
  const orbit = Math.min(cityRadius() * 0.25, 4_000);
  const pose = (progress) => {
    const angle = progress * Math.PI * 2;
    const x = center.x + Math.cos(angle) * orbit;
    const y = center.y + Math.sin(angle) * orbit;
    Object.assign(state.view, { x, y, z: groundHeightAt(x, y), yaw: angle + Math.PI / 2, pitch: 0.42, distance: 1_100 });
  };

  const loadStarted = performance.now();
  pose(0);
  const report = await new Promise((resolve, reject) => {
    let phase = "loading";
    let index = 0;
    let loadMs = 0;
    const samples = [];
    let peakTriangles = 0;
    let peakDraws = 0;
    let peakBytes = 0;
    state.bench = {
      fail: reject,
      beforeFrame() {
        if (phase === "flying") pose(index / frames);
      },
      afterFrame(interval, cpu, rendered) {
        if (phase === "loading") {
          if (state.frame > 2 && !state.pendingDetail && !state.requested.size && state.detailed.size) {
            loadMs = performance.now() - loadStarted;
            phase = "flying";
          }
          return;
        }
        samples.push({ interval, cpu, rendered });
        peakTriangles = Math.max(peakTriangles, state.rendered.triangles);
        peakDraws = Math.max(peakDraws, state.rendered.draws);
        peakBytes = Math.max(peakBytes, state.gpuBytes);
        index += 1;
        if (index >= frames) {
          state.bench = null;
          resolve({
            renderer: rendererName(),
            canvas: [canvas.width, canvas.height],
            cityGenerateMs: city.generateMs,
            cityMeshMs: city.meshMs,
            firstViewLoadMs: loadMs,
            districtsDetailed: state.detailed.size,
            ...summarize(samples.slice(1)),
            districtBuildMs: { p50: percentile(state.buildTimes, 0.5), p95: percentile(state.buildTimes, 0.95), max: Math.max(0, ...state.buildTimes), count: state.buildTimes.length },
            uploadMs: { p50: percentile(state.uploadTimes, 0.5), p95: percentile(state.uploadTimes, 0.95), max: Math.max(0, ...state.uploadTimes) },
            peakTriangles,
            peakDraws,
            peakGpuMegabytes: peakBytes / 1e6,
            cityHash: $("hash").textContent,
          });
        }
      },
    };
  }).catch((error) => ({ failed: String(error) }));

  $("benchmark").disabled = false;
  if (report.failed) {
    $("bench").textContent = report.failed;
  } else {
    $("bench").replaceChildren(
      statRow("Quadro p50 / p95", `${formatNumber(report.renderedMs.p50, 1)} / ${formatNumber(report.renderedMs.p95, 1)} ms`),
      statRow("CPU p95", `${formatNumber(report.cpuMs.p95, 2)} ms`),
      statRow("Distrito p50", `${formatNumber(report.districtBuildMs.p50, 1)} ms`),
      statRow("Primeira vista", `${formatNumber(report.firstViewLoadMs)} ms`),
      statRow("Pico de triângulos", formatNumber(report.peakTriangles)),
      statRow("Placa", report.renderer),
    );
  }
  window.benchmarkReport = report;
  return report;
}

async function main() {
  if (!gl) {
    $("loading").textContent = "Este navegador não tem WebGL2.";
    return;
  }
  setupGl();
  setupControls();
  requestAnimationFrame(frame);

  if (urlParams.has("bench")) {
    await new Promise((resolve) => {
      const wait = () => (regionsById.size ? resolve() : setTimeout(wait, 20));
      wait();
    });
    const report = await runBenchmark({ frames: Number(urlParams.get("frames") ?? 600), detail: Number(urlParams.get("detail") ?? 24) });
    if (urlParams.has("report")) {
      await fetch("/bench-result", { method: "POST", body: JSON.stringify(report) });
    }
  }
}

main().catch((error) => {
  $("error").textContent = String(error);
  $("loading").hidden = true;
});
