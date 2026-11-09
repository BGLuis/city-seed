use crate::environment::Environment;
use crate::geometry::{Point, centroid, distance_to_segment, edges, point};
use crate::networks::{
    Network, NetworkSpec, Topology, build_all, default_specs, reject_transit, validate_specs,
};
use crate::rng::{Rng, fnv1a};
use crate::sim::{EXPLOSION_MAX_DEPTH, EXPLOSION_RADIUS, Event, Grid, Simulation, Snapshot};
use crate::streets::{
    self, Lattice, ON_LINE_TOLERANCE, StreetClass, StreetEdge, StreetLayout, StreetNode,
    StreetOptions,
};
use crate::terrain::{Terrain, TerrainGrid, noise};
use crate::{
    CityContext, District, DistrictInput, GenError, Port, Profile, SimError, generate, regions,
    rivers, transit, validate_profile,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};
use std::fmt;

const MIN_POPULATION: u64 = 1_000;
const MAX_POPULATION: u64 = 40_000_000;
const GROSS_DENSITY_PER_SQUARE_METER: f64 = 0.003;
const DISTRICT_AREA: f64 = 3.5e6;
const SUB_CENTER_SPACING: f64 = 25_000.0;
const TRUNK_SLOPE_FACTOR: f64 = 0.2;
const TRUNK_DEPTH_FACTOR: f64 = 2.5;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CityInput {
    pub seed: u64,
    pub population: u64,
    #[serde(default)]
    pub region: Option<String>,
    #[serde(default)]
    pub profile: Profile,
    #[serde(default)]
    pub extra_networks: Vec<NetworkSpec>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DensityCenter {
    pub at: Point,
    pub radius: f64,
    pub weight: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DensityField {
    pub seed: u64,
    pub centers: Vec<DensityCenter>,
}

impl DensityField {
    fn new(seed: u64, side: f64) -> Self {
        let mut rng = Rng::derive(seed, "density");
        let middle = point(side / 2.0, side / 2.0);
        let mut centers = vec![DensityCenter {
            at: middle,
            radius: side * 0.14,
            weight: 1.0,
        }];
        for _ in 0..(side / SUB_CENTER_SPACING) as u32 {
            centers.push(DensityCenter {
                at: point(rng.range(0.1, 0.9) * side, rng.range(0.1, 0.9) * side),
                radius: side * rng.range(0.03, 0.07),
                weight: rng.range(0.5, 0.85),
            });
        }
        Self {
            seed: rng.next_u64(),
            centers,
        }
    }

    /// Rational falloff rather than a Gaussian: `exp` differs across platforms.
    pub fn at(&self, p: Point) -> f64 {
        let peak = self
            .centers
            .iter()
            .map(|center| {
                let ratio = p.distance(center.at) / center.radius;
                center.weight / (1.0 + ratio * ratio)
            })
            .fold(0.0, f64::max);
        let texture = 0.75 + 0.5 * noise(self.seed, p.x / 2_500.0, p.y / 2_500.0);
        (peak * texture).clamp(0.0, 1.0)
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct DistrictPort {
    pub network: String,
    pub node: usize,
    pub level: Option<f64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct DistrictPlan {
    pub id: usize,
    pub polygon: Vec<Point>,
    pub density: f64,
    pub ports: Vec<DistrictPort>,
}

#[derive(Clone, Debug, Serialize)]
pub struct City {
    pub seed: u64,
    pub population: u64,
    pub boundary: Vec<Point>,
    pub terrain: TerrainGrid,
    pub nodes: Vec<StreetNode>,
    pub edges: Vec<StreetEdge>,
    pub districts: Vec<DistrictPlan>,
    pub specs: Vec<NetworkSpec>,
    pub trunk_specs: Vec<NetworkSpec>,
    pub networks: Vec<Network>,
    pub profile: Profile,
    pub context: CityContext,
}

impl Grid for City {
    fn network_specs(&self) -> &[NetworkSpec] {
        &self.trunk_specs
    }

    fn networks(&self) -> &[Network] {
        &self.networks
    }

    fn consumer_count(&self) -> usize {
        self.districts.len()
    }

    fn consumer_node(&self, consumer: usize, network: usize) -> usize {
        self.districts[consumer].ports[network].node
    }

    fn consumer_hit(&self, _consumer: usize, _center: Point, _radius: f64) -> bool {
        false
    }
}

pub fn generate_city(input: &CityInput) -> Result<City, GenError> {
    if !(MIN_POPULATION..=MAX_POPULATION).contains(&input.population) {
        return Err(GenError::InvalidPopulation);
    }
    validate_profile(&input.profile)?;
    let seed = match &input.region {
        Some(id) => {
            regions::find(id).ok_or_else(|| GenError::UnknownRegion(id.clone()))?;
            Rng::derive(input.seed, id).next_u64()
        }
        None => input.seed,
    };
    let mut specs = default_specs(&input.profile);
    specs.extend(input.extra_networks.iter().cloned());
    reject_transit(&specs)?;

    let side = (input.population as f64 / GROSS_DENSITY_PER_SQUARE_METER).sqrt();
    let boundary = vec![
        point(0.0, 0.0),
        point(side, 0.0),
        point(side, side),
        point(0.0, side),
    ];
    let terrain = Terrain::new(seed, input.profile.relief);
    let density = DensityField::new(seed, side);
    let rivers = rivers::generate(seed, input.profile.rivers, side, &terrain);
    let environment = Environment::new(seed, &input.profile, &terrain, &rivers, Some(&density));
    let layout = streets::build(
        &boundary,
        &StreetOptions {
            lattice: Lattice {
                block_area: DISTRICT_AREA,
                shape: input.profile.block_shape,
                irregularity: input.profile.street_irregularity,
            },
            interior: StreetClass::Arterial,
            bridge_every: 1,
            rivers: &rivers,
        },
        &terrain,
        &mut Rng::derive(seed, "macro"),
    );

    let mut trunk_specs: Vec<NetworkSpec> = specs.iter().map(trunk_spec).collect();
    let ports = facility_ports(&trunk_specs, &layout, &density, side);
    let mut networks = build_all(
        &trunk_specs,
        &ports,
        &boundary,
        &layout,
        &environment,
        Rng::derive(seed, "trunk-overhead").next_u64(),
    );
    let lines = transit::build(
        &layout,
        &density,
        &environment,
        &input.profile,
        input.population,
        side,
        &mut Rng::derive(seed, "transit"),
    );
    for (spec, network) in lines.networks {
        trunk_specs.push(spec);
        networks.push(network);
    }
    validate_specs(&trunk_specs)?;
    let districts = plan_districts(&trunk_specs, &layout, &networks, &density);

    Ok(City {
        seed,
        population: input.population,
        terrain: terrain.grid(&boundary),
        boundary,
        nodes: layout.nodes,
        edges: layout.edges,
        districts,
        specs,
        trunk_specs,
        networks,
        profile: input.profile.clone(),
        context: CityContext {
            terrain_seed: seed,
            density,
            rivers,
            rails: lines.rails,
        },
    })
}

/// Trunk mains are larger than street pipes, so they run flatter and deeper.
fn trunk_spec(spec: &NetworkSpec) -> NetworkSpec {
    let mut trunk = spec.clone();
    if let Topology::Gravity { slope, max_depth } = trunk.topology {
        trunk.topology = Topology::Gravity {
            slope: slope * TRUNK_SLOPE_FACTOR,
            max_depth: max_depth * TRUNK_DEPTH_FACTOR,
        };
    }
    trunk
}

fn districts_per_facility(topology: &Topology) -> usize {
    match topology {
        Topology::Radial => 8,
        Topology::Looped { .. } => 25,
        Topology::Gravity { .. } => 20,
        Topology::Transit => unreachable!("transit has lines, not facilities"),
    }
}

/// Spreads facilities over a coarse grid and sites each where its physics wants
/// it: treatment plants low, pressurised reservoirs high, the rest near demand.
fn facility_ports(
    specs: &[NetworkSpec],
    layout: &StreetLayout,
    density: &DensityField,
    side: f64,
) -> Vec<Port> {
    let mut ports = Vec::new();
    for spec in specs {
        let wanted = (layout.blocks.len() / districts_per_facility(&spec.topology)).max(1);
        let cells_per_side = (wanted as f64).sqrt().ceil();
        let mut cells: BTreeMap<(u32, u32), Vec<usize>> = BTreeMap::new();
        for (index, node) in layout.nodes.iter().enumerate() {
            let cell = |value: f64| {
                ((value / side * cells_per_side) as u32).min(cells_per_side as u32 - 1)
            };
            cells
                .entry((cell(node.pos.x), cell(node.pos.y)))
                .or_default()
                .push(index);
        }
        for nodes in cells.values() {
            let score = |node: usize| match spec.topology {
                Topology::Gravity { .. } => -layout.nodes[node].elevation,
                Topology::Looped {
                    booster_head: Some(_),
                } => layout.nodes[node].elevation,
                _ => density.at(layout.nodes[node].pos),
            };
            let site = nodes
                .iter()
                .copied()
                .max_by(|a, b| score(*a).total_cmp(&score(*b)))
                .expect("cells are never empty");
            ports.push(Port {
                network: spec.id.clone(),
                at: layout.nodes[site].pos,
                level: None,
            });
        }
    }
    ports
}

fn plan_districts(
    specs: &[NetworkSpec],
    layout: &StreetLayout,
    networks: &[Network],
    density: &DensityField,
) -> Vec<DistrictPlan> {
    let levels: Vec<BTreeMap<usize, f64>> = networks
        .iter()
        .zip(specs)
        .map(|(network, spec)| match spec.topology {
            Topology::Gravity { .. } => network.segments.iter().map(|s| (s.a, s.za)).collect(),
            _ => BTreeMap::new(),
        })
        .collect();

    layout
        .blocks
        .iter()
        .enumerate()
        .map(|(id, polygon)| {
            let mut on_boundary: Vec<usize> = edges(polygon)
                .flat_map(|(a, b)| layout.index.on_segment(a, b, ON_LINE_TOLERANCE))
                .collect();
            on_boundary.sort_unstable();
            on_boundary.dedup();
            let elevation = |node: &usize| layout.nodes[*node].elevation;
            let ports = specs
                .iter()
                .enumerate()
                .map(|(index, spec)| {
                    if spec.topology == Topology::Transit {
                        return DistrictPort {
                            network: spec.id.clone(),
                            node: nearest_station(&networks[index], layout, centroid(polygon)),
                            level: None,
                        };
                    }
                    let chosen = on_boundary.iter().copied().reduce(|best, node| {
                        let lower = elevation(&node) < elevation(&best);
                        let wants_low = matches!(spec.topology, Topology::Gravity { .. });
                        if lower == wants_low { node } else { best }
                    });
                    let node = chosen.expect("district corners are street nodes");
                    DistrictPort {
                        network: spec.id.clone(),
                        node,
                        level: levels[index].get(&node).copied(),
                    }
                })
                .collect();
            DistrictPlan {
                id,
                polygon: polygon.clone(),
                density: density.at(centroid(polygon)),
                ports,
            }
        })
        .collect()
}

fn nearest_station(line: &Network, layout: &StreetLayout, p: Point) -> usize {
    line.powered_nodes
        .iter()
        .copied()
        .min_by(|a, b| {
            layout.nodes[*a]
                .pos
                .distance(p)
                .total_cmp(&layout.nodes[*b].pos.distance(p))
        })
        .expect("lines have stations")
}

pub fn district_input(city: &City, id: usize) -> Result<DistrictInput, GenError> {
    let plan = city
        .districts
        .get(id)
        .ok_or(GenError::UnknownDistrict(id))?;
    let mut key = city.seed.to_le_bytes().to_vec();
    key.extend_from_slice(&(id as u64).to_le_bytes());
    Ok(DistrictInput {
        seed: Rng::new(fnv1a(&key)).next_u64(),
        boundary: plan.polygon.clone(),
        ports: plan
            .ports
            .iter()
            .take(city.specs.len())
            .map(|port| Port {
                network: port.network.clone(),
                at: city.nodes[port.node].pos,
                level: port.level,
            })
            .collect(),
        profile: Profile {
            density: plan.density,
            ..city.profile.clone()
        },
        networks: Some(city.specs.clone()),
        extra_networks: Vec::new(),
        context: Some(city.context.clone()),
    })
}

pub fn generate_district(city: &City, id: usize) -> Result<District, GenError> {
    generate(&district_input(city, id)?)
}

pub fn city_hash(city: &City) -> u64 {
    fnv1a(
        serde_json::to_string(city)
            .expect("city serializes")
            .as_bytes(),
    )
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum CityEvent {
    Destroy {
        center: Point,
        radius: f64,
        max_depth: f64,
    },
    CutTrunk {
        network: String,
        segment: usize,
    },
    CutLocal {
        district: usize,
        network: String,
        segment: usize,
    },
}

#[derive(Clone, Debug, Serialize)]
pub struct CitySnapshot {
    pub trunk: Snapshot,
    pub districts: BTreeMap<usize, Snapshot>,
}

#[derive(Debug, PartialEq)]
pub enum CityError {
    Generation(GenError),
    Simulation(SimError),
}

impl From<GenError> for CityError {
    fn from(error: GenError) -> Self {
        Self::Generation(error)
    }
}

impl From<SimError> for CityError {
    fn from(error: SimError) -> Self {
        Self::Simulation(error)
    }
}

impl fmt::Display for CityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Generation(error) => error.fmt(f),
            Self::Simulation(error) => error.fmt(f),
        }
    }
}

struct Detailed {
    district: District,
    simulation: Simulation,
}

#[derive(Clone, Copy, PartialEq)]
enum Origin {
    Outside,
    Trunk,
    District(usize),
}

struct Blast {
    origin: Origin,
    center: Point,
    radius: f64,
    max_depth: f64,
}

pub struct CitySession {
    city: City,
    trunk: Simulation,
    detailed: BTreeMap<usize, Detailed>,
}

impl CitySession {
    pub fn new(city: City) -> Self {
        let trunk = Simulation::new(&city);
        Self {
            city,
            trunk,
            detailed: BTreeMap::new(),
        }
    }

    pub fn city(&self) -> &City {
        &self.city
    }

    pub fn detail(&mut self, id: usize) -> Result<&District, GenError> {
        if !self.detailed.contains_key(&id) {
            let district = generate_district(&self.city, id)?;
            let simulation = Simulation::new(&district);
            self.detailed.insert(
                id,
                Detailed {
                    district,
                    simulation,
                },
            );
            self.sync_supply();
        }
        Ok(&self.detailed[&id].district)
    }

    pub fn reset(&mut self) -> CitySnapshot {
        self.trunk = Simulation::new(&self.city);
        for detailed in self.detailed.values_mut() {
            detailed.simulation = Simulation::new(&detailed.district);
        }
        self.snapshot()
    }

    pub fn snapshot(&self) -> CitySnapshot {
        CitySnapshot {
            trunk: self.trunk.snapshot(&self.city),
            districts: self
                .detailed
                .iter()
                .map(|(id, detailed)| (*id, detailed.simulation.snapshot(&detailed.district)))
                .collect(),
        }
    }

    pub fn apply(&mut self, event: &CityEvent) -> Result<CitySnapshot, CityError> {
        let mut blasts = VecDeque::new();
        match event {
            CityEvent::Destroy {
                center,
                radius,
                max_depth,
            } => blasts.push_back(Blast {
                origin: Origin::Outside,
                center: *center,
                radius: *radius,
                max_depth: *max_depth,
            }),
            CityEvent::CutTrunk { network, segment } => {
                let cut = Event::Cut {
                    network: network.clone(),
                    segment: *segment,
                };
                self.apply_to_trunk(&cut, &mut blasts)?;
            }
            CityEvent::CutLocal {
                district,
                network,
                segment,
            } => {
                let cut = Event::Cut {
                    network: network.clone(),
                    segment: *segment,
                };
                self.apply_to_district(*district, &cut, &mut blasts)?;
            }
        }

        // Explosions can spill across districts and into the trunks. Every leak
        // ignites at most once, so this queue always drains.
        while let Some(blast) = blasts.pop_front() {
            let strike = Event::Destroy {
                center: blast.center,
                radius: blast.radius,
                max_depth: blast.max_depth,
            };
            // Districts go first: their ruptures must see the supply that was
            // live before this blast cut the trunks feeding them.
            for id in self.districts_touching(blast.center, blast.radius) {
                if blast.origin != Origin::District(id) {
                    self.apply_to_district(id, &strike, &mut blasts)?;
                }
            }
            if blast.origin != Origin::Trunk {
                self.apply_to_trunk(&strike, &mut blasts)?;
            }
        }
        self.sync_supply();
        Ok(self.snapshot())
    }

    fn apply_to_trunk(
        &mut self,
        event: &Event,
        blasts: &mut VecDeque<Blast>,
    ) -> Result<(), SimError> {
        let before = self.trunk.hazard_count();
        self.trunk.run(&self.city, event)?;
        queue_explosions(&self.trunk, before, Origin::Trunk, blasts);
        Ok(())
    }

    fn apply_to_district(
        &mut self,
        id: usize,
        event: &Event,
        blasts: &mut VecDeque<Blast>,
    ) -> Result<(), CityError> {
        self.detail(id)?;
        let detailed = self.detailed.get_mut(&id).expect("detailed above");
        let before = detailed.simulation.hazard_count();
        detailed.simulation.run(&detailed.district, event)?;
        queue_explosions(&detailed.simulation, before, Origin::District(id), blasts);
        Ok(())
    }

    fn districts_touching(&self, center: Point, radius: f64) -> Vec<usize> {
        self.city
            .districts
            .iter()
            .filter(|plan| circle_touches_polygon(center, radius, &plan.polygon))
            .map(|plan| plan.id)
            .collect()
    }

    fn sync_supply(&mut self) {
        let served = self.trunk.service(&self.city);
        for (id, detailed) in &mut self.detailed {
            let local_networks = detailed.district.networks.len();
            let ports = self.city.districts[*id].ports.iter().take(local_networks);
            for (network, port) in ports.enumerate() {
                detailed
                    .simulation
                    .set_supply(network, served[network][port.node]);
            }
        }
    }
}

fn queue_explosions(
    simulation: &Simulation,
    before: usize,
    origin: Origin,
    blasts: &mut VecDeque<Blast>,
) {
    for center in simulation.explosions_since(before) {
        blasts.push_back(Blast {
            origin,
            center,
            radius: EXPLOSION_RADIUS,
            max_depth: EXPLOSION_MAX_DEPTH,
        });
    }
}

fn circle_touches_polygon(center: Point, radius: f64, polygon: &[Point]) -> bool {
    let inside = edges(polygon).all(|(a, b)| (b - a).cross(center - a) >= 0.0);
    inside || edges(polygon).any(|(a, b)| distance_to_segment(center, a, b) <= radius)
}
