use crate::geometry::{Point, centroid};
use crate::streets::StreetLayout;
use crate::terrain::{Terrain, noise};
use crate::{GenError, Port, Profile};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::BinaryHeap;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Topology {
    Radial,
    Looped { booster_head: Option<f64> },
    Gravity { slope: f64, max_depth: f64 },
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuptureEffect {
    Flood,
    GasLeak,
    LiveCable,
    Contamination,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NetworkSpec {
    pub id: String,
    pub label: String,
    pub color: String,
    pub topology: Topology,
    pub depth: f64,
    pub lateral_offset: f64,
    #[serde(default)]
    pub overhead_height: Option<f64>,
    #[serde(default)]
    pub requires: Option<String>,
    #[serde(default)]
    pub on_rupture: Vec<RuptureEffect>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Segment {
    pub a: usize,
    pub b: usize,
    pub pa: Point,
    pub pb: Point,
    pub za: f64,
    pub zb: f64,
    pub depth: f64,
    pub overhead: bool,
    pub street_edge: Option<usize>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Network {
    pub id: String,
    pub sources: Vec<usize>,
    pub node_count: usize,
    pub segments: Vec<Segment>,
    pub powered_nodes: Vec<usize>,
}

const MIN_DEPTH_CLEARANCE: f64 = 0.2;
const TRUNK_OUTWARD_DISTANCE: f64 = 40.0;
const UPHILL_FLOW_PENALTY: f64 = 40.0;
const MIN_PIPE_RUN: f64 = 1.0;

pub fn default_specs(profile: &Profile) -> Vec<NetworkSpec> {
    let mut specs = vec![
        NetworkSpec {
            id: "power".into(),
            label: "Energia".into(),
            color: "#f2b632".into(),
            topology: Topology::Radial,
            depth: 0.7,
            lateral_offset: 3.5,
            overhead_height: Some(9.0),
            requires: None,
            on_rupture: vec![RuptureEffect::LiveCable],
        },
        NetworkSpec {
            id: "water".into(),
            label: "Água".into(),
            color: "#3b9dff".into(),
            topology: Topology::Looped {
                booster_head: Some(15.0),
            },
            depth: 1.2,
            lateral_offset: -2.0,
            overhead_height: None,
            requires: Some("power".into()),
            on_rupture: vec![RuptureEffect::Flood],
        },
    ];
    if profile.piped_gas {
        specs.push(NetworkSpec {
            id: "gas".into(),
            label: "Gás".into(),
            color: "#ff6b4a".into(),
            topology: Topology::Looped { booster_head: None },
            depth: 0.95,
            lateral_offset: 2.0,
            overhead_height: None,
            requires: None,
            on_rupture: vec![RuptureEffect::GasLeak],
        });
    }
    specs.push(NetworkSpec {
        id: "sewer".into(),
        label: "Esgoto".into(),
        color: "#9c7a54".into(),
        topology: Topology::Gravity {
            slope: 0.005,
            max_depth: 6.0,
        },
        depth: 1.8,
        lateral_offset: 0.0,
        overhead_height: None,
        requires: Some("power".into()),
        on_rupture: vec![RuptureEffect::Contamination],
    });
    specs
}

pub fn validate_specs(specs: &[NetworkSpec]) -> Result<(), GenError> {
    for (i, spec) in specs.iter().enumerate() {
        if specs[..i].iter().any(|other| other.id == spec.id) {
            return Err(GenError::DuplicateNetwork(spec.id.clone()));
        }
        if let Some(required) = &spec.requires
            && !specs[..i].iter().any(|other| &other.id == required)
        {
            return Err(GenError::UnknownDependency {
                network: spec.id.clone(),
                requires: required.clone(),
            });
        }
        // Parallel lines are separated by lateral offset, but every network
        // crosses every other at intersections, so only depth can separate them.
        if let Some(other) = specs[..i]
            .iter()
            .find(|other| (other.depth - spec.depth).abs() < MIN_DEPTH_CLEARANCE)
        {
            return Err(GenError::DepthConflict(other.id.clone(), spec.id.clone()));
        }
    }
    Ok(())
}

pub fn port_for(spec: &NetworkSpec, boundary: &[Point], terrain: &Terrain) -> Point {
    let by_height = |a: &&Point, b: &&Point| terrain.height(**a).total_cmp(&terrain.height(**b));
    let vertex = match spec.topology {
        Topology::Gravity { .. } => boundary.iter().min_by(by_height),
        _ => boundary.iter().max_by(by_height),
    }
    .expect("boundary has vertices");
    let center = centroid(boundary);
    *vertex + (*vertex - center).normalized() * TRUNK_OUTWARD_DISTANCE
}

struct Frontier {
    cost: f64,
    node: usize,
}

impl Ord for Frontier {
    /// Reversed so the max-heap pops the cheapest node, lowest index first.
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .cost
            .total_cmp(&self.cost)
            .then(other.node.cmp(&self.node))
    }
}

impl PartialOrd for Frontier {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Frontier {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}

impl Eq for Frontier {}

struct Graph<'a> {
    layout: &'a StreetLayout,
    adjacency: Vec<Vec<(usize, usize)>>,
}

impl<'a> Graph<'a> {
    fn new(layout: &'a StreetLayout) -> Self {
        let mut adjacency = vec![Vec::new(); layout.nodes.len()];
        for (index, edge) in layout.edges.iter().enumerate() {
            adjacency[edge.a].push((edge.b, index));
            adjacency[edge.b].push((edge.a, index));
        }
        Self { layout, adjacency }
    }

    fn nearest_node(&self, p: Point) -> usize {
        self.layout
            .index
            .nearest(p)
            .expect("street graph has nodes")
    }

    /// Grows one shortest-path tree per root; each node joins its cheapest root.
    fn shortest_forest(&self, roots: &[usize], cost: impl Fn(usize, usize) -> f64) -> Forest {
        let count = self.layout.nodes.len();
        let mut distance = vec![f64::MAX; count];
        let mut next_hop = vec![None; count];
        let mut owner = vec![0; count];
        let mut settled = vec![false; count];
        let mut frontier = BinaryHeap::new();
        for (index, root) in roots.iter().enumerate().rev() {
            distance[*root] = 0.0;
            owner[*root] = index;
            frontier.push(Frontier {
                cost: 0.0,
                node: *root,
            });
        }
        while let Some(Frontier { node: current, .. }) = frontier.pop() {
            if std::mem::replace(&mut settled[current], true) {
                continue;
            }
            for &(neighbor, edge) in &self.adjacency[current] {
                let candidate = distance[current] + cost(neighbor, current);
                if candidate < distance[neighbor] {
                    distance[neighbor] = candidate;
                    next_hop[neighbor] = Some((current, edge));
                    owner[neighbor] = owner[current];
                    frontier.push(Frontier {
                        cost: candidate,
                        node: neighbor,
                    });
                }
            }
        }
        Forest {
            distance,
            next_hop,
            owner,
        }
    }
}

struct Forest {
    distance: Vec<f64>,
    next_hop: Vec<Option<(usize, usize)>>,
    owner: Vec<usize>,
}

pub fn build_all(
    specs: &[NetworkSpec],
    ports: &[Port],
    boundary: &[Point],
    layout: &StreetLayout,
    terrain: &Terrain,
    profile: &Profile,
    overhead_seed: u64,
) -> Vec<Network> {
    let graph = Graph::new(layout);
    specs
        .iter()
        .map(|spec| {
            let mut own_ports: Vec<(Point, Option<f64>)> = ports
                .iter()
                .filter(|port| port.network == spec.id)
                .map(|port| (port.at, port.level))
                .collect();
            if own_ports.is_empty() {
                own_ports.push((port_for(spec, boundary, terrain), None));
            }
            let entries = own_ports
                .iter()
                .map(|(at, _)| graph.nearest_node(*at))
                .collect();
            let builder = Builder {
                spec,
                ports: own_ports,
                entries,
                graph: &graph,
                terrain,
                profile,
                overhead_seed,
            };
            builder.build()
        })
        .collect()
}

struct Builder<'a> {
    spec: &'a NetworkSpec,
    ports: Vec<(Point, Option<f64>)>,
    entries: Vec<usize>,
    graph: &'a Graph<'a>,
    terrain: &'a Terrain,
    profile: &'a Profile,
    overhead_seed: u64,
}

impl Builder<'_> {
    fn build(&self) -> Network {
        match self.spec.topology {
            Topology::Radial => self.radial(),
            Topology::Looped { booster_head } => self.looped(booster_head),
            Topology::Gravity { slope, max_depth } => self.gravity(slope, max_depth),
        }
    }

    fn layout(&self) -> &StreetLayout {
        self.graph.layout
    }

    fn street_count(&self) -> usize {
        self.layout().nodes.len()
    }

    fn source(&self, port: usize) -> usize {
        self.street_count() + port
    }

    fn port_of(&self, node: usize) -> Option<&(Point, Option<f64>)> {
        node.checked_sub(self.street_count())
            .map(|port| &self.ports[port])
    }

    fn is_overhead(&self, a: Point, b: Point) -> bool {
        let midpoint = (a + b) * 0.5;
        self.spec.overhead_height.is_some()
            && noise(self.overhead_seed, midpoint.x / 400.0, midpoint.y / 400.0)
                < self.profile.overhead_share
    }

    fn flat_segment(&self, a: usize, b: usize, street_edge: Option<usize>) -> Segment {
        let (pa, ground_a) = self.endpoint(a);
        let (pb, ground_b) = self.endpoint(b);
        let overhead = self.is_overhead(pa, pb);
        let (za, zb, depth) = match (overhead, self.spec.overhead_height) {
            (true, Some(height)) => (ground_a + height, ground_b + height, 0.0),
            _ => (
                ground_a - self.spec.depth,
                ground_b - self.spec.depth,
                self.spec.depth,
            ),
        };
        Segment {
            a,
            b,
            pa,
            pb,
            za,
            zb,
            depth,
            overhead,
            street_edge,
        }
    }

    fn endpoint(&self, node: usize) -> (Point, f64) {
        match self.port_of(node) {
            Some((at, _)) => (*at, self.terrain.height(*at)),
            None => {
                let street_node = &self.layout().nodes[node];
                (street_node.pos, street_node.elevation)
            }
        }
    }

    fn finish(&self, segments: Vec<Segment>, powered_nodes: Vec<usize>) -> Network {
        Network {
            id: self.spec.id.clone(),
            sources: (0..self.ports.len()).map(|p| self.source(p)).collect(),
            node_count: self.street_count() + self.ports.len(),
            segments,
            powered_nodes,
        }
    }

    fn port_links(&self) -> Vec<Segment> {
        self.entries
            .iter()
            .enumerate()
            .map(|(port, entry)| self.flat_segment(self.source(port), *entry, None))
            .collect()
    }

    fn radial(&self) -> Network {
        let forest = self.forest(|neighbor, current| self.length(neighbor, current));
        let mut segments = self.port_links();
        for (node, hop) in forest.next_hop.iter().enumerate() {
            if let Some((parent, edge)) = hop {
                segments.push(self.flat_segment(*parent, node, Some(*edge)));
            }
        }
        self.finish(segments, Vec::new())
    }

    fn looped(&self, booster_head: Option<f64>) -> Network {
        let mut segments = self.port_links();
        for (index, edge) in self.layout().edges.iter().enumerate() {
            segments.push(self.flat_segment(edge.a, edge.b, Some(index)));
        }
        let powered_nodes = booster_head.map_or_else(Vec::new, |head| {
            let forest = self.forest(|neighbor, current| self.length(neighbor, current));
            (0..self.street_count())
                .filter(|node| {
                    let (_, source_ground) = self.endpoint(self.source(forest.owner[*node]));
                    self.layout().nodes[*node].elevation > source_ground - head
                })
                .collect()
        });
        self.finish(segments, powered_nodes)
    }

    fn gravity(&self, slope: f64, max_depth: f64) -> Network {
        let forest = self.forest(|neighbor, current| {
            let rise =
                self.layout().nodes[current].elevation - self.layout().nodes[neighbor].elevation;
            self.length(neighbor, current) + UPHILL_FLOW_PENALTY * rise.max(0.0)
        });
        let cover = self.spec.depth;
        let mut order: Vec<usize> = (0..self.street_count()).collect();
        order.sort_by(|a, b| forest.distance[*b].total_cmp(&forest.distance[*a]));

        let mut invert: Vec<f64> = (0..self.street_count() + self.ports.len())
            .map(|node| {
                self.port_of(node)
                    .and_then(|(_, level)| *level)
                    .unwrap_or_else(|| self.endpoint(node).1 - cover)
            })
            .collect();
        let mut segments = Vec::new();
        let mut pumps = Vec::new();

        for node in order {
            let (position, ground) = self.endpoint(node);
            if ground - invert[node] > max_depth {
                invert[node] = ground - cover;
                pumps.push(node);
            }
            let (target, edge) = match forest.next_hop[node] {
                Some((parent, edge)) => (parent, Some(edge)),
                None => (self.source(forest.owner[node]), None),
            };
            let (target_position, _) = self.endpoint(target);
            let fall = slope * position.distance(target_position).max(MIN_PIPE_RUN);
            if let Some((_, Some(level))) = self.port_of(target)
                && invert[node] - fall < *level
            {
                invert[node] = *level + fall;
                pumps.push(node);
            }
            let arrival = (invert[node] - fall).min(invert[target]);
            segments.push(Segment {
                a: node,
                b: target,
                pa: position,
                pb: target_position,
                za: invert[node],
                zb: arrival,
                depth: ground - invert[node],
                overhead: false,
                street_edge: edge,
            });
            invert[target] = arrival;
        }
        pumps.sort_unstable();
        pumps.dedup();
        self.finish(segments, pumps)
    }

    fn forest(&self, cost: impl Fn(usize, usize) -> f64) -> Forest {
        self.graph.shortest_forest(&self.entries, cost)
    }

    fn length(&self, a: usize, b: usize) -> f64 {
        self.layout().nodes[a]
            .pos
            .distance(self.layout().nodes[b].pos)
    }
}
