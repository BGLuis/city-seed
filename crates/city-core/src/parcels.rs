use crate::Profile;
use crate::city::DensityField;
use crate::environment::{Environment, Soil};
use crate::geometry::{
    Point, centroid, clip_band, distance_to_segment, edges, extent, inset_edges,
    longest_edge_direction, polyline_touches_polygon, signed_area,
};
use crate::rng::Rng;
use crate::streets::StreetLayout;
use crate::terrain::noise;
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LandUse {
    Residential,
    Commercial,
    Industrial,
    Park,
    Water,
    Rail,
    Forest,
    Wetland,
}

/// Linear features that claim whole blocks from development.
pub struct Corridors<'a> {
    pub rivers: &'a [Vec<Point>],
    pub rails: &'a [Vec<Point>],
}

#[derive(Clone, Debug, Serialize)]
pub struct Lot {
    pub polygon: Vec<Point>,
    pub land_use: LandUse,
    pub soil: Soil,
}

#[derive(Clone, Debug, Serialize)]
pub struct Building {
    pub lot: usize,
    pub footprint: Vec<Point>,
    pub floors: u32,
    pub land_use: LandUse,
    pub street_node: usize,
}

const MIN_LOT_AREA: f64 = 60.0;
const MIN_FOOTPRINT_AREA: f64 = 30.0;
const TWO_ROW_DEPTH: f64 = 70.0;
const ARTERIAL_SETBACK: f64 = 14.0;
const LOCAL_SETBACK: f64 = 8.0;
const FLOOD_LIMIT: f64 = 0.6;
const STEEP_SLOPE: f64 = 0.2;
const WILD_LAND_VALUE: f64 = 0.07;
const LUSH_VEGETATION: f64 = 0.55;
const SEISMIC_HEIGHT_PENALTY: f64 = 0.6;

pub fn subdivide(
    boundary: &[Point],
    layout: &StreetLayout,
    profile: &Profile,
    city_density: Option<&DensityField>,
    corridors: &Corridors,
    environment: &Environment,
    rng: &mut Rng,
) -> (Vec<Lot>, Vec<Building>) {
    let center = centroid(boundary);
    let radius = boundary
        .iter()
        .map(|p| p.distance(center))
        .fold(0.0, f64::max);
    let value_seed = rng.next_u64();
    let mut lots = Vec::new();
    let mut buildings = Vec::new();

    for block in &layout.blocks {
        let touches = |lines: &[Vec<Point>]| {
            lines
                .iter()
                .any(|line| polyline_touches_polygon(line, block))
        };
        let ground = environment.sample(centroid(block));
        let claimed = if touches(corridors.rivers) {
            Some(LandUse::Water)
        } else if touches(corridors.rails) {
            Some(LandUse::Rail)
        } else if ground.flood_risk > FLOOD_LIMIT {
            Some(LandUse::Wetland)
        } else if ground.slope > STEEP_SLOPE {
            Some(LandUse::Forest)
        } else {
            None
        };
        if let Some(land_use) = claimed {
            lots.push(Lot {
                polygon: block.clone(),
                land_use,
                soil: ground.soil,
            });
            continue;
        }
        let setbacks: Vec<f64> = edges(block)
            .map(|(a, b)| street_setback(a, b, layout))
            .collect();
        let buildable = inset_edges(block, &setbacks);
        if buildable.len() < 3 || signed_area(&buildable) < MIN_LOT_AREA {
            continue;
        }
        let middle = centroid(&buildable);
        let value = city_density.map_or_else(
            || land_value(middle, center, radius, value_seed),
            |field| field.at(middle),
        );
        let land_use = if value < WILD_LAND_VALUE && ground.vegetation > LUSH_VEGETATION {
            LandUse::Forest
        } else {
            choose_land_use(value, ground.vegetation, profile, rng)
        };
        if matches!(land_use, LandUse::Park | LandUse::Forest) {
            lots.push(Lot {
                polygon: buildable,
                land_use,
                soil: ground.soil,
            });
            continue;
        }
        for polygon in split_lots(&buildable, land_use, rng) {
            let lot = lots.len();
            let soil = environment.soil(centroid(&polygon));
            if let Some(footprint) = footprint_of(&polygon, rng) {
                buildings.push(Building {
                    lot,
                    street_node: nearest_node(centroid(&footprint), layout),
                    floors: floors(land_use, value, profile, rng)
                        .min(height_limit(soil, profile.seismic_risk)),
                    land_use,
                    footprint,
                });
            }
            lots.push(Lot {
                polygon,
                land_use,
                soil,
            });
        }
    }
    (lots, buildings)
}

fn street_setback(a: Point, b: Point, layout: &StreetLayout) -> f64 {
    let midpoint = (a + b) * 0.5;
    let on_arterial = layout
        .arterials
        .iter()
        .any(|(s, e)| distance_to_segment(midpoint, *s, *e) < 0.01);
    if on_arterial {
        ARTERIAL_SETBACK
    } else {
        LOCAL_SETBACK
    }
}

fn land_value(p: Point, center: Point, radius: f64, seed: u64) -> f64 {
    let centrality = 1.0 - p.distance(center) / radius;
    (0.65 * centrality + 0.5 * noise(seed, p.x / 500.0, p.y / 500.0) - 0.05).clamp(0.0, 1.0)
}

/// Taller towers need firmer ground, and earthquakes lower every limit.
fn height_limit(soil: Soil, seismic_risk: f64) -> u32 {
    let bearing = match soil {
        Soil::Rocky => 120.0,
        Soil::Clay => 40.0,
        Soil::Sand => 25.0,
        Soil::Alluvial => 12.0,
    };
    ((bearing * (1.0 - SEISMIC_HEIGHT_PENALTY * seismic_risk)) as u32).max(1)
}

fn choose_land_use(value: f64, vegetation: f64, profile: &Profile, rng: &mut Rng) -> LandUse {
    if rng.chance(profile.park_share * (0.5 + vegetation)) {
        LandUse::Park
    } else if value > 0.62 {
        LandUse::Commercial
    } else if value < 0.3 && rng.chance(0.35) {
        LandUse::Industrial
    } else {
        LandUse::Residential
    }
}

fn split_lots(buildable: &[Point], land_use: LandUse, rng: &mut Rng) -> Vec<Vec<Point>> {
    let along = longest_edge_direction(buildable);
    let across = along.left_normal();
    let (across_low, across_high) = extent(buildable, across);
    let rows = if across_high - across_low > TWO_ROW_DEPTH {
        2
    } else {
        1
    };
    let row_depth = (across_high - across_low) / rows as f64;
    let target = match land_use {
        LandUse::Commercial => 28.0,
        LandUse::Industrial => 50.0,
        _ => 16.0,
    };

    let mut result = Vec::new();
    for row in 0..rows {
        let low = across_low + row_depth * row as f64;
        let strip = clip_band(buildable, across, low, low + row_depth);
        if strip.len() < 3 {
            continue;
        }
        let (mut cursor, end) = extent(&strip, along);
        while cursor < end {
            let mut width = target * rng.range(0.8, 1.2);
            if end - cursor - width < target * 0.5 {
                width = end - cursor;
            }
            let piece = clip_band(&strip, along, cursor, cursor + width);
            if piece.len() >= 3 && signed_area(&piece) > MIN_LOT_AREA {
                result.push(piece);
            }
            cursor += width;
        }
    }
    result
}

fn footprint_of(lot: &[Point], rng: &mut Rng) -> Option<Vec<Point>> {
    let setback = rng.range(1.5, 3.5);
    let footprint = inset_edges(lot, &vec![setback; lot.len()]);
    (footprint.len() >= 3 && signed_area(&footprint) > MIN_FOOTPRINT_AREA).then_some(footprint)
}

fn floors(land_use: LandUse, value: f64, profile: &Profile, rng: &mut Rng) -> u32 {
    let scale = 0.5 + profile.density;
    let height = match land_use {
        LandUse::Commercial => rng.range(5.0, 40.0) * value * value * scale,
        LandUse::Residential => rng.range(2.0, 12.0) * value * scale,
        LandUse::Industrial => rng.range(1.0, 3.0),
        _ => 0.0,
    };
    (height as u32).max(1)
}

fn nearest_node(p: Point, layout: &StreetLayout) -> usize {
    layout.index.nearest(p).expect("street graph has nodes")
}
