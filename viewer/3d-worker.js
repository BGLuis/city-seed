// Generation runs here so that building a district never stalls a frame.

const VERTEX_BYTES = 28;
const encoder = new TextEncoder();
const decoder = new TextDecoder();

let wasm;
let session = 0;

function takeString(pointer) {
  const memory = new Uint8Array(wasm.memory.buffer);
  let end = pointer;
  while (memory[end]) end += 1;
  const text = decoder.decode(memory.subarray(pointer, end));
  wasm.city_string_free(pointer);
  return text;
}

function withCString(text, use) {
  const bytes = encoder.encode(`${text}\0`);
  const pointer = wasm.city_alloc(bytes.length);
  new Uint8Array(wasm.memory.buffer, pointer, bytes.length).set(bytes);
  try {
    return use(pointer);
  } finally {
    wasm.city_dealloc(pointer, bytes.length);
  }
}

function checked(pointer) {
  if (!pointer) throw new Error(takeString(wasm.city_last_error()));
  return pointer;
}

function extractMesh(pointer) {
  const info = JSON.parse(takeString(wasm.city_mesh_info(pointer)));
  const hash = takeString(wasm.city_mesh_hash_hex(pointer));
  const transfer = [];
  info.layers.forEach((layer, index) => {
    const vertices = wasm.city_mesh_vertices(pointer, index);
    const indices = wasm.city_mesh_indices(pointer, index);
    layer.vertices = wasm.memory.buffer.slice(vertices, vertices + layer.vertex_count * VERTEX_BYTES);
    layer.indices = wasm.memory.buffer.slice(indices, indices + layer.index_count * 4);
    transfer.push(layer.vertices, layer.indices);
  });
  wasm.city_mesh_free(pointer);
  return { mesh: { ...info, hash }, transfer };
}

const handlers = {
  generate({ input, token }) {
    if (session) wasm.city_free(session);
    session = 0;
    const started = performance.now();
    session = checked(withCString(JSON.stringify(input), (p) => wasm.city_generate(p)));
    const generated = performance.now();
    const { mesh, transfer } = extractMesh(wasm.city_mesh_macro(session));
    postMessage({
      kind: "city",
      token,
      mesh,
      cityHash: takeString(wasm.city_hash_hex(session)),
      generateMs: generated - started,
      meshMs: performance.now() - generated,
    }, transfer);
  },
  district({ id, token }) {
    const started = performance.now();
    const { mesh, transfer } = extractMesh(checked(wasm.city_mesh_district(session, id)));
    postMessage({ kind: "district", token, id, mesh, buildMs: performance.now() - started }, transfer);
  },
};

const ready = (async () => {
  const bytes = await (await fetch("city.wasm")).arrayBuffer();
  wasm = (await WebAssembly.instantiate(bytes, {})).instance.exports;
  postMessage({ kind: "ready", regions: JSON.parse(takeString(wasm.city_regions())) });
})().catch((error) => postMessage({ kind: "error", message: String(error) }));

onmessage = async ({ data }) => {
  await ready;
  try {
    handlers[data.kind](data);
  } catch (error) {
    postMessage({ kind: "error", token: data.token, message: String(error.message ?? error) });
  }
};
