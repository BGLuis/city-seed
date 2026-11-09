use crate::geometry::{Point, distance_to_segment};
use crate::networks::RuptureEffect;
use crate::{District, SimError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Event {
    Cut {
        network: String,
        segment: usize,
    },
    Destroy {
        center: Point,
        radius: f64,
        max_depth: f64,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HazardKind {
    Flood,
    GasLeak,
    LiveCable,
    Contamination,
    ElectricRisk,
    Explosion,
}

#[derive(Clone, Debug, Serialize)]
pub struct Hazard {
    pub kind: HazardKind,
    pub at: Point,
}

#[derive(Clone, Debug, Serialize)]
pub struct Snapshot {
    pub broken: BTreeMap<String, Vec<usize>>,
    pub destroyed: Vec<usize>,
    pub outages: BTreeMap<String, Vec<usize>>,
    pub hazards: Vec<Hazard>,
}

const FLOOD_CABLE_RADIUS: f64 = 8.0;
const IGNITION_RADIUS: f64 = 15.0;
const EXPLOSION_RADIUS: f64 = 25.0;
const EXPLOSION_MAX_DEPTH: f64 = 3.0;

#[derive(Clone)]
pub struct Simulation {
    broken: Vec<Vec<bool>>,
    destroyed: Vec<bool>,
    hazards: Vec<Hazard>,
    reacted: Vec<bool>,
}

impl Simulation {
    pub fn new(district: &District) -> Self {
        Self {
            broken: district
                .networks
                .iter()
                .map(|network| vec![false; network.segments.len()])
                .collect(),
            destroyed: vec![false; district.buildings.len()],
            hazards: Vec::new(),
            reacted: Vec::new(),
        }
    }

    pub fn apply(&mut self, district: &District, event: &Event) -> Result<Snapshot, SimError> {
        let mut pending = match event {
            Event::Cut { network, segment } => {
                let index = district
                    .networks
                    .iter()
                    .position(|n| &n.id == network)
                    .ok_or_else(|| SimError::UnknownNetwork(network.clone()))?;
                if *segment >= district.networks[index].segments.len() {
                    return Err(SimError::UnknownSegment(*segment));
                }
                vec![(index, *segment)]
            }
            Event::Destroy {
                center,
                radius,
                max_depth,
            } => self.blast(district, *center, *radius, *max_depth),
        };

        // Every segment breaks and every hazard reacts at most once, so the
        // cascade is bounded by the size of the district and always terminates.
        loop {
            self.rupture(district, &pending);
            let electrified = self.electrify();
            let explosions = self.ignite(district);
            let exploded = !explosions.is_empty();
            pending = explosions.into_iter().flatten().collect();
            if pending.is_empty() && !electrified && !exploded {
                break;
            }
        }
        Ok(self.snapshot(district))
    }

    pub fn snapshot(&self, district: &District) -> Snapshot {
        let served = self.service(district);
        let mut broken = BTreeMap::new();
        let mut outages = BTreeMap::new();
        for (index, network) in district.networks.iter().enumerate() {
            let segments = self.broken[index]
                .iter()
                .enumerate()
                .filter(|(_, is_broken)| **is_broken)
                .map(|(segment, _)| segment)
                .collect();
            broken.insert(network.id.clone(), segments);
            let lost = district
                .buildings
                .iter()
                .enumerate()
                .filter(|(b, building)| !self.destroyed[*b] && !served[index][building.street_node])
                .map(|(b, _)| b)
                .collect();
            outages.insert(network.id.clone(), lost);
        }
        Snapshot {
            broken,
            destroyed: self
                .destroyed
                .iter()
                .enumerate()
                .filter(|(_, is_destroyed)| **is_destroyed)
                .map(|(b, _)| b)
                .collect(),
            outages,
            hazards: self.hazards.clone(),
        }
    }

    fn service(&self, district: &District) -> Vec<Vec<bool>> {
        let mut served: Vec<Vec<bool>> = Vec::with_capacity(district.networks.len());
        for (index, network) in district.networks.iter().enumerate() {
            let mut gated = vec![false; network.node_count];
            if let Some(required) = &district.specs[index].requires {
                let dependency = district
                    .specs
                    .iter()
                    .position(|spec| &spec.id == required)
                    .expect("validated at generation");
                for node in &network.powered_nodes {
                    gated[*node] = !served[dependency][*node];
                }
            }
            let mut adjacency = vec![Vec::new(); network.node_count];
            for (segment_index, segment) in network.segments.iter().enumerate() {
                if !self.broken[index][segment_index] {
                    adjacency[segment.a].push(segment.b);
                    adjacency[segment.b].push(segment.a);
                }
            }
            let mut reached = vec![false; network.node_count];
            reached[network.source] = true;
            let mut stack = vec![network.source];
            while let Some(node) = stack.pop() {
                for &next in &adjacency[node] {
                    if !reached[next] && !gated[next] {
                        reached[next] = true;
                        stack.push(next);
                    }
                }
            }
            served.push(reached);
        }
        served
    }

    fn blast(
        &mut self,
        district: &District,
        center: Point,
        radius: f64,
        max_depth: f64,
    ) -> Vec<(usize, usize)> {
        for (index, building) in district.buildings.iter().enumerate() {
            if building
                .footprint
                .iter()
                .any(|p| p.distance(center) <= radius)
            {
                self.destroyed[index] = true;
            }
        }
        let mut hits = Vec::new();
        for (network_index, network) in district.networks.iter().enumerate() {
            for (segment_index, segment) in network.segments.iter().enumerate() {
                if segment.depth <= max_depth
                    && distance_to_segment(center, segment.pa, segment.pb) <= radius
                {
                    hits.push((network_index, segment_index));
                }
            }
        }
        hits
    }

    fn rupture(&mut self, district: &District, pending: &[(usize, usize)]) {
        if pending.is_empty() {
            return;
        }
        // Supply is read before any of this round's breaks, otherwise a large
        // blast would isolate its own pipes and suppress their leaks.
        let supplied = self.service(district);
        for &(network_index, segment_index) in pending {
            if self.broken[network_index][segment_index] {
                continue;
            }
            self.broken[network_index][segment_index] = true;
            let segment = &district.networks[network_index].segments[segment_index];
            if !supplied[network_index][segment.a] && !supplied[network_index][segment.b] {
                continue;
            }
            let at = (segment.pa + segment.pb) * 0.5;
            for effect in &district.specs[network_index].on_rupture {
                match effect {
                    RuptureEffect::Flood => self.raise(HazardKind::Flood, at),
                    RuptureEffect::GasLeak => self.raise(HazardKind::GasLeak, at),
                    RuptureEffect::Contamination => self.raise(HazardKind::Contamination, at),
                    RuptureEffect::LiveCable => {
                        self.raise(HazardKind::LiveCable, at);
                        if segment.overhead {
                            self.raise(HazardKind::ElectricRisk, at);
                        }
                    }
                }
            }
        }
    }

    fn raise(&mut self, kind: HazardKind, at: Point) {
        self.hazards.push(Hazard { kind, at });
        self.reacted.push(false);
    }

    fn electrify(&mut self) -> bool {
        let floods: Vec<Point> = self.of_kind(HazardKind::Flood);
        let mut changed = false;
        for index in 0..self.hazards.len() {
            if self.hazards[index].kind != HazardKind::LiveCable || self.reacted[index] {
                continue;
            }
            let at = self.hazards[index].at;
            if floods.iter().any(|f| f.distance(at) <= FLOOD_CABLE_RADIUS) {
                self.reacted[index] = true;
                self.raise(HazardKind::ElectricRisk, at);
                changed = true;
            }
        }
        changed
    }

    fn ignite(&mut self, district: &District) -> Vec<Vec<(usize, usize)>> {
        let mut sources = self.of_kind(HazardKind::ElectricRisk);
        sources.extend(self.of_kind(HazardKind::Explosion));
        let mut explosions = Vec::new();
        for index in 0..self.hazards.len() {
            if self.hazards[index].kind != HazardKind::GasLeak || self.reacted[index] {
                continue;
            }
            let at = self.hazards[index].at;
            if sources.iter().any(|s| s.distance(at) <= IGNITION_RADIUS) {
                self.reacted[index] = true;
                self.raise(HazardKind::Explosion, at);
                explosions.push(self.blast(district, at, EXPLOSION_RADIUS, EXPLOSION_MAX_DEPTH));
            }
        }
        explosions
    }

    fn of_kind(&self, kind: HazardKind) -> Vec<Point> {
        self.hazards
            .iter()
            .filter(|hazard| hazard.kind == kind)
            .map(|hazard| hazard.at)
            .collect()
    }
}
