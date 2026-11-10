pub mod city;
pub mod environment;
pub mod geometry;
pub mod mesh;
pub mod networks;
pub mod parcels;
pub mod regions;
pub mod rivers;
pub mod rng;
pub mod sim;
pub mod spatial;
pub mod streets;
pub mod terrain;
pub mod transit;

use city::DensityField;
use environment::Environment;
use geometry::{Point, is_convex_ccw, point, signed_area};
use networks::{Network, NetworkSpec, default_specs, reject_transit, validate_specs};
use parcels::{Building, Corridors, Lot};
use rng::{Rng, fnv1a};
use serde::{Deserialize, Serialize};
use std::fmt;
use streets::{Lattice, StreetClass, StreetEdge, StreetNode, StreetOptions};
use terrain::{Terrain, TerrainGrid};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub overhead_share: f64,
    pub piped_gas: bool,
    pub park_share: f64,
    pub density: f64,
    /// 0 is a rectangular grid, 1 a honeycomb, anything between is a blend.
    pub block_shape: f64,
    pub street_irregularity: f64,
    pub rivers: u32,
    pub metro_density: f64,
    pub rail_density: f64,
    pub mean_temperature: f64,
    pub annual_rainfall: f64,
    pub bedrock_depth: f64,
    pub seismic_risk: f64,
    pub relief: f64,
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            overhead_share: 0.5,
            piped_gas: true,
            park_share: 0.08,
            density: 0.5,
            block_shape: 0.0,
            street_irregularity: 0.3,
            rivers: 0,
            metro_density: 0.0,
            rail_density: 0.0,
            mean_temperature: 20.0,
            annual_rainfall: 1200.0,
            bedrock_depth: 15.0,
            seismic_risk: 0.1,
            relief: 1.0,
        }
    }
}

/// Where a network enters the district. The macro level will set these so
/// neighbouring districts meet the same trunk lines.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Port {
    pub network: String,
    pub at: Point,
    /// Pipe level at the port, for gravity networks joining an existing trunk.
    #[serde(default)]
    pub level: Option<f64>,
}

/// City-wide fields a district must share with its neighbours.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CityContext {
    pub terrain_seed: u64,
    pub density: DensityField,
    pub rivers: Vec<Vec<Point>>,
    pub rails: Vec<Vec<Point>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DistrictInput {
    pub seed: u64,
    pub boundary: Vec<Point>,
    #[serde(default)]
    pub ports: Vec<Port>,
    #[serde(default)]
    pub profile: Profile,
    #[serde(default)]
    pub networks: Option<Vec<NetworkSpec>>,
    #[serde(default)]
    pub extra_networks: Vec<NetworkSpec>,
    #[serde(default)]
    pub context: Option<CityContext>,
}

impl DistrictInput {
    pub fn square(seed: u64, size: f64) -> Self {
        Self {
            seed,
            boundary: vec![
                point(0.0, 0.0),
                point(size, 0.0),
                point(size, size),
                point(0.0, size),
            ],
            ports: Vec::new(),
            profile: Profile::default(),
            networks: None,
            extra_networks: Vec::new(),
            context: None,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct District {
    pub seed: u64,
    pub boundary: Vec<Point>,
    pub terrain: TerrainGrid,
    pub street_nodes: Vec<StreetNode>,
    pub street_edges: Vec<StreetEdge>,
    pub lots: Vec<Lot>,
    pub buildings: Vec<Building>,
    pub specs: Vec<NetworkSpec>,
    pub networks: Vec<Network>,
}

#[derive(Debug, PartialEq)]
pub enum GenError {
    InvalidBoundary,
    InvalidPopulation,
    InvalidProfile,
    UnknownRegion(String),
    UnknownDistrict(usize),
    TransitNotConfigurable(String),
    DuplicateNetwork(String),
    UnknownDependency { network: String, requires: String },
    DepthConflict(String, String),
}

#[derive(Debug, PartialEq)]
pub enum SimError {
    UnknownNetwork(String),
    UnknownSegment(usize),
}

impl fmt::Display for GenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GenError::InvalidBoundary => {
                write!(
                    f,
                    "boundary must be a convex CCW polygon of 1 ha to 400 km²"
                )
            }
            GenError::InvalidPopulation => {
                write!(f, "population must be between 1 thousand and 40 million")
            }
            GenError::InvalidProfile => {
                write!(
                    f,
                    "profile has an out-of-range block, climate or geology value"
                )
            }
            GenError::UnknownRegion(id) => write!(f, "unknown region '{id}'"),
            GenError::UnknownDistrict(id) => write!(f, "unknown district {id}"),
            GenError::TransitNotConfigurable(id) => {
                write!(
                    f,
                    "network '{id}': transit lines are routed by the city generator"
                )
            }
            GenError::DuplicateNetwork(id) => write!(f, "duplicate network '{id}'"),
            GenError::UnknownDependency { network, requires } => {
                write!(
                    f,
                    "network '{network}' requires '{requires}', which must be declared before it"
                )
            }
            GenError::DepthConflict(a, b) => {
                write!(
                    f,
                    "networks '{a}' and '{b}' are too close in depth to cross"
                )
            }
        }
    }
}

impl fmt::Display for SimError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SimError::UnknownNetwork(id) => write!(f, "unknown network '{id}'"),
            SimError::UnknownSegment(index) => write!(f, "unknown segment {index}"),
        }
    }
}

const MIN_DISTRICT_AREA: f64 = 1.0e4;
const MAX_DISTRICT_AREA: f64 = 4.0e8;
const BASE_BLOCK_AREA: f64 = 16_000.0;
const LOCAL_BRIDGE_SPACING: usize = 5;

pub fn validate_profile(profile: &Profile) -> Result<(), GenError> {
    let unit = 0.0..=1.0;
    if unit.contains(&profile.block_shape)
        && unit.contains(&profile.street_irregularity)
        && unit.contains(&profile.seismic_risk)
        && profile.annual_rainfall >= 0.0
        && profile.bedrock_depth > 0.0
        && profile.relief >= 0.0
    {
        Ok(())
    } else {
        Err(GenError::InvalidProfile)
    }
}

pub fn generate(input: &DistrictInput) -> Result<District, GenError> {
    let area = signed_area(&input.boundary);
    if input.boundary.len() < 3
        || !is_convex_ccw(&input.boundary)
        || !(MIN_DISTRICT_AREA..=MAX_DISTRICT_AREA).contains(&area)
    {
        return Err(GenError::InvalidBoundary);
    }

    let mut specs = input
        .networks
        .clone()
        .unwrap_or_else(|| default_specs(&input.profile));
    specs.extend(input.extra_networks.iter().cloned());
    validate_specs(&specs)?;
    reject_transit(&specs)?;
    validate_profile(&input.profile)?;

    let context = input.context.as_ref();
    let city_seed = context.map_or(input.seed, |c| c.terrain_seed);
    let terrain = Terrain::new(city_seed, input.profile.relief);
    let block_area = BASE_BLOCK_AREA * (1.4 - 0.8 * input.profile.density);
    let corridors = Corridors {
        rivers: context.map_or(&[], |c| &c.rivers),
        rails: context.map_or(&[], |c| &c.rails),
    };
    let environment = Environment::new(
        city_seed,
        &input.profile,
        &terrain,
        corridors.rivers,
        context.map(|c| &c.density),
    );
    let layout = streets::build(
        &input.boundary,
        &StreetOptions {
            lattice: Lattice {
                block_area,
                shape: input.profile.block_shape,
                irregularity: input.profile.street_irregularity,
            },
            interior: StreetClass::Local,
            bridge_every: LOCAL_BRIDGE_SPACING,
            rivers: corridors.rivers,
        },
        &terrain,
        &mut Rng::derive(input.seed, "streets"),
    );
    let (lots, buildings) = parcels::subdivide(
        &input.boundary,
        &layout,
        &input.profile,
        context.map(|c| &c.density),
        &corridors,
        &environment,
        &mut Rng::derive(input.seed, "parcels"),
    );
    let networks = networks::build_all(
        &specs,
        &input.ports,
        &input.boundary,
        &layout,
        &environment,
        Rng::derive(input.seed, "overhead").next_u64(),
    );

    Ok(District {
        seed: input.seed,
        boundary: input.boundary.clone(),
        terrain: terrain.grid(&input.boundary),
        street_nodes: layout.nodes,
        street_edges: layout.edges,
        lots,
        buildings,
        specs,
        networks,
    })
}

pub fn district_hash(district: &District) -> u64 {
    let serialized = serde_json::to_string(district).expect("district serializes");
    fnv1a(serialized.as_bytes())
}
