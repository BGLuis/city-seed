use crate::Profile;
use crate::city::DensityField;
use crate::geometry::{Point, distance_to_segment, point};
use crate::rng::Rng;
use crate::terrain::{Terrain, noise};
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Soil {
    Rocky,
    Clay,
    Sand,
    Alluvial,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Sample {
    pub elevation: f64,
    pub slope: f64,
    pub rock_depth: f64,
    pub soil: Soil,
    pub temperature: f64,
    pub rainfall: f64,
    pub vegetation: f64,
    pub flood_risk: f64,
}

const SLOPE_PROBE: f64 = 10.0;
const ALLUVIAL_WIDTH: f64 = 500.0;
const SHALLOW_ROCK: f64 = 3.0;
const LAPSE_RATE: f64 = 0.0065;
const HEAT_ISLAND: f64 = 2.5;
const OROGRAPHIC_RAIN: f64 = 2.0;
const IDEAL_TEMPERATURE: f64 = 20.0;
const TEMPERATURE_TOLERANCE: f64 = 25.0;
const LUSH_RAINFALL: f64 = 1500.0;
const FLOOD_REACH: f64 = 1500.0;
const FLOOD_RISE: f64 = 15.0;
const VALLEY_FLOOR: f64 = 20.0;
const REFERENCE_RAINFALL: f64 = 1500.0;

/// City-wide natural conditions. Every district samples the same fields, so
/// geology and climate stay continuous across district borders.
pub struct Environment<'a> {
    seed: u64,
    profile: &'a Profile,
    terrain: &'a Terrain,
    rivers: &'a [Vec<Point>],
    density: Option<&'a DensityField>,
}

impl<'a> Environment<'a> {
    pub fn new(
        city_seed: u64,
        profile: &'a Profile,
        terrain: &'a Terrain,
        rivers: &'a [Vec<Point>],
        density: Option<&'a DensityField>,
    ) -> Self {
        Self {
            seed: Rng::derive(city_seed, "environment").next_u64(),
            profile,
            terrain,
            rivers,
            density,
        }
    }

    pub fn terrain(&self) -> &'a Terrain {
        self.terrain
    }

    pub fn profile(&self) -> &'a Profile {
        self.profile
    }

    pub fn rock_depth(&self, p: Point) -> f64 {
        self.profile.bedrock_depth * (0.3 + 1.4 * noise(self.seed, p.x / 1_800.0, p.y / 1_800.0))
    }

    fn river_distance(&self, p: Point) -> Option<f64> {
        self.rivers
            .iter()
            .flat_map(|river| river.windows(2))
            .map(|piece| distance_to_segment(p, piece[0], piece[1]))
            .min_by(f64::total_cmp)
    }

    pub fn soil(&self, p: Point) -> Soil {
        let near_river = self
            .river_distance(p)
            .is_some_and(|distance| distance < ALLUVIAL_WIDTH);
        if near_river {
            Soil::Alluvial
        } else if self.rock_depth(p) < SHALLOW_ROCK {
            Soil::Rocky
        } else if self.soil_texture(p) < 0.5 {
            Soil::Clay
        } else {
            Soil::Sand
        }
    }

    /// Two octaves: thresholding a single one of value noise leaves straight
    /// edges along its lattice.
    fn soil_texture(&self, p: Point) -> f64 {
        0.7 * noise(self.seed ^ 1, p.x / 2_500.0, p.y / 2_500.0)
            + 0.3 * noise(self.seed ^ 4, p.x / 700.0, p.y / 700.0)
    }

    fn slope(&self, p: Point) -> f64 {
        let dx = self.terrain.height(p + point(SLOPE_PROBE, 0.0))
            - self.terrain.height(p - point(SLOPE_PROBE, 0.0));
        let dy = self.terrain.height(p + point(0.0, SLOPE_PROBE))
            - self.terrain.height(p - point(0.0, SLOPE_PROBE));
        point(dx, dy).length() / (2.0 * SLOPE_PROBE)
    }

    /// Compares the ground with the local valley floor rather than with the
    /// nearest bank, whose height jumps wherever the closest bank changes.
    fn flood_risk(&self, p: Point, elevation: f64, rainfall: f64) -> f64 {
        self.river_distance(p).map_or(0.0, |distance| {
            let closeness = (1.0 - distance / FLOOD_REACH).clamp(0.0, 1.0);
            let local_height = elevation - self.terrain.regional_height(p);
            let rise = local_height - VALLEY_FLOOR * self.terrain.relief();
            let lowness = (1.0 - rise / FLOOD_RISE).clamp(0.0, 1.0);
            (closeness * lowness * (rainfall / REFERENCE_RAINFALL)).clamp(0.0, 1.0)
        })
    }

    pub fn sample(&self, p: Point) -> Sample {
        let elevation = self.terrain.height(p);
        let crowding = self.density.map_or(0.0, |field| field.at(p));
        let temperature =
            self.profile.mean_temperature - LAPSE_RATE * elevation + HEAT_ISLAND * crowding;
        let variation = 0.75 + 0.5 * noise(self.seed ^ 2, p.x / 4_000.0, p.y / 4_000.0);
        let rainfall = self.profile.annual_rainfall * variation + OROGRAPHIC_RAIN * elevation;
        let moisture = (rainfall / LUSH_RAINFALL).min(1.0);
        let warmth =
            (1.0 - (temperature - IDEAL_TEMPERATURE).abs() / TEMPERATURE_TOLERANCE).max(0.0);
        let patchiness = 0.7 + 0.6 * noise(self.seed ^ 3, p.x / 1_200.0, p.y / 1_200.0);
        Sample {
            elevation,
            slope: self.slope(p),
            rock_depth: self.rock_depth(p),
            soil: self.soil(p),
            temperature,
            rainfall,
            vegetation: (moisture * warmth * patchiness - 0.6 * crowding).clamp(0.0, 1.0),
            flood_risk: self.flood_risk(p, elevation, rainfall),
        }
    }
}
