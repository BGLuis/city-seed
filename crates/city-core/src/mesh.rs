//! Triangle meshes for 3D engines, one buffer per layer. Coordinates are metres
//! with Z up, relative to `CityMesh::origin`: `f32` cannot hold absolute
//! positions in a 100 km city without visible jitter.

use crate::District;
use crate::city::City;
use crate::geometry::{Point, centroid, edges, point};
use crate::networks::{Network, NetworkSpec, Segment};
use crate::parcels::LandUse;
use crate::rng::fnv1a;
use crate::streets::{StreetEdge, StreetNode};
use crate::terrain::Terrain;
use serde::Serialize;

pub const FLOOR_HEIGHT: f64 = 3.2;

const FAR_TILE_DIVISIONS: usize = 4;
const NEAR_TILE_DIVISIONS: usize = 32;
const TILE_LIFT: f64 = -1.0;
const LOT_CELL: f64 = 30.0;
const MAX_LOT_DIVISIONS: usize = 16;
const LOT_LIFT: f64 = 0.1;
const STREET_LIFT: f64 = 0.2;
const AVENUE_LIFT: f64 = 3.0;
const WATER_LIFT: f64 = 4.0;
const FOUNDATION_DEPTH: f64 = 0.5;
const LOCAL_PIPE_HALF_WIDTH: f64 = 0.6;
const TRUNK_PIPE_HALF_WIDTH: f64 = 2.0;
pub const PIPE_HALF_HEIGHT: f64 = 0.3;
/// Narrower than a block, so the ribbon stays inside the blocks claimed as water.
const RIVER_WIDTH: f64 = 80.0;
const RIVER_DRAPE_STEP: f64 = 40.0;
const DRAPE_STEP: f64 = 150.0;
const MIN_SEGMENT_LENGTH: f64 = 0.01;

const STREET_COLOR: Rgb = [66, 69, 74];
const BRIDGE_COLOR: Rgb = [176, 92, 76];
const WATER_COLOR: Rgb = [58, 124, 176];
const TILE_NEAR_COLOR: Rgb = [108, 108, 104];
const SPARSE_COLOR: Rgb = [132, 142, 112];
const DENSE_COLOR: Rgb = [176, 170, 188];

type Rgb = [u8; 3];
type P3 = [f64; 3];

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub color: [u8; 4],
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Bounds {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

/// The slice of a layer's index buffer covering one district, so a renderer can
/// skip it once the detailed mesh of that district is loaded.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Tile {
    pub district: usize,
    pub first_index: u32,
    pub index_count: u32,
    pub bounds: Bounds,
}

#[derive(Clone, Debug)]
pub struct MeshLayer {
    pub id: String,
    pub label: String,
    /// Below the surface: only worth drawing while the ground is hidden.
    pub underground: bool,
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub tiles: Vec<Tile>,
}

impl MeshLayer {
    pub fn bounds(&self) -> Bounds {
        bounds_of(self.vertices.iter())
    }
}

#[derive(Clone, Debug)]
pub struct CityMesh {
    pub origin: [f64; 2],
    pub layers: Vec<MeshLayer>,
}

#[derive(Serialize)]
pub struct LayerSummary<'a> {
    pub id: &'a str,
    pub label: &'a str,
    pub underground: bool,
    pub vertex_count: usize,
    pub index_count: usize,
    pub bounds: Bounds,
    pub tiles: &'a [Tile],
}

#[derive(Serialize)]
pub struct MeshSummary<'a> {
    pub origin: [f64; 2],
    pub layers: Vec<LayerSummary<'a>>,
}

impl CityMesh {
    pub fn summary(&self) -> MeshSummary<'_> {
        MeshSummary {
            origin: self.origin,
            layers: self
                .layers
                .iter()
                .map(|layer| LayerSummary {
                    id: &layer.id,
                    label: &layer.label,
                    underground: layer.underground,
                    vertex_count: layer.vertices.len(),
                    index_count: layer.indices.len(),
                    bounds: layer.bounds(),
                    tiles: &layer.tiles,
                })
                .collect(),
        }
    }

    pub fn triangle_count(&self) -> usize {
        self.layers.iter().map(|l| l.indices.len() / 3).sum()
    }

    pub fn vertex_count(&self) -> usize {
        self.layers.iter().map(|l| l.vertices.len()).sum()
    }
}

pub fn mesh_hash(mesh: &CityMesh) -> u64 {
    let mut bytes = Vec::new();
    for layer in &mesh.layers {
        for vertex in &layer.vertices {
            for value in vertex.position.iter().chain(&vertex.normal) {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            bytes.extend_from_slice(&vertex.color);
        }
        for index in &layer.indices {
            bytes.extend_from_slice(&index.to_le_bytes());
        }
    }
    fnv1a(&bytes)
}

fn bounds_of<'a>(vertices: impl Iterator<Item = &'a Vertex>) -> Bounds {
    let mut bounds = Bounds {
        min: [f32::MAX; 3],
        max: [f32::MIN; 3],
    };
    for vertex in vertices {
        for axis in 0..3 {
            bounds.min[axis] = bounds.min[axis].min(vertex.position[axis]);
            bounds.max[axis] = bounds.max[axis].max(vertex.position[axis]);
        }
    }
    bounds
}

fn offset(p: P3, by: P3, scale: f64) -> P3 {
    [
        p[0] + by[0] * scale,
        p[1] + by[1] * scale,
        p[2] + by[2] * scale,
    ]
}

fn opaque([r, g, b]: Rgb) -> [u8; 4] {
    [r, g, b, 255]
}

fn blend(from: Rgb, to: Rgb, amount: f64) -> Rgb {
    let mix = |a: u8, b: u8| (f64::from(a) + (f64::from(b) - f64::from(a)) * amount) as u8;
    [
        mix(from[0], to[0]),
        mix(from[1], to[1]),
        mix(from[2], to[2]),
    ]
}

fn hex_color(text: &str) -> Rgb {
    let channel = |range| {
        text.get(range)
            .and_then(|digits| u8::from_str_radix(digits, 16).ok())
            .unwrap_or(128)
    };
    [channel(1..3), channel(3..5), channel(5..7)]
}

fn ground_color(land_use: LandUse) -> Rgb {
    match land_use {
        LandUse::Residential => [150, 146, 132],
        LandUse::Commercial => [142, 136, 150],
        LandUse::Industrial => [140, 126, 108],
        LandUse::Park => [112, 158, 92],
        LandUse::Forest => [62, 112, 70],
        LandUse::Wetland => [112, 160, 144],
        LandUse::Water => WATER_COLOR,
        LandUse::Rail => [96, 90, 84],
    }
}

fn building_color(land_use: LandUse, floors: u32) -> Rgb {
    let base = match land_use {
        LandUse::Commercial => [132, 150, 186],
        LandUse::Industrial => [168, 142, 112],
        _ => [206, 192, 170],
    };
    blend(base, [236, 238, 242], (f64::from(floors) / 40.0).min(0.6))
}

/// Accumulates one layer. Each face gets its own vertices so edges stay sharp.
struct Builder {
    origin: Point,
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
}

impl Builder {
    fn new(origin: Point) -> Self {
        Self {
            origin,
            vertices: Vec::new(),
            indices: Vec::new(),
        }
    }

    /// Adds a planar convex face wound counter-clockwise when seen from outside.
    fn face(&mut self, corners: &[P3], color: Rgb) {
        let mut normal = [0.0; 3];
        for (index, a) in corners.iter().enumerate() {
            let b = corners[(index + 1) % corners.len()];
            normal[0] += (a[1] - b[1]) * (a[2] + b[2]);
            normal[1] += (a[2] - b[2]) * (a[0] + b[0]);
            normal[2] += (a[0] - b[0]) * (a[1] + b[1]);
        }
        let length = (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
        if length == 0.0 {
            return;
        }
        let first = self.vertices.len() as u32;
        let normal = normal.map(|value| (value / length) as f32);
        let color = opaque(color);
        self.vertices.extend(corners.iter().map(|corner| Vertex {
            position: [
                (corner[0] - self.origin.x) as f32,
                (corner[1] - self.origin.y) as f32,
                corner[2] as f32,
            ],
            normal,
            color,
        }));
        for index in 1..corners.len() as u32 - 1 {
            self.indices
                .extend_from_slice(&[first, first + index, first + index + 1]);
        }
    }

    fn layer(self, id: &str, label: &str, underground: bool) -> MeshLayer {
        MeshLayer {
            id: id.into(),
            label: label.into(),
            underground,
            vertices: self.vertices,
            indices: self.indices,
            tiles: Vec::new(),
        }
    }

    /// Follows the terrain with a triangle fan whose every blade is subdivided
    /// the same way, so neighbouring tiles meet along their shared edges.
    fn tile(
        &mut self,
        polygon: &[Point],
        terrain: &Terrain,
        divisions: usize,
        lift: f64,
        color: Rgb,
    ) {
        let center = centroid(polygon);
        let steps = divisions as f64;
        let row_start = |row: usize| row * (row + 1) / 2;
        let mut grid = Vec::with_capacity(row_start(divisions + 1));
        for (a, b) in edges(polygon) {
            grid.clear();
            for row in 0..=divisions {
                for column in 0..=row {
                    let along = center + (a - center) * (row as f64 / steps);
                    let p = along + (b - a) * (column as f64 / steps);
                    grid.push([p.x, p.y, terrain.height(p) + lift]);
                }
            }
            for row in 0..divisions {
                for column in 0..=row {
                    let at = |r: usize, c: usize| grid[row_start(r) + c];
                    let next_column = at(row + 1, column + 1);
                    self.face(&[at(row, column), at(row + 1, column), next_column], color);
                    if column < row {
                        self.face(&[at(row, column), next_column, at(row, column + 1)], color);
                    }
                }
            }
        }
    }

    /// A flat strip, overrun by half its width to close the gap at junctions.
    fn ribbon(&mut self, from: P3, to: P3, width: f64, color: Rgb) {
        let along = point(to[0] - from[0], to[1] - from[1]);
        if along.length() < MIN_SEGMENT_LENGTH {
            return;
        }
        let half = width / 2.0;
        let direction = along.normalized();
        let side = direction.left_normal() * half;
        let overrun = direction * half;
        let start = point(from[0], from[1]) - overrun;
        let end = point(to[0], to[1]) + overrun;
        let corner = |p: Point, z: f64| [p.x, p.y, z];
        self.face(
            &[
                corner(start - side, from[2]),
                corner(end - side, to[2]),
                corner(end + side, to[2]),
                corner(start + side, from[2]),
            ],
            color,
        );
    }

    /// A rectangular tube with exaggerated width: real pipes would be invisible.
    fn tube(&mut self, from: P3, to: P3, half_width: f64, color: Rgb) {
        let along = point(to[0] - from[0], to[1] - from[1]);
        if along.length() < MIN_SEGMENT_LENGTH {
            return;
        }
        let side = along.normalized().left_normal();
        let left = [side.x * half_width, side.y * half_width, 0.0];
        let up = [0.0, 0.0, PIPE_HALF_HEIGHT];
        let section = [
            offset(offset([0.0; 3], left, -1.0), up, 1.0),
            offset(offset([0.0; 3], left, 1.0), up, 1.0),
            offset(offset([0.0; 3], left, 1.0), up, -1.0),
            offset(offset([0.0; 3], left, -1.0), up, -1.0),
        ];
        for index in 0..section.len() {
            let u = section[index];
            let v = section[(index + 1) % section.len()];
            self.face(
                &[
                    offset(from, u, 1.0),
                    offset(to, u, 1.0),
                    offset(to, v, 1.0),
                    offset(from, v, 1.0),
                ],
                color,
            );
        }
    }

    fn building(&mut self, footprint: &[Point], floors: u32, terrain: &Terrain, color: Rgb) {
        let ground = terrain.height(centroid(footprint));
        let top = ground + f64::from(floors) * FLOOR_HEIGHT;
        let base = footprint
            .iter()
            .map(|p| terrain.height(*p))
            .fold(ground, f64::min)
            - FOUNDATION_DEPTH;
        for (a, b) in edges(footprint) {
            self.face(
                &[
                    [a.x, a.y, base],
                    [b.x, b.y, base],
                    [b.x, b.y, top],
                    [a.x, a.y, top],
                ],
                color,
            );
        }
        let roof: Vec<P3> = footprint.iter().map(|p| [p.x, p.y, top]).collect();
        self.face(&roof, color);
    }
}

/// Large lots must follow the relief, or the ground below pokes through them.
fn lot_divisions(polygon: &[Point]) -> usize {
    let center = centroid(polygon);
    let reach = polygon
        .iter()
        .map(|p| p.distance(center))
        .fold(0.0, f64::max);
    ((reach / LOT_CELL).ceil() as usize).clamp(1, MAX_LOT_DIVISIONS)
}

fn is_buried(segment: &Segment) -> bool {
    !segment.overhead && segment.depth > 0.0
}

/// One layer for what lies below ground and another for what can be seen.
fn network_layers(
    specs: &[NetworkSpec],
    networks: &[Network],
    origin: Point,
    half_width: f64,
) -> Vec<MeshLayer> {
    let mut layers = Vec::new();
    for (spec, network) in specs.iter().zip(networks) {
        for underground in [true, false] {
            let mut builder = Builder::new(origin);
            let color = hex_color(&spec.color);
            for segment in network
                .segments
                .iter()
                .filter(|segment| is_buried(segment) == underground)
            {
                builder.tube(
                    [segment.pa.x, segment.pa.y, segment.za],
                    [segment.pb.x, segment.pb.y, segment.zb],
                    half_width,
                    color,
                );
            }
            if !builder.indices.is_empty() {
                layers.push(builder.layer(&spec.id, &spec.label, underground));
            }
        }
    }
    layers
}

fn street_layer(
    nodes: &[StreetNode],
    street_edges: &[StreetEdge],
    origin: Point,
    terrain: &Terrain,
    drape: Option<f64>,
    lift: f64,
) -> MeshLayer {
    let mut builder = Builder::new(origin);
    for edge in street_edges {
        let (a, b) = (&nodes[edge.a], &nodes[edge.b]);
        let color = if edge.bridge {
            BRIDGE_COLOR
        } else {
            STREET_COLOR
        };
        match drape {
            Some(step) => draped_line(&mut builder, a.pos, b.pos, step, |p| {
                (terrain.height(p) + lift, edge.width, color)
            }),
            None => builder.ribbon(
                [a.pos.x, a.pos.y, a.elevation + lift],
                [b.pos.x, b.pos.y, b.elevation + lift],
                edge.width,
                color,
            ),
        }
    }
    builder.layer("streets", "Ruas", false)
}

fn draped_line(
    builder: &mut Builder,
    from: Point,
    to: Point,
    step: f64,
    style: impl Fn(Point) -> (f64, f64, Rgb),
) {
    let pieces = ((from.distance(to) / step).ceil() as usize).max(1);
    let at = |index: usize| from + (to - from) * (index as f64 / pieces as f64);
    for index in 0..pieces {
        let (a, b) = (at(index), at(index + 1));
        let (za, width, color) = style(a);
        let (zb, ..) = style(b);
        builder.ribbon([a.x, a.y, za], [b.x, b.y, zb], width, color);
    }
}

/// The whole city at low detail: one ground tile per district plus avenues,
/// rivers and trunk networks.
pub fn city_mesh(city: &City) -> CityMesh {
    let origin = centroid(&city.boundary);
    let terrain = Terrain::new(city.context.terrain_seed, city.profile.relief);

    let mut ground = Builder::new(origin);
    let mut tiles = Vec::with_capacity(city.districts.len());
    for plan in &city.districts {
        let (first_index, first_vertex) = (ground.indices.len(), ground.vertices.len());
        let color = blend(SPARSE_COLOR, DENSE_COLOR, plan.density);
        ground.tile(
            &plan.polygon,
            &terrain,
            FAR_TILE_DIVISIONS,
            TILE_LIFT,
            color,
        );
        tiles.push(Tile {
            district: plan.id,
            first_index: first_index as u32,
            index_count: (ground.indices.len() - first_index) as u32,
            bounds: bounds_of(ground.vertices[first_vertex..].iter()),
        });
    }
    let mut ground_layer = ground.layer("ground", "Superfície", false);
    ground_layer.tiles = tiles;

    let mut water = Builder::new(origin);
    for river in &city.context.rivers {
        for piece in river.windows(2) {
            draped_line(&mut water, piece[0], piece[1], RIVER_DRAPE_STEP, |p| {
                (terrain.height(p) + WATER_LIFT, RIVER_WIDTH, WATER_COLOR)
            });
        }
    }

    let mut layers = vec![
        ground_layer,
        street_layer(
            &city.nodes,
            &city.edges,
            origin,
            &terrain,
            Some(DRAPE_STEP),
            AVENUE_LIFT,
        ),
        water.layer("rivers", "Rios", false),
    ];
    layers.extend(network_layers(
        &city.trunk_specs,
        &city.networks,
        origin,
        TRUNK_PIPE_HALF_WIDTH,
    ));
    CityMesh {
        origin: [origin.x, origin.y],
        layers,
    }
}

/// Full detail for one district, positioned to replace its tile from `city_mesh`.
pub fn district_mesh(city: &City, id: usize, district: &District) -> CityMesh {
    let origin = centroid(&city.districts[id].polygon);
    let terrain = Terrain::new(city.context.terrain_seed, city.profile.relief);

    let mut ground = Builder::new(origin);
    ground.tile(
        &district.boundary,
        &terrain,
        NEAR_TILE_DIVISIONS,
        TILE_LIFT,
        TILE_NEAR_COLOR,
    );
    for lot in &district.lots {
        let color = ground_color(lot.land_use);
        match lot_divisions(&lot.polygon) {
            1 => {
                let corners: Vec<P3> = lot
                    .polygon
                    .iter()
                    .map(|p| [p.x, p.y, terrain.height(*p) + LOT_LIFT])
                    .collect();
                ground.face(&corners, color);
            }
            divisions => ground.tile(&lot.polygon, &terrain, divisions, LOT_LIFT, color),
        }
    }

    let mut buildings = Builder::new(origin);
    for building in &district.buildings {
        buildings.building(
            &building.footprint,
            building.floors,
            &terrain,
            building_color(building.land_use, building.floors),
        );
    }

    let mut layers = vec![
        ground.layer("ground", "Superfície", false),
        street_layer(
            &district.street_nodes,
            &district.street_edges,
            origin,
            &terrain,
            None,
            STREET_LIFT,
        ),
        buildings.layer("buildings", "Prédios", false),
    ];
    layers.extend(network_layers(
        &district.specs,
        &district.networks,
        origin,
        LOCAL_PIPE_HALF_WIDTH,
    ));
    CityMesh {
        origin: [origin.x, origin.y],
        layers,
    }
}

#[cfg(test)]
mod layout {
    use super::Vertex;

    #[test]
    fn vertex_matches_the_documented_28_byte_layout() {
        assert_eq!(std::mem::size_of::<Vertex>(), 28);
    }
}
