use crate::Profile;
use crate::city::DensityField;
use crate::geometry::{Point, point};
use crate::networks::{Graph, Network, NetworkSpec, Segment, Topology};
use crate::rng::Rng;
use crate::streets::StreetLayout;
use std::collections::BTreeSet;

const METRO_MIN_POPULATION: u64 = 1_000_000;
const RAIL_MIN_POPULATION: u64 = 500_000;
const METRO_LINE_SPACING: f64 = 12_000.0;
const RAIL_LINE_SPACING: f64 = 15_000.0;
const METRO_REACH: f64 = 25_000.0;
const STATION_SPACING: f64 = 4_000.0;
const MIN_TUNNEL_COVER: f64 = 5.0;

pub struct TransitLines {
    pub networks: Vec<(NetworkSpec, Network)>,
    pub rails: Vec<Vec<Point>>,
}

fn metro_spec() -> NetworkSpec {
    NetworkSpec {
        id: "metro".into(),
        label: "Metrô".into(),
        color: "#e05cc0".into(),
        topology: Topology::Transit,
        depth: 18.0,
        lateral_offset: 0.0,
        overhead_height: Some(8.0),
        requires: Some("power".into()),
        on_rupture: Vec::new(),
    }
}

fn rail_spec() -> NetworkSpec {
    NetworkSpec {
        id: "rail".into(),
        label: "Trem".into(),
        color: "#d8dde0".into(),
        topology: Topology::Transit,
        depth: 0.0,
        lateral_offset: 0.0,
        overhead_height: None,
        requires: Some("power".into()),
        on_rupture: Vec::new(),
    }
}

pub fn build(
    layout: &StreetLayout,
    density: &DensityField,
    profile: &Profile,
    population: u64,
    side: f64,
    rng: &mut Rng,
) -> TransitLines {
    let graph = Graph::new(layout);
    let mut networks = Vec::new();
    let mut rails = Vec::new();

    let metro_lines = line_count(profile.metro_density, side, METRO_LINE_SPACING);
    if population >= METRO_MIN_POPULATION && metro_lines > 0 {
        let spec = metro_spec();
        let network = metro(&spec, metro_lines, layout, &graph, density, side, rng);
        networks.push((spec, network));
    }

    let rail_lines = line_count(profile.rail_density, side, RAIL_LINE_SPACING);
    if population >= RAIL_MIN_POPULATION && rail_lines > 0 {
        let spec = rail_spec();
        let (network, lines) = rail(&spec, rail_lines, layout, &graph, density, side, rng);
        networks.push((spec, network));
        rails = lines;
    }

    TransitLines { networks, rails }
}

fn line_count(density: f64, side: f64, spacing: f64) -> u32 {
    (density * side / spacing).round() as u32
}

fn random_direction(rng: &mut Rng) -> Point {
    loop {
        let candidate = point(rng.range(-1.0, 1.0), rng.range(-1.0, 1.0));
        if candidate.length() > 0.1 {
            return candidate.normalized();
        }
    }
}

fn clamp_to_city(p: Point, side: f64) -> Point {
    point(p.x.clamp(0.0, side), p.y.clamp(0.0, side))
}

/// Walks the street graph from `from` to `to`, returning `(node, edge)` hops.
fn route(graph: &Graph, layout: &StreetLayout, from: usize, to: usize) -> Vec<(usize, usize)> {
    let length = |a: usize, b: usize| layout.nodes[a].pos.distance(layout.nodes[b].pos);
    let forest = graph.shortest_forest(&[to], length);
    let mut hops = Vec::new();
    let mut current = from;
    while let Some((next, edge)) = forest.next_hop[current] {
        hops.push((next, edge));
        current = next;
    }
    hops
}

/// Track level follows the mean ground around a station, so hills bury the line
/// deeper and valleys bring it up to the surface, where it runs on a viaduct.
fn track_level(node: usize, layout: &StreetLayout, spec: &NetworkSpec) -> f64 {
    let neighbours: Vec<f64> = layout
        .edges
        .iter()
        .filter(|e| e.a == node || e.b == node)
        .map(|e| layout.nodes[if e.a == node { e.b } else { e.a }].elevation)
        .collect();
    let total: f64 = neighbours.iter().sum::<f64>() + layout.nodes[node].elevation;
    total / (neighbours.len() + 1) as f64 - spec.depth
}

fn metro(
    spec: &NetworkSpec,
    lines: u32,
    layout: &StreetLayout,
    graph: &Graph,
    density: &DensityField,
    side: f64,
    rng: &mut Rng,
) -> Network {
    let mut built = BTreeSet::new();
    let mut termini = BTreeSet::new();
    let mut stations = BTreeSet::new();
    let mut segments = Vec::new();
    let height = spec.overhead_height.expect("metro viaducts have a height");

    for index in 0..lines as usize {
        let center = density.centers[index % density.centers.len()].at;
        let reach = METRO_REACH.min(side / 2.0) * rng.range(0.6, 1.0);
        let direction = random_direction(rng);
        let east = graph.nearest_node(clamp_to_city(center + direction * reach, side));
        let hub = graph.nearest_node(center);
        let west = graph.nearest_node(clamp_to_city(center - direction * reach, side));
        termini.extend([east, west]);

        let mut current = east;
        let hops = route(graph, layout, east, hub)
            .into_iter()
            .chain(route(graph, layout, hub, west));
        stations.insert(current);
        for (next, edge) in hops {
            stations.insert(next);
            if built.insert(edge) {
                segments.push(metro_segment(current, next, edge, layout, spec, height));
            }
            current = next;
        }
    }
    finish(spec, layout, segments, termini, stations)
}

fn metro_segment(
    a: usize,
    b: usize,
    edge: usize,
    layout: &StreetLayout,
    spec: &NetworkSpec,
    height: f64,
) -> Segment {
    let (ground_a, ground_b) = (layout.nodes[a].elevation, layout.nodes[b].elevation);
    let (level_a, level_b) = (track_level(a, layout, spec), track_level(b, layout, spec));
    let cover = ((ground_a - level_a) + (ground_b - level_b)) / 2.0;
    let elevated = cover < MIN_TUNNEL_COVER;
    Segment {
        a,
        b,
        pa: layout.nodes[a].pos,
        pb: layout.nodes[b].pos,
        za: if elevated { ground_a + height } else { level_a },
        zb: if elevated { ground_b + height } else { level_b },
        depth: if elevated { 0.0 } else { cover },
        overhead: elevated,
        street_edge: Some(edge),
    }
}

fn rail(
    spec: &NetworkSpec,
    lines: u32,
    layout: &StreetLayout,
    graph: &Graph,
    density: &DensityField,
    side: f64,
    rng: &mut Rng,
) -> (Network, Vec<Vec<Point>>) {
    let hub_position = density.centers[0].at;
    let hub = graph.nearest_node(hub_position);
    let mut termini = BTreeSet::from([hub]);
    let mut stations = BTreeSet::from([hub]);
    let mut built = BTreeSet::new();
    let mut segments = Vec::new();
    let mut polylines = Vec::new();

    for _ in 0..lines {
        let direction = random_direction(rng);
        let end = clamp_to_city(hub_position + direction * side, side);
        let length = hub_position.distance(end);
        let mut previous = hub;
        let mut polyline = vec![layout.nodes[hub].pos];
        let mut along = STATION_SPACING;
        while along < length {
            let target = hub_position + (end - hub_position) * (along / length);
            let station = graph.nearest_node(target);
            if station != previous {
                stations.insert(station);
                polyline.push(layout.nodes[station].pos);
                if built.insert((previous.min(station), previous.max(station))) {
                    segments.push(rail_segment(previous, station, layout));
                }
                previous = station;
            }
            along += STATION_SPACING;
        }
        termini.insert(previous);
        polylines.push(polyline);
    }
    (finish(spec, layout, segments, termini, stations), polylines)
}

fn rail_segment(a: usize, b: usize, layout: &StreetLayout) -> Segment {
    Segment {
        a,
        b,
        pa: layout.nodes[a].pos,
        pb: layout.nodes[b].pos,
        za: layout.nodes[a].elevation,
        zb: layout.nodes[b].elevation,
        depth: 0.0,
        overhead: false,
        street_edge: None,
    }
}

fn finish(
    spec: &NetworkSpec,
    layout: &StreetLayout,
    segments: Vec<Segment>,
    termini: BTreeSet<usize>,
    stations: BTreeSet<usize>,
) -> Network {
    Network {
        id: spec.id.clone(),
        sources: termini.into_iter().collect(),
        node_count: layout.nodes.len(),
        segments,
        powered_nodes: stations.into_iter().collect(),
    }
}
