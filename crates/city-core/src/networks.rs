use crate::geometry::{Point, centroid};
use crate::streets::StreetLayout;
use crate::terrain::{Terrain, noise};
use crate::{GenError, Port, Profile};
use serde::{Deserialize, Serialize};

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
    pub source: usize,
    pub node_count: usize,
    pub segments: Vec<Segment>,
    pub powered_nodes: Vec<usize>,
}

const MIN_DEPTH_CLEARANCE: f64 = 0.2;
const TRUNK_OUTWARD_DISTANCE: f64 = 40.0;
const UPHILL_FLOW_PENALTY: f64 = 40.0;

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
        (0..self.layout.nodes.len())
            .min_by(|a, b| {
                let da = self.layout.nodes[*a].pos.distance(p);
                let db = self.layout.nodes[*b].pos.distance(p);
                da.total_cmp(&db)
            })
            .expect("street graph has nodes")
    }

    /// Returns each node's cost to `root` and its next hop towards it.
    fn shortest_tree(
        &self,
        root: usize,
        cost: impl Fn(usize, usize) -> f64,
    ) -> (Vec<f64>, Vec<Option<(usize, usize)>>) {
        let count = self.layout.nodes.len();
        let mut distance = vec![f64::MAX; count];
        let mut next_hop = vec![None; count];
        let mut settled = vec![false; count];
        distance[root] = 0.0;
        for _ in 0..count {
            let Some(current) = (0..count)
                .filter(|n| !settled[*n] && distance[*n] < f64::MAX)
                .min_by(|a, b| distance[*a].total_cmp(&distance[*b]))
            else {
                break;
            };
            settled[current] = true;
            for &(neighbor, edge) in &self.adjacency[current] {
                let candidate = distance[current] + cost(neighbor, current);
                if candidate < distance[neighbor] {
                    distance[neighbor] = candidate;
                    next_hop[neighbor] = Some((current, edge));
                }
            }
        }
        (distance, next_hop)
    }
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
            let port = ports
                .iter()
                .find(|port| port.network == spec.id)
                .map_or_else(|| port_for(spec, boundary, terrain), |port| port.at);
            let builder = Builder {
                spec,
                port,
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
    port: Point,
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

    fn source(&self) -> usize {
        self.layout().nodes.len()
    }

    fn entry(&self) -> usize {
        self.graph.nearest_node(self.port)
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
        if node == self.source() {
            (self.port, self.terrain.height(self.port))
        } else {
            let street_node = &self.layout().nodes[node];
            (street_node.pos, street_node.elevation)
        }
    }

    fn finish(&self, segments: Vec<Segment>, powered_nodes: Vec<usize>) -> Network {
        Network {
            id: self.spec.id.clone(),
            source: self.source(),
            node_count: self.source() + 1,
            segments,
            powered_nodes,
        }
    }

    fn radial(&self) -> Network {
        let (_, next_hop) = self.tree(|neighbor, current| self.length(neighbor, current));
        let mut segments = vec![self.flat_segment(self.source(), self.entry(), None)];
        for (node, hop) in next_hop.iter().enumerate() {
            if let Some((parent, edge)) = hop {
                segments.push(self.flat_segment(*parent, node, Some(*edge)));
            }
        }
        self.finish(segments, Vec::new())
    }

    fn looped(&self, booster_head: Option<f64>) -> Network {
        let mut segments = vec![self.flat_segment(self.source(), self.entry(), None)];
        for (index, edge) in self.layout().edges.iter().enumerate() {
            segments.push(self.flat_segment(edge.a, edge.b, Some(index)));
        }
        let source_ground = self.terrain.height(self.port);
        let powered_nodes = booster_head.map_or_else(Vec::new, |head| {
            (0..self.layout().nodes.len())
                .filter(|node| self.layout().nodes[*node].elevation > source_ground - head)
                .collect()
        });
        self.finish(segments, powered_nodes)
    }

    fn gravity(&self, slope: f64, max_depth: f64) -> Network {
        let (distance, next_hop) = self.tree(|neighbor, current| {
            let rise =
                self.layout().nodes[current].elevation - self.layout().nodes[neighbor].elevation;
            self.length(neighbor, current) + UPHILL_FLOW_PENALTY * rise.max(0.0)
        });
        let cover = self.spec.depth;
        let mut order: Vec<usize> = (0..self.layout().nodes.len()).collect();
        order.sort_by(|a, b| distance[*b].total_cmp(&distance[*a]));

        let mut invert: Vec<f64> = (0..=self.source())
            .map(|node| self.endpoint(node).1 - cover)
            .collect();
        let mut segments = Vec::new();
        let mut pumps = Vec::new();

        for node in order {
            let (position, ground) = self.endpoint(node);
            if ground - invert[node] > max_depth {
                invert[node] = ground - cover;
                pumps.push(node);
            }
            let (target, edge) = match next_hop[node] {
                Some((parent, edge)) => (parent, Some(edge)),
                None => (self.source(), None),
            };
            let (target_position, _) = self.endpoint(target);
            let arrival =
                (invert[node] - slope * position.distance(target_position)).min(invert[target]);
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
        self.finish(segments, pumps)
    }

    fn tree(&self, cost: impl Fn(usize, usize) -> f64) -> (Vec<f64>, Vec<Option<(usize, usize)>>) {
        self.graph.shortest_tree(self.entry(), cost)
    }

    fn length(&self, a: usize, b: usize) -> f64 {
        self.layout().nodes[a]
            .pos
            .distance(self.layout().nodes[b].pos)
    }
}
