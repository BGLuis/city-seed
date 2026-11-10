use city_core::city::{City, CityInput, generate_city, generate_district};
use city_core::geometry::{centroid, point, point_in_convex};
use city_core::mesh::{
    CityMesh, FLOOR_HEIGHT, MeshLayer, PIPE_HALF_HEIGHT, city_mesh, district_mesh, mesh_hash,
};
use city_core::networks::Segment;
use city_core::terrain::Terrain;
use city_core::{District, regions};

/// Slivers below this are rounding noise in `f32`, with no meaningful winding.
const MEASURABLE_AREA: f32 = 0.01;
const MIN_DRAWN_SEGMENT: f64 = 0.01;
/// Lots drape over the terrain, so a fan sliver may sit almost perpendicular to
/// the face normal; a triangle wound backwards would be near -1.
const MIN_FACING_COSINE: f32 = -0.5;

fn city_with(seed: u64, population: u64, region: &str, overhead_share: Option<f64>) -> City {
    let mut profile = regions::find(region).expect("preset").profile;
    if let Some(share) = overhead_share {
        profile.overhead_share = share;
    }
    generate_city(&CityInput {
        seed,
        population,
        region: Some(region.into()),
        profile,
        extra_networks: Vec::new(),
    })
    .expect("valid city")
}

fn city() -> City {
    city_with(42, 2_000_000, "JP-KT", None)
}

fn busiest_district(city: &City) -> (usize, District) {
    (0..city.districts.len().min(40))
        .map(|id| (id, generate_district(city, id).expect("district")))
        .max_by_key(|(_, district)| district.buildings.len())
        .expect("city has districts")
}

fn layer<'a>(mesh: &'a CityMesh, id: &str, underground: bool) -> &'a MeshLayer {
    mesh.layers
        .iter()
        .find(|l| l.id == id && l.underground == underground)
        .unwrap_or_else(|| panic!("layer {id} underground={underground}"))
}

fn assert_well_formed(mesh: &CityMesh) {
    for layer in &mesh.layers {
        assert_eq!(layer.indices.len() % 3, 0, "{}", layer.id);
        for triangle in layer.indices.chunks(3) {
            let corners = triangle.map_windows_compat(&layer.vertices);
            let normal = cross(
                subtract(corners[1], corners[0]),
                subtract(corners[2], corners[0]),
            );
            let stored = layer.vertices[triangle[0] as usize].normal;
            let length = dot(stored, stored).sqrt();
            assert!(
                (length - 1.0).abs() < 1e-3,
                "{}: normal {stored:?}",
                layer.id
            );
            let doubled_area = dot(normal, normal).sqrt();
            assert!(
                doubled_area < MEASURABLE_AREA
                    || dot(normal, stored) / doubled_area > MIN_FACING_COSINE,
                "{}: triangle wound against its normal",
                layer.id
            );
        }
    }
}

trait TriangleCorners {
    fn map_windows_compat(&self, vertices: &[city_core::mesh::Vertex]) -> [[f32; 3]; 3];
}

impl TriangleCorners for [u32] {
    fn map_windows_compat(&self, vertices: &[city_core::mesh::Vertex]) -> [[f32; 3]; 3] {
        [
            vertices[self[0] as usize].position,
            vertices[self[1] as usize].position,
            vertices[self[2] as usize].position,
        ]
    }
}

fn subtract(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

#[test]
fn every_triangle_is_valid_and_faces_outwards() {
    let city = city();
    assert_well_formed(&city_mesh(&city));
    let (id, district) = busiest_district(&city);
    assert_well_formed(&district_mesh(&city, id, &district));
}

#[test]
fn buildings_rise_exactly_their_floor_count_above_the_ground() {
    let city = city();
    let terrain = Terrain::new(city.context.terrain_seed, city.profile.relief);
    let (id, district) = busiest_district(&city);
    let mesh = district_mesh(&city, id, &district);
    let buildings = layer(&mesh, "buildings", false);
    assert!(!district.buildings.is_empty());

    let mut cursor = 0;
    for building in &district.buildings {
        let count = building.footprint.len() * 5;
        let vertices = &buildings.vertices[cursor..cursor + count];
        cursor += count;
        let top = vertices
            .iter()
            .map(|v| v.position[2])
            .fold(f32::MIN, f32::max);
        let base = vertices
            .iter()
            .map(|v| v.position[2])
            .fold(f32::MAX, f32::min);
        let ground = terrain.height(centroid(&building.footprint));
        let expected = ground + f64::from(building.floors) * FLOOR_HEIGHT;
        assert!(
            (f64::from(top) - expected).abs() < 0.01,
            "top {top} != {expected}"
        );
        for corner in &building.footprint {
            assert!(
                f64::from(base) < terrain.height(*corner),
                "a corner of the building floats above the ground"
            );
        }
    }
    assert_eq!(cursor, buildings.vertices.len());
}

#[test]
fn buildings_stay_inside_their_district() {
    let city = city();
    let (id, district) = busiest_district(&city);
    let mesh = district_mesh(&city, id, &district);
    let polygon = &city.districts[id].polygon;
    for vertex in &layer(&mesh, "buildings", false).vertices {
        let world = point(
            mesh.origin[0] + f64::from(vertex.position[0]),
            mesh.origin[1] + f64::from(vertex.position[1]),
        );
        assert!(point_in_convex(world, polygon), "{world:?} is outside");
    }
}

fn assert_tubes_follow_segments(layer: &MeshLayer, segments: &[&Segment]) {
    assert_eq!(layer.vertices.len(), segments.len() * 16, "{}", layer.id);
    for (tube, segment) in layer.vertices.chunks(16).zip(segments) {
        let top = tube.iter().map(|v| v.position[2]).fold(f32::MIN, f32::max);
        let bottom = tube.iter().map(|v| v.position[2]).fold(f32::MAX, f32::min);
        let high = segment.za.max(segment.zb) + PIPE_HALF_HEIGHT;
        let low = segment.za.min(segment.zb) - PIPE_HALF_HEIGHT;
        assert!((f64::from(top) - high).abs() < 0.01, "{top} != {high}");
        assert!((f64::from(bottom) - low).abs() < 0.01, "{bottom} != {low}");
    }
}

#[test]
fn network_tubes_sit_at_the_level_the_network_was_routed() {
    let city = city();
    let (id, district) = busiest_district(&city);
    let mesh = district_mesh(&city, id, &district);
    for network in &district.networks {
        for underground in [true, false] {
            let segments: Vec<&Segment> = network
                .segments
                .iter()
                .filter(|s| (!s.overhead && s.depth > 0.0) == underground)
                .filter(|s| s.pa.distance(s.pb) >= MIN_DRAWN_SEGMENT)
                .collect();
            if !segments.is_empty() {
                assert_tubes_follow_segments(layer(&mesh, &network.id, underground), &segments);
            }
        }
    }
}

#[test]
fn power_is_only_underground_when_the_region_buries_it() {
    let buried = city_with(5, 500_000, "BR-SE", Some(0.0));
    let (id, district) = busiest_district(&buried);
    let mesh = district_mesh(&buried, id, &district);
    assert!(layer(&mesh, "power", true).indices.len() > 100);

    let aerial = city_with(5, 500_000, "BR-SE", Some(1.0));
    let (id, district) = busiest_district(&aerial);
    let mesh = district_mesh(&aerial, id, &district);
    assert!(!mesh.layers.iter().any(|l| l.id == "power" && l.underground));
    assert!(layer(&mesh, "power", false).indices.len() > 100);
}

#[test]
fn layers_take_the_colour_of_their_network() {
    let city = city();
    let (id, district) = busiest_district(&city);
    let mesh = district_mesh(&city, id, &district);
    let spec = district
        .specs
        .iter()
        .find(|s| s.id == "sewer")
        .expect("sewer");
    let [r, g, b] =
        [1, 3, 5].map(|at| u8::from_str_radix(&spec.color[at..at + 2], 16).expect("hex colour"));
    assert_eq!(
        layer(&mesh, "sewer", true).vertices[0].color,
        [r, g, b, 255]
    );
}

#[test]
fn macro_ground_has_one_contiguous_tile_per_district() {
    let city = city();
    let mesh = city_mesh(&city);
    let ground = layer(&mesh, "ground", false);
    assert_eq!(ground.tiles.len(), city.districts.len());

    let mut next = 0;
    for (tile, plan) in ground.tiles.iter().zip(&city.districts) {
        assert_eq!(tile.district, plan.id);
        assert_eq!(tile.first_index, next, "tiles must be contiguous");
        assert!(tile.index_count > 0);
        next += tile.index_count;
        let middle = centroid(&plan.polygon);
        for (axis, value) in [middle.x - mesh.origin[0], middle.y - mesh.origin[1]]
            .into_iter()
            .enumerate()
        {
            assert!(f64::from(tile.bounds.min[axis]) <= value);
            assert!(f64::from(tile.bounds.max[axis]) >= value);
        }
    }
    assert_eq!(next as usize, ground.indices.len());
}

#[test]
fn neighbouring_tiles_share_the_height_of_their_common_corners() {
    let city = city();
    let mesh = city_mesh(&city);
    let ground = layer(&mesh, "ground", false);
    let mut heights = std::collections::HashMap::new();
    let mut shared = 0;
    for vertex in &ground.vertices {
        let key = (
            (vertex.position[0] * 10.0) as i64,
            (vertex.position[1] * 10.0) as i64,
        );
        if let Some(previous) = heights.insert(key, vertex.position[2]) {
            shared += 1;
            assert!((previous - vertex.position[2]).abs() < 0.01);
        }
    }
    assert!(shared > 0);
}

#[test]
fn the_same_input_builds_the_same_mesh() {
    let first = city();
    let second = city();
    assert_eq!(
        mesh_hash(&city_mesh(&first)),
        mesh_hash(&city_mesh(&second))
    );
    let (id, district) = busiest_district(&first);
    let again = generate_district(&second, id).expect("district");
    assert_eq!(
        mesh_hash(&district_mesh(&first, id, &district)),
        mesh_hash(&district_mesh(&second, id, &again)),
    );

    let other = city_with(43, 2_000_000, "JP-KT", None);
    assert_ne!(mesh_hash(&city_mesh(&first)), mesh_hash(&city_mesh(&other)));
}
