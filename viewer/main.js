const $ = (id) => document.getElementById(id);
const encoder = new TextEncoder();
const decoder = new TextDecoder();

const LOCAL_DETAIL_SCALE = 0.05;
const TRUNK_DETAIL_SCALE = 0.015;
const LAND_COLORS = {
  residential: [120, 140, 128],
  commercial: [150, 160, 190],
  industrial: [150, 128, 104],
};
const HAZARDS = {
  flood: ["#3b9dff", "Alagamento"],
  gas_leak: ["#ffd24a", "Vazamento de gás"],
  live_cable: ["#c9a227", "Cabo exposto"],
  contamination: ["#8a6d3b", "Contaminação"],
  electric_risk: ["#ff4fd8", "Risco elétrico"],
  explosion: ["#ff3b1f", "Explosão"],
};
const BASE_LAYERS = [
  ["terrain", "Terreno"],
  ["streets", "Ruas"],
  ["water", "Rios"],
  ["buildings", "Prédios"],
  ["hazards", "Riscos"],
];

let wasm;
let session = 0;
let city = null;
let snapshot = null;
let selected = null;
let hover = null;
let drag = null;
let view = { scale: 1, x: 0, y: 0 };
let terrainImage = null;
let cityPaths = null;
let drawQueued = false;
let regions = [];
const locals = new Map();
const visible = {};
const canvas = $("map");
const ctx = canvas.getContext("2d");

function takeString(pointer) {
  const memory = new Uint8Array(wasm.memory.buffer);
  let end = pointer;
  while (memory[end]) end++;
  const text = decoder.decode(memory.subarray(pointer, end));
  wasm.city_string_free(pointer);
  return text;
}

function withCString(text, callback) {
  const bytes = encoder.encode(text + "\0");
  const pointer = wasm.city_alloc(bytes.length);
  new Uint8Array(wasm.memory.buffer, pointer, bytes.length).set(bytes);
  try {
    return callback(pointer);
  } finally {
    wasm.city_dealloc(pointer, bytes.length);
  }
}

function checked(pointer) {
  if (!pointer) throw new Error(takeString(wasm.city_last_error()));
  return takeString(pointer);
}

function generate() {
  $("error").textContent = "";
  const region = $("region").value;
  const input = {
    seed: Number($("seed").value),
    population: Number($("population").value),
    region,
    profile: {
      overhead_share: Number($("overhead").value),
      piped_gas: $("gas").checked,
      park_share: Number($("parks").value),
      density: 0.5,
      mean_temperature: Number($("temperature").value),
      annual_rainfall: Number($("rainfall").value),
      bedrock_depth: Number($("bedrock").value),
      seismic_risk: Number($("seismic").value),
      relief: Number($("relief").value),
      block_shape: Number($("shape").value),
      street_irregularity: Number($("irregularity").value),
      rivers: Number($("rivers").value),
      metro_density: Number($("metro").value),
      rail_density: Number($("rail").value),
    },
  };
  const started = performance.now();
  const next = withCString(JSON.stringify(input), (p) => wasm.city_generate(p));
  const elapsed = performance.now() - started;
  if (!next) {
    $("error").textContent = takeString(wasm.city_last_error());
    return;
  }
  if (session) wasm.city_free(session);
  session = next;
  city = JSON.parse(takeString(wasm.city_json(session)));
  snapshot = JSON.parse(checked(wasm.city_snapshot(session)));
  $("hash").textContent = `${region}:${input.seed} · hash ${takeString(wasm.city_hash_hex(session))} · gerada em ${elapsed.toFixed(0)} ms`;
  selected = null;
  locals.clear();
  buildTerrain();
  buildLayers();
  buildCityPaths();
  fit();
}

function buildLayers() {
  const entries = [...BASE_LAYERS, ...city.trunk_specs.map((s) => [s.id, s.label, s.color])];
  $("layers").replaceChildren(
    ...entries.map(([id, label, color]) => {
      visible[id] ??= true;
      const row = document.createElement("label");
      row.className = "check";
      const box = document.createElement("input");
      box.type = "checkbox";
      box.checked = visible[id];
      box.onchange = () => {
        visible[id] = box.checked;
        queueDraw();
      };
      row.append(box);
      if (color) {
        const swatch = document.createElement("span");
        swatch.className = "swatch";
        swatch.style.background = color;
        row.append(swatch);
      }
      row.append(label);
      return row;
    }),
  );
}

function buildTerrain() {
  const { cols, rows, heights } = city.terrain;
  let low = Infinity;
  let high = -Infinity;
  for (const height of heights) {
    low = Math.min(low, height);
    high = Math.max(high, height);
  }
  const span = high - low || 1;
  terrainImage = document.createElement("canvas");
  terrainImage.width = cols;
  terrainImage.height = rows;
  const context = terrainImage.getContext("2d");
  const image = context.createImageData(cols, rows);
  heights.forEach((height, i) => {
    const t = (height - low) / span;
    image.data.set([22 + 44 * t, 32 + 56 * t, 30 + 36 * t, 255], i * 4);
  });
  context.putImageData(image, 0, 0);
}

function polygonPath(path, points) {
  points.forEach((p, i) => (i ? path.lineTo(p.x, p.y) : path.moveTo(p.x, p.y)));
  path.closePath();
}

function segmentPath(path, segment, offset) {
  const dx = segment.pb.x - segment.pa.x;
  const dy = segment.pb.y - segment.pa.y;
  const length = Math.hypot(dx, dy) || 1;
  const shiftX = (-dy / length) * offset;
  const shiftY = (dx / length) * offset;
  path.moveTo(segment.pa.x + shiftX, segment.pa.y + shiftY);
  path.lineTo(segment.pb.x + shiftX, segment.pb.y + shiftY);
}

function networkPaths(networks, specs, broken, nodes) {
  return networks.map((network, index) => {
    const spec = specs[index];
    const brokenSet = new Set(broken[network.id]);
    const paths = { id: network.id, color: spec.color, buried: new Path2D(), overhead: new Path2D(), broken: new Path2D(), pumps: [], sources: [] };
    network.segments.forEach((segment, segmentIndex) => {
      const target = brokenSet.has(segmentIndex) ? paths.broken : segment.overhead ? paths.overhead : paths.buried;
      segmentPath(target, segment, spec.lateral_offset);
      if (segment.street_edge === null && network.sources.includes(segment.a)) paths.sources.push(segment.pa);
    });
    paths.pumps = network.powered_nodes.map((node) => nodes[node].pos);
    return paths;
  });
}

function buildCityPaths() {
  const fills = new Map();
  const lost = new Set(Object.values(snapshot.trunk.outages).flat());
  const lostPath = new Path2D();
  const outlines = new Path2D();
  for (const district of city.districts) {
    polygonPath(outlines, district.polygon);
    if (lost.has(district.id)) polygonPath(lostPath, district.polygon);
    const bucket = Math.round(district.density * 8);
    if (!fills.has(bucket)) fills.set(bucket, new Path2D());
    polygonPath(fills.get(bucket), district.polygon);
  }
  const roads = new Path2D();
  const bridges = new Path2D();
  for (const edge of city.edges) {
    const a = city.nodes[edge.a].pos;
    const b = city.nodes[edge.b].pos;
    const target = edge.bridge ? bridges : roads;
    target.moveTo(a.x, a.y);
    target.lineTo(b.x, b.y);
  }
  const lines = (polylines) => {
    const path = new Path2D();
    for (const line of polylines) {
      line.forEach((p, i) => (i ? path.lineTo(p.x, p.y) : path.moveTo(p.x, p.y)));
    }
    return path;
  };
  cityPaths = {
    fills,
    lostPath,
    outlines,
    roads,
    bridges,
    rivers: lines(city.context.rivers),
    rails: lines(city.context.rails),
    networks: networkPaths(city.networks, city.trunk_specs, snapshot.trunk.broken, city.nodes),
  };
}

function buildLocalPaths(entry, id) {
  const district = entry.data;
  const state = snapshot.districts[id];
  const lost = new Set(Object.values(state.outages).flat());
  const destroyed = new Set(state.destroyed);
  const buildings = new Map();
  const lostPath = new Path2D();
  const destroyedPath = new Path2D();
  district.buildings.forEach((building, index) => {
    if (destroyed.has(index)) {
      polygonPath(destroyedPath, building.footprint);
    } else if (lost.has(index)) {
      polygonPath(lostPath, building.footprint);
    } else {
      const [r, g, b] = LAND_COLORS[building.land_use];
      const lift = Math.round(Math.min(building.floors / 30, 1) * 5) * 14;
      const color = `rgb(${r + lift},${g + lift},${b + lift})`;
      if (!buildings.has(color)) buildings.set(color, new Path2D());
      polygonPath(buildings.get(color), building.footprint);
    }
  });
  const parks = new Path2D();
  const water = new Path2D();
  const railBeds = new Path2D();
  const wetland = new Path2D();
  for (const lot of district.lots) {
    const target = { park: parks, forest: parks, water, wetland, rail: railBeds }[lot.land_use];
    if (target) polygonPath(target, lot.polygon);
  }
  const streets = { arterial: new Path2D(), local: new Path2D(), bridge: new Path2D() };
  for (const edge of district.street_edges) {
    const a = district.street_nodes[edge.a].pos;
    const b = district.street_nodes[edge.b].pos;
    const target = edge.bridge ? streets.bridge : streets[edge.class];
    target.moveTo(a.x, a.y);
    target.lineTo(b.x, b.y);
  }
  entry.paths = { buildings, lostPath, destroyedPath, parks, water, wetland, railBeds, streets, networks: networkPaths(district.networks, district.specs, state.broken, district.street_nodes) };
}

function refreshState() {
  for (const id of Object.keys(snapshot.districts).map(Number)) {
    if (!locals.has(id)) {
      const data = JSON.parse(checked(wasm.city_district_json(session, id)));
      locals.set(id, { data, bounds: boundsOf(city.districts[id].polygon), paths: null });
    }
    buildLocalPaths(locals.get(id), id);
  }
  buildCityPaths();
  queueDraw();
}

function boundsOf(points) {
  return {
    minX: Math.min(...points.map((p) => p.x)),
    maxX: Math.max(...points.map((p) => p.x)),
    minY: Math.min(...points.map((p) => p.y)),
    maxY: Math.max(...points.map((p) => p.y)),
  };
}

function resize() {
  const ratio = window.devicePixelRatio || 1;
  const box = canvas.getBoundingClientRect();
  canvas.width = box.width * ratio;
  canvas.height = box.height * ratio;
  queueDraw();
}

function frame(bounds, margin) {
  const ratio = window.devicePixelRatio || 1;
  const pad = margin * ratio;
  const width = bounds.maxX - bounds.minX;
  const height = bounds.maxY - bounds.minY;
  view.scale = Math.min((canvas.width - pad) / width, (canvas.height - pad) / height);
  view.x = (canvas.width - width * view.scale) / 2 - bounds.minX * view.scale;
  view.y = (canvas.height - height * view.scale) / 2 - bounds.minY * view.scale;
  queueDraw();
}

function fit() {
  resize();
  frame(boundsOf(city.boundary), 80);
}

const sx = (p) => p.x * view.scale + view.x;
const sy = (p) => p.y * view.scale + view.y;

function toWorld(event) {
  const box = canvas.getBoundingClientRect();
  const ratio = canvas.width / box.width;
  return {
    x: ((event.clientX - box.left) * ratio - view.x) / view.scale,
    y: ((event.clientY - box.top) * ratio - view.y) / view.scale,
  };
}

function queueDraw() {
  if (drawQueued) return;
  drawQueued = true;
  requestAnimationFrame(() => {
    drawQueued = false;
    draw();
  });
}

function strokeNetwork(paths, pixelWidth) {
  const px = 1 / view.scale;
  const showIntact = view.scale >= TRUNK_DETAIL_SCALE;
  ctx.setLineDash([]);
  ctx.strokeStyle = "#ff3b1f";
  ctx.lineWidth = (pixelWidth + 1) * px;
  ctx.stroke(paths.broken);
  if (!showIntact) return;
  ctx.lineWidth = pixelWidth * px;
  ctx.strokeStyle = paths.color;
  ctx.stroke(paths.buried);
  ctx.setLineDash([6 * px, 4 * px]);
  ctx.stroke(paths.overhead);
  ctx.setLineDash([]);
  ctx.fillStyle = paths.color;
  for (const p of paths.pumps) ctx.fillRect(p.x - 3 * px, p.y - 3 * px, 6 * px, 6 * px);
  for (const p of paths.sources) {
    ctx.beginPath();
    ctx.arc(p.x, p.y, 6 * px, 0, Math.PI * 2);
    ctx.fill();
  }
}

function drawLocal(entry) {
  const { paths } = entry;
  if (visible.water) {
    ctx.fillStyle = "#1f5f8f";
    ctx.fill(paths.water);
  }
  ctx.fillStyle = "#4a4640";
  ctx.fill(paths.railBeds);
  ctx.fillStyle = "#3f6b63";
  ctx.fill(paths.wetland);
  if (visible.streets) {
    ctx.lineCap = "round";
    ctx.strokeStyle = "#39433f";
    ctx.lineWidth = 24;
    ctx.stroke(paths.streets.arterial);
    ctx.lineWidth = 12;
    ctx.stroke(paths.streets.local);
    ctx.strokeStyle = "#a8a08c";
    ctx.lineWidth = 14;
    ctx.stroke(paths.streets.bridge);
  }
  ctx.fillStyle = "#2f6a45";
  ctx.fill(paths.parks);
  if (visible.buildings) {
    for (const [color, path] of paths.buildings) {
      ctx.fillStyle = color;
      ctx.fill(path);
    }
    ctx.fillStyle = "#d27a46";
    ctx.fill(paths.lostPath);
    ctx.fillStyle = "#1a1a1a";
    ctx.fill(paths.destroyedPath);
    ctx.strokeStyle = "#ff3b1f";
    ctx.lineWidth = 1 / view.scale;
    ctx.stroke(paths.destroyedPath);
  }
  ctx.lineCap = "butt";
  for (const network of paths.networks) {
    if (visible[network.id]) strokeNetwork(network, 1.4);
  }
}

function inViewport(bounds) {
  const left = -view.x / view.scale;
  const top = -view.y / view.scale;
  return bounds.maxX >= left && bounds.minX <= left + canvas.width / view.scale && bounds.maxY >= top && bounds.minY <= top + canvas.height / view.scale;
}

function draw() {
  if (!city) return;
  ctx.setTransform(1, 0, 0, 1, 0, 0);
  ctx.fillStyle = "#0f1413";
  ctx.fillRect(0, 0, canvas.width, canvas.height);
  ctx.setTransform(view.scale, 0, 0, view.scale, view.x, view.y);
  const px = 1 / view.scale;

  if (visible.terrain) {
    const { origin, cell, cols, rows } = city.terrain;
    ctx.drawImage(terrainImage, origin.x, origin.y, (cols - 1) * cell, (rows - 1) * cell);
  }
  for (const [bucket, path] of cityPaths.fills) {
    ctx.fillStyle = `rgba(150,165,190,${0.08 + bucket * 0.07})`;
    ctx.fill(path);
  }
  ctx.fillStyle = "rgba(210,122,70,.65)";
  ctx.fill(cityPaths.lostPath);
  if (visible.water) {
    ctx.strokeStyle = "#1f5f8f";
    ctx.lineWidth = Math.max(2 * px, 250);
    ctx.lineJoin = "round";
    ctx.stroke(cityPaths.rivers);
  }
  if (visible.streets) {
    ctx.strokeStyle = "#566660";
    ctx.lineWidth = Math.max(px, 24);
    ctx.stroke(cityPaths.roads);
    ctx.strokeStyle = "#c7bfa8";
    ctx.lineWidth = Math.max(1.5 * px, 30);
    ctx.stroke(cityPaths.bridges);
  }

  const showDetail = view.scale >= LOCAL_DETAIL_SCALE;
  for (const entry of locals.values()) {
    if (showDetail && inViewport(entry.bounds)) drawLocal(entry);
  }
  ctx.lineCap = "butt";
  for (const network of cityPaths.networks) {
    if (visible[network.id]) strokeNetwork(network, 2.2);
  }
  ctx.strokeStyle = "#e8c468";
  ctx.lineWidth = 1.5 * px;
  for (const id of locals.keys()) {
    const polygon = new Path2D();
    polygonPath(polygon, city.districts[id].polygon);
    ctx.stroke(polygon);
  }

  ctx.setTransform(1, 0, 0, 1, 0, 0);
  if (visible.hazards) drawHazards();
  drawCursor();
  drawSection();
  drawStats();
}

function allHazards() {
  return [snapshot.trunk, ...Object.values(snapshot.districts)].flatMap((state) => state.hazards);
}

function drawHazards() {
  for (const hazard of allHazards()) {
    const size = hazard.kind === "explosion" ? 9 : 5;
    ctx.fillStyle = HAZARDS[hazard.kind][0];
    ctx.strokeStyle = "#000";
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.arc(sx(hazard.at), sy(hazard.at), size, 0, Math.PI * 2);
    ctx.fill();
    ctx.stroke();
  }
}

function drawCursor() {
  if (!hover || mode() !== "destroy") return;
  ctx.strokeStyle = "#ff3b1f";
  ctx.lineWidth = 1.5;
  ctx.beginPath();
  ctx.arc(sx(hover), sy(hover), Number($("radius").value) * view.scale, 0, Math.PI * 2);
  ctx.stroke();
}

function drawSection() {
  const target = $("section");
  const ratio = window.devicePixelRatio || 1;
  const box = target.getBoundingClientRect();
  target.width = box.width * ratio;
  target.height = box.height * ratio;
  const context = target.getContext("2d");
  context.scale(ratio, ratio);
  context.font = "11px system-ui";
  if (!selected) {
    context.fillStyle = "#8b9893";
    context.fillText("Detalhe um distrito e use a ferramenta Corte.", 12, 22);
    return;
  }
  const { data } = locals.get(selected.district);
  const state = snapshot.districts[selected.district];
  const edge = data.street_edges[selected.street];
  const x = (meters) => box.width / 2 + (meters * box.width) / 34;
  const groundY = box.height * 0.5;
  const y = (height) => groundY - height * 11;

  context.fillStyle = "#2b2520";
  context.fillRect(0, groundY, box.width, box.height - groundY);
  context.fillStyle = "#454f4b";
  context.fillRect(x(-edge.width / 2), groundY - 2, x(edge.width / 2) - x(-edge.width / 2), 4);
  context.fillStyle = "#dfe6e2";
  context.fillText(`${edge.class === "arterial" ? "Avenida" : "Rua local"}, ${edge.width} m`, 8, 14);

  data.networks.forEach((network, index) => {
    const spec = data.specs[index];
    if (!visible[spec.id]) return;
    const broken = new Set(state.broken[network.id]);
    network.segments.forEach((segment, segmentIndex) => {
      if (segment.street_edge !== selected.street) return;
      const height = segment.overhead ? spec.overhead_height : -segment.depth;
      const px = x(spec.lateral_offset);
      const py = y(height);
      if (segment.overhead) {
        context.strokeStyle = "#6b5a3d";
        context.beginPath();
        context.moveTo(px, groundY);
        context.lineTo(px, py);
        context.stroke();
      }
      context.fillStyle = broken.has(segmentIndex) ? "#ff3b1f" : spec.color;
      context.beginPath();
      context.arc(px, py, 5, 0, Math.PI * 2);
      context.fill();
      context.fillStyle = "#dfe6e2";
      context.fillText(`${spec.label} ${segment.overhead ? "+" + height : height.toFixed(1)} m`, px + 8, py + 4);
    });
  });
}

function drawStats() {
  const row = (label, value) => {
    const element = document.createElement("div");
    element.className = "stat";
    const name = document.createElement("span");
    name.textContent = label;
    const number = document.createElement("b");
    number.textContent = value.toLocaleString("pt-BR");
    element.append(name, number);
    return element;
  };
  const states = Object.values(snapshot.districts);
  const hazards = allHazards();
  const sum = (values) => values.reduce((a, b) => a + b, 0);
  const rows = [
    row("População", city.population),
    row("Distritos", city.districts.length),
    row("Detalhados", locals.size),
    row("Prédios detalhados", sum([...locals.values()].map((e) => e.data.buildings.length))),
    row("Prédios destruídos", sum(states.map((s) => s.destroyed.length))),
    ...city.trunk_specs.map((spec) => row(`Distritos sem ${spec.label.toLowerCase()}`, snapshot.trunk.outages[spec.id].length)),
    ...city.specs.map((spec) => row(`Prédios sem ${spec.label.toLowerCase()}`, sum(states.map((s) => s.outages[spec.id].length)))),
    ...Object.entries(HAZARDS).map(([kind, [, label]]) => row(label, hazards.filter((h) => h.kind === kind).length)),
  ];
  $("stats").replaceChildren(...rows);
}

const mode = () => document.querySelector("input[name=mode]:checked").value;

function apply(event) {
  snapshot = JSON.parse(checked(withCString(JSON.stringify(event), (p) => wasm.city_apply_event(session, p))));
  refreshState();
}

function districtAt(at) {
  return city.districts.find((district) =>
    district.polygon.every((a, i) => {
      const b = district.polygon[(i + 1) % district.polygon.length];
      return (b.x - a.x) * (at.y - a.y) - (b.y - a.y) * (at.x - a.x) >= 0;
    }),
  );
}

function distanceToSegment(p, a, b) {
  const dx = b.x - a.x;
  const dy = b.y - a.y;
  const t = Math.max(0, Math.min(1, ((p.x - a.x) * dx + (p.y - a.y) * dy) / (dx * dx + dy * dy || 1)));
  return Math.hypot(p.x - a.x - dx * t, p.y - a.y - dy * t);
}

function nearestStreet(data, at) {
  let best = null;
  let bestDistance = Infinity;
  data.street_edges.forEach((edge, index) => {
    const distance = distanceToSegment(at, data.street_nodes[edge.a].pos, data.street_nodes[edge.b].pos);
    if (distance < bestDistance) {
      bestDistance = distance;
      best = index;
    }
  });
  return best;
}

function nearestSegment(networks, at, limit) {
  let best = null;
  let bestDistance = limit;
  for (const network of networks) {
    if (!visible[network.id]) continue;
    network.segments.forEach((segment, index) => {
      const distance = distanceToSegment(at, segment.pa, segment.pb);
      if (distance < bestDistance) {
        bestDistance = distance;
        best = { network: network.id, segment: index };
      }
    });
  }
  return best;
}

function detail(district) {
  const data = JSON.parse(checked(wasm.city_district_json(session, district.id)));
  snapshot = JSON.parse(checked(wasm.city_snapshot(session)));
  locals.set(district.id, { data, bounds: boundsOf(district.polygon), paths: null });
  refreshState();
  frame(boundsOf(district.polygon), 80);
}

function handleClick(at) {
  const district = districtAt(at);
  const tool = mode();
  if (tool === "destroy") {
    apply({ kind: "destroy", center: at, radius: Number($("radius").value), max_depth: Number($("depth").value) });
  } else if (tool === "detail") {
    if (district) detail(district);
  } else if (tool === "cut") {
    const limit = 12 / view.scale;
    const entry = district && view.scale >= LOCAL_DETAIL_SCALE ? locals.get(district.id) : null;
    const local = entry ? nearestSegment(entry.data.networks, at, limit) : null;
    if (local) {
      apply({ kind: "cut_local", district: district.id, ...local });
    } else {
      const trunk = nearestSegment(city.networks, at, limit);
      if (trunk) apply({ kind: "cut_trunk", ...trunk });
    }
  } else if (district && locals.has(district.id)) {
    selected = { district: district.id, street: nearestStreet(locals.get(district.id).data, at) };
    queueDraw();
  }
}

canvas.addEventListener("mousedown", (event) => {
  drag = { x: event.clientX, y: event.clientY, startX: event.clientX, startY: event.clientY, moved: false };
});
window.addEventListener("mouseup", (event) => {
  if (drag && !drag.moved && event.target === canvas && city) handleClick(toWorld(event));
  drag = null;
});
canvas.addEventListener("mousemove", (event) => {
  if (drag && Math.hypot(event.clientX - drag.startX, event.clientY - drag.startY) > 4) drag.moved = true;
  if (drag?.moved) {
    const ratio = canvas.width / canvas.getBoundingClientRect().width;
    view.x += (event.clientX - drag.x) * ratio;
    view.y += (event.clientY - drag.y) * ratio;
    drag.x = event.clientX;
    drag.y = event.clientY;
  }
  hover = toWorld(event);
  queueDraw();
});
canvas.addEventListener(
  "wheel",
  (event) => {
    event.preventDefault();
    const at = toWorld(event);
    view.scale *= event.deltaY < 0 ? 1.2 : 1 / 1.2;
    const box = canvas.getBoundingClientRect();
    const ratio = canvas.width / box.width;
    view.x = (event.clientX - box.left) * ratio - at.x * view.scale;
    view.y = (event.clientY - box.top) * ratio - at.y * view.scale;
    queueDraw();
  },
  { passive: false },
);
function applyRegion() {
  const { profile } = regions.find((r) => r.id === $("region").value);
  $("overhead").value = profile.overhead_share;
  $("parks").value = profile.park_share;
  $("gas").checked = profile.piped_gas;
  $("temperature").value = profile.mean_temperature;
  $("rainfall").value = profile.annual_rainfall;
  $("bedrock").value = profile.bedrock_depth;
  $("seismic").value = profile.seismic_risk;
  $("relief").value = profile.relief;
  $("shape").value = profile.block_shape;
  $("irregularity").value = profile.street_irregularity;
  $("rivers").value = profile.rivers;
  $("metro").value = profile.metro_density;
  $("rail").value = profile.rail_density;
}

$("region").onchange = () => {
  applyRegion();
  generate();
};
$("preset").onchange = () => {
  $("population").value = $("preset").value;
  generate();
};
$("generate").onclick = generate;
$("random").onclick = () => {
  $("seed").value = Math.floor(Math.random() * 1e9);
  generate();
};
$("reset").onclick = () => {
  snapshot = JSON.parse(checked(wasm.city_reset(session)));
  refreshState();
};
$("fit").onclick = fit;
window.addEventListener("resize", resize);

(async () => {
  const bytes = await (await fetch("city.wasm")).arrayBuffer();
  wasm = (await WebAssembly.instantiate(bytes, {})).instance.exports;
  regions = JSON.parse(takeString(wasm.city_regions()));
  $("region").replaceChildren(
    ...regions.map((region) => {
      const option = document.createElement("option");
      option.value = region.id;
      option.textContent = region.label;
      return option;
    }),
  );
  applyRegion();
  generate();
  window.cityReady = true;
})();
