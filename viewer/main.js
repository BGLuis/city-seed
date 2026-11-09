const $ = (id) => document.getElementById(id);
const encoder = new TextEncoder();
const decoder = new TextDecoder();

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
  ["lots", "Lotes"],
  ["buildings", "Prédios"],
  ["hazards", "Riscos"],
];

let wasm;
let session = 0;
let district = null;
let snapshot = null;
let selectedStreet = null;
let hover = null;
let view = { scale: 1, x: 0, y: 0 };
let terrainImage = null;
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
  const size = Number($("size").value);
  const input = {
    seed: Number($("seed").value),
    boundary: [{ x: 0, y: 0 }, { x: size, y: 0 }, { x: size, y: size }, { x: 0, y: size }],
    profile: {
      overhead_share: Number($("overhead").value),
      piped_gas: $("gas").checked,
      park_share: Number($("parks").value),
      density: Number($("density").value),
    },
  };
  const next = withCString(JSON.stringify(input), (p) => wasm.city_generate(p));
  if (!next) {
    $("error").textContent = takeString(wasm.city_last_error());
    return;
  }
  if (session) wasm.city_free(session);
  session = next;
  district = JSON.parse(takeString(wasm.city_district_json(session)));
  snapshot = JSON.parse(checked(wasm.city_reset(session)));
  $("hash").textContent = "hash " + takeString(wasm.city_hash(session));
  selectedStreet = null;
  buildTerrain();
  buildLayers();
  resize();
}

function buildLayers() {
  const entries = [...BASE_LAYERS, ...district.specs.map((s) => [s.id, s.label, s.color])];
  $("layers").replaceChildren(
    ...entries.map(([id, label, color]) => {
      visible[id] ??= id !== "lots";
      const row = document.createElement("label");
      row.className = "check";
      const box = document.createElement("input");
      box.type = "checkbox";
      box.checked = visible[id];
      box.onchange = () => {
        visible[id] = box.checked;
        draw();
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
  const { cols, rows, heights } = district.terrain;
  const low = Math.min(...heights);
  const span = Math.max(...heights) - low || 1;
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

function resize() {
  const ratio = window.devicePixelRatio || 1;
  const box = canvas.getBoundingClientRect();
  canvas.width = box.width * ratio;
  canvas.height = box.height * ratio;
  const xs = district.boundary.map((p) => p.x);
  const ys = district.boundary.map((p) => p.y);
  const worldW = Math.max(...xs) - Math.min(...xs);
  const worldH = Math.max(...ys) - Math.min(...ys);
  const margin = 80 * ratio;
  view.scale = Math.min((canvas.width - margin) / worldW, (canvas.height - margin) / worldH);
  view.x = (canvas.width - worldW * view.scale) / 2 - Math.min(...xs) * view.scale;
  view.y = (canvas.height - worldH * view.scale) / 2 - Math.min(...ys) * view.scale;
  draw();
}

const sx = (p) => p.x * view.scale + view.x;
const sy = (p) => p.y * view.scale + view.y;

function fromEvent(event) {
  const box = canvas.getBoundingClientRect();
  const ratio = canvas.width / box.width;
  return {
    x: ((event.clientX - box.left) * ratio - view.x) / view.scale,
    y: ((event.clientY - box.top) * ratio - view.y) / view.scale,
  };
}

function polygon(points) {
  ctx.beginPath();
  points.forEach((p, i) => (i ? ctx.lineTo(sx(p), sy(p)) : ctx.moveTo(sx(p), sy(p))));
  ctx.closePath();
}

function draw() {
  if (!district) return;
  ctx.fillStyle = "#0f1413";
  ctx.fillRect(0, 0, canvas.width, canvas.height);

  if (visible.terrain) {
    const { origin, cell, cols, rows } = district.terrain;
    ctx.imageSmoothingEnabled = true;
    ctx.drawImage(terrainImage, sx(origin), sy(origin), (cols - 1) * cell * view.scale, (rows - 1) * cell * view.scale);
  }

  for (const lot of district.lots) {
    if (lot.land_use === "park") {
      polygon(lot.polygon);
      ctx.fillStyle = "#2f6a45";
      ctx.fill();
    } else if (visible.lots) {
      polygon(lot.polygon);
      ctx.strokeStyle = "rgba(255,255,255,.14)";
      ctx.lineWidth = 1;
      ctx.stroke();
    }
  }

  if (visible.streets) drawStreets();
  if (visible.buildings) drawBuildings();
  district.networks.forEach((network, index) => {
    if (visible[network.id]) drawNetwork(network, district.specs[index]);
  });
  if (visible.hazards) drawHazards();
  drawCursor();
  drawSection();
  drawStats();
}

function drawStreets() {
  ctx.lineCap = "round";
  ctx.strokeStyle = "#39433f";
  district.street_edges.forEach((edge, index) => {
    const a = district.street_nodes[edge.a].pos;
    const b = district.street_nodes[edge.b].pos;
    ctx.lineWidth = edge.width * view.scale;
    ctx.strokeStyle = index === selectedStreet ? "#e8c468" : "#39433f";
    ctx.beginPath();
    ctx.moveTo(sx(a), sy(a));
    ctx.lineTo(sx(b), sy(b));
    ctx.stroke();
  });
}

function drawBuildings() {
  const lost = new Set(Object.values(snapshot.outages).flat());
  const destroyed = new Set(snapshot.destroyed);
  district.buildings.forEach((building, index) => {
    polygon(building.footprint);
    if (destroyed.has(index)) {
      ctx.fillStyle = "#1a1a1a";
      ctx.fill();
      ctx.strokeStyle = "#ff3b1f";
      ctx.lineWidth = 1;
      ctx.stroke();
      return;
    }
    const [r, g, b] = LAND_COLORS[building.land_use];
    const lift = Math.min(building.floors / 30, 1) * 70;
    ctx.fillStyle = lost.has(index) ? `rgb(${190 + lift / 2},${95 + lift / 3},70)` : `rgb(${r + lift},${g + lift},${b + lift})`;
    ctx.fill();
  });
}

function drawNetwork(network, spec) {
  const broken = new Set(snapshot.broken[network.id]);
  ctx.lineCap = "butt";
  network.segments.forEach((segment, index) => {
    const direction = { x: segment.pb.x - segment.pa.x, y: segment.pb.y - segment.pa.y };
    const length = Math.hypot(direction.x, direction.y) || 1;
    const shift = { x: (-direction.y / length) * spec.lateral_offset, y: (direction.x / length) * spec.lateral_offset };
    const a = { x: segment.pa.x + shift.x, y: segment.pa.y + shift.y };
    const b = { x: segment.pb.x + shift.x, y: segment.pb.y + shift.y };
    ctx.setLineDash(segment.overhead ? [6, 4] : []);
    ctx.lineWidth = segment.street_edge === null ? 3 : 1.6;
    ctx.strokeStyle = broken.has(index) ? "#ff3b1f" : spec.color;
    ctx.beginPath();
    ctx.moveTo(sx(a), sy(a));
    ctx.lineTo(sx(b), sy(b));
    ctx.stroke();
  });
  ctx.setLineDash([]);
  ctx.fillStyle = spec.color;
  for (const node of network.powered_nodes) {
    const p = district.street_nodes[node].pos;
    ctx.fillRect(sx(p) - 3, sy(p) - 3, 6, 6);
  }
  const trunk = network.segments.find((s) => s.street_edge === null);
  if (trunk) {
    const at = trunk.a === network.source ? trunk.pa : trunk.pb;
    ctx.beginPath();
    ctx.arc(sx(at), sy(at), 7, 0, Math.PI * 2);
    ctx.fill();
  }
}

function drawHazards() {
  for (const hazard of snapshot.hazards) {
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
  if (selectedStreet === null) {
    context.fillStyle = "#8b9893";
    context.fillText("Use a ferramenta Corte e clique numa rua.", 12, 22);
    return;
  }
  const edge = district.street_edges[selectedStreet];
  const x = (meters) => box.width / 2 + (meters * box.width) / 34;
  const groundY = box.height * 0.5;
  const y = (height) => groundY - height * 11;

  context.fillStyle = "#2b2520";
  context.fillRect(0, groundY, box.width, box.height - groundY);
  context.fillStyle = "#454f4b";
  context.fillRect(x(-edge.width / 2), groundY - 2, x(edge.width / 2) - x(-edge.width / 2), 4);
  context.fillStyle = "#dfe6e2";
  context.fillText(`${edge.class === "arterial" ? "Avenida" : "Rua local"}, ${edge.width} m`, 8, 14);

  district.networks.forEach((network, index) => {
    const spec = district.specs[index];
    if (!visible[spec.id]) return;
    const broken = new Set(snapshot.broken[network.id]);
    network.segments.forEach((segment, segmentIndex) => {
      if (segment.street_edge !== selectedStreet) return;
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
    number.textContent = value;
    element.append(name, number);
    return element;
  };
  const rows = [
    row("Prédios", district.buildings.length),
    row("Destruídos", snapshot.destroyed.length),
    ...district.specs.map((spec) => row(`Sem ${spec.label.toLowerCase()}`, snapshot.outages[spec.id].length)),
    ...Object.entries(HAZARDS).map(([kind, [, label]]) =>
      row(label, snapshot.hazards.filter((h) => h.kind === kind).length),
    ),
  ];
  $("stats").replaceChildren(...rows);
}

const mode = () => document.querySelector("input[name=mode]:checked").value;

function apply(event) {
  snapshot = JSON.parse(checked(withCString(JSON.stringify(event), (p) => wasm.city_apply_event(session, p))));
  draw();
}

function distanceToSegment(p, a, b) {
  const dx = b.x - a.x;
  const dy = b.y - a.y;
  const t = Math.max(0, Math.min(1, ((p.x - a.x) * dx + (p.y - a.y) * dy) / (dx * dx + dy * dy || 1)));
  return Math.hypot(p.x - a.x - dx * t, p.y - a.y - dy * t);
}

function nearestStreet(at) {
  let best = null;
  let bestDistance = Infinity;
  district.street_edges.forEach((edge, index) => {
    const distance = distanceToSegment(at, district.street_nodes[edge.a].pos, district.street_nodes[edge.b].pos);
    if (distance < bestDistance) {
      bestDistance = distance;
      best = index;
    }
  });
  return best;
}

function nearestSegment(at) {
  let best = null;
  let bestDistance = 30;
  district.networks.forEach((network) => {
    if (!visible[network.id]) return;
    network.segments.forEach((segment, index) => {
      const distance = distanceToSegment(at, segment.pa, segment.pb);
      if (distance < bestDistance) {
        bestDistance = distance;
        best = { network: network.id, segment: index };
      }
    });
  });
  return best;
}

canvas.addEventListener("click", (event) => {
  const at = fromEvent(event);
  if (mode() === "destroy") {
    apply({ kind: "destroy", center: at, radius: Number($("radius").value), max_depth: Number($("depth").value) });
  } else if (mode() === "cut") {
    const target = nearestSegment(at);
    if (target) apply({ kind: "cut", ...target });
  } else {
    selectedStreet = nearestStreet(at);
    draw();
  }
});
canvas.addEventListener("mousemove", (event) => {
  hover = fromEvent(event);
  draw();
});
$("generate").onclick = generate;
$("random").onclick = () => {
  $("seed").value = Math.floor(Math.random() * 1e9);
  generate();
};
$("reset").onclick = () => {
  snapshot = JSON.parse(checked(wasm.city_reset(session)));
  draw();
};
window.addEventListener("resize", resize);

(async () => {
  const bytes = await (await fetch("city.wasm")).arrayBuffer();
  wasm = (await WebAssembly.instantiate(bytes, {})).instance.exports;
  generate();
  window.cityReady = true;
})();
