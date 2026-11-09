use crate::geometry::{
    Point, centroid, clip_half_plane, distance_to_segment, edges, point, point_in_convex,
    segments_cross, signed_area, without_short_edges,
};
use crate::rng::Rng;
use crate::spatial::SpatialIndex;
use crate::terrain::Terrain;
use serde::Serialize;
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StreetClass {
    Arterial,
    Local,
}

impl StreetClass {
    pub fn width(self) -> f64 {
        match self {
            StreetClass::Arterial => 24.0,
            StreetClass::Local => 12.0,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct StreetNode {
    pub pos: Point,
    pub elevation: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct StreetEdge {
    pub a: usize,
    pub b: usize,
    pub class: StreetClass,
    pub width: f64,
    pub bridge: bool,
}

pub struct StreetLayout {
    pub nodes: Vec<StreetNode>,
    pub edges: Vec<StreetEdge>,
    pub blocks: Vec<Vec<Point>>,
    pub arterials: Vec<(Point, Point)>,
    pub index: SpatialIndex,
}

/// Block shapes form one continuous family: a Voronoi diagram over a lattice
/// whose rows shift by half a cell. `shape` 0 gives rectangles, 1 regular
/// hexagons, values between give stretched hexagons, and `irregularity` jitters
/// the sites into pentagons and other mixes. Voronoi cells are always convex,
/// which everything downstream (insets, lots, footprints) relies on.
#[derive(Clone, Copy, Debug)]
pub struct Lattice {
    pub block_area: f64,
    pub shape: f64,
    pub irregularity: f64,
}

pub struct StreetOptions<'a> {
    pub lattice: Lattice,
    pub interior: StreetClass,
    pub bridge_every: usize,
    pub rivers: &'a [Vec<Point>],
}

struct RawStreet {
    a: Point,
    b: Point,
    class: StreetClass,
}

pub const ON_LINE_TOLERANCE: f64 = 0.01;
const HEX_ROW_RATIO: f64 = 0.866_025_403_784_438_6;
const MAX_JITTER: f64 = 0.45;
const BOUNDARY_MARGIN: f64 = 0.2;
const VORONOI_REACH: f64 = 3.5;
const MIN_CELL_AREA: f64 = 1.0;

pub fn build(
    boundary: &[Point],
    options: &StreetOptions,
    terrain: &Terrain,
    rng: &mut Rng,
) -> StreetLayout {
    let (sites, spacing) = lattice_sites(boundary, &options.lattice, rng);
    let blocks = voronoi_cells(boundary, &sites, spacing);

    let boundary_streets = edges(boundary).map(|(a, b)| RawStreet {
        a,
        b,
        class: StreetClass::Arterial,
    });
    let cell_streets = blocks.iter().flat_map(|block| {
        edges(block).map(|(a, b)| RawStreet {
            a,
            b,
            class: options.interior,
        })
    });
    let raw: Vec<RawStreet> = boundary_streets.chain(cell_streets).collect();

    let mut index = SpatialIndex::new(spacing);
    for street in &raw {
        intern(&mut index, street.a);
        intern(&mut index, street.b);
    }

    let mut street_edges: Vec<StreetEdge> = Vec::new();
    let mut known_edges = BTreeSet::new();
    for street in &raw {
        let axis = street.b - street.a;
        let mut along: Vec<(f64, usize)> = index
            .on_segment(street.a, street.b, ON_LINE_TOLERANCE)
            .into_iter()
            .map(|i| ((index.points()[i] - street.a).dot(axis), i))
            .collect();
        along.sort_by(|l, r| l.0.total_cmp(&r.0));
        for pair in along.windows(2) {
            let (a, b) = (pair[0].1.min(pair[1].1), pair[0].1.max(pair[1].1));
            if known_edges.insert((a, b)) {
                street_edges.push(StreetEdge {
                    a,
                    b,
                    class: street.class,
                    width: street.class.width(),
                    bridge: false,
                });
            }
        }
    }
    let street_edges = keep_river_crossings(street_edges, &index, options);

    StreetLayout {
        nodes: index
            .points()
            .iter()
            .map(|pos| StreetNode {
                pos: *pos,
                elevation: terrain.height(*pos),
            })
            .collect(),
        edges: street_edges,
        blocks,
        arterials: edges(boundary).collect(),
        index,
    }
}

fn intern(index: &mut SpatialIndex, p: Point) {
    if !index.any_within(p, ON_LINE_TOLERANCE) {
        index.insert(p);
    }
}

fn lattice_sites(boundary: &[Point], lattice: &Lattice, rng: &mut Rng) -> (Vec<Point>, f64) {
    let row_ratio = 1.0 + (HEX_ROW_RATIO - 1.0) * lattice.shape;
    let spacing = (lattice.block_area / row_ratio).sqrt();
    let row_height = spacing * row_ratio;
    let center = centroid(boundary);
    let reach = boundary
        .iter()
        .map(|p| p.distance(center))
        .fold(0.0, f64::max);
    let rows = (reach / row_height).ceil() as i64 + 1;
    let cols = (reach / spacing).ceil() as i64 + 1;
    let jitter = lattice.irregularity * MAX_JITTER * spacing;

    let mut sites = Vec::new();
    for row in -rows..=rows {
        let shift = if row.rem_euclid(2) == 1 {
            0.5 * lattice.shape * spacing
        } else {
            0.0
        };
        for col in -cols..=cols {
            let nudge = point(rng.range(-1.0, 1.0), rng.range(-1.0, 1.0)) * jitter;
            let site =
                center + point(col as f64 * spacing + shift, row as f64 * row_height) + nudge;
            let clear_of_boundary = edges(boundary)
                .all(|(a, b)| distance_to_segment(site, a, b) >= BOUNDARY_MARGIN * spacing);
            if point_in_convex(site, boundary) && clear_of_boundary {
                sites.push(site);
            }
        }
    }
    if sites.is_empty() {
        sites.push(center);
    }
    (sites, spacing)
}

fn voronoi_cells(boundary: &[Point], sites: &[Point], spacing: f64) -> Vec<Vec<Point>> {
    let mut index = SpatialIndex::new(spacing);
    for site in sites {
        index.insert(*site);
    }
    sites
        .iter()
        .enumerate()
        .filter_map(|(i, site)| {
            let mut cell = boundary.to_vec();
            for j in index.within(*site, spacing * VORONOI_REACH) {
                if j != i && cell.len() >= 3 {
                    let other = sites[j];
                    cell = clip_half_plane(&cell, (*site + other) * 0.5, *site - other);
                }
            }
            let cell = without_short_edges(&cell, ON_LINE_TOLERANCE);
            (cell.len() >= 3 && signed_area(&cell) > MIN_CELL_AREA).then_some(cell)
        })
        .collect()
}

/// Rivers cut the street grid: most crossings disappear and the survivors
/// become bridges, but no part of the graph is ever left unreachable.
fn keep_river_crossings(
    streets: Vec<StreetEdge>,
    index: &SpatialIndex,
    options: &StreetOptions,
) -> Vec<StreetEdge> {
    let crosses_river = |edge: &StreetEdge| {
        let (a, b) = (index.points()[edge.a], index.points()[edge.b]);
        options.rivers.iter().any(|river| {
            river
                .windows(2)
                .any(|piece| segments_cross(a, b, piece[0], piece[1]))
        })
    };
    let crossing: Vec<bool> = streets.iter().map(crosses_river).collect();

    let mut components: Vec<usize> = (0..index.points().len()).collect();
    for (edge, _) in streets.iter().zip(&crossing).filter(|(_, c)| !**c) {
        union(&mut components, edge.a, edge.b);
    }

    let mut seen_crossings = 0;
    streets
        .into_iter()
        .zip(crossing)
        .filter_map(|(mut edge, crosses)| {
            if !crosses {
                return Some(edge);
            }
            let scheduled = seen_crossings % options.bridge_every == 0;
            seen_crossings += 1;
            let connects_new_ground =
                find(&mut components, edge.a) != find(&mut components, edge.b);
            if edge.class == StreetClass::Arterial || scheduled || connects_new_ground {
                union(&mut components, edge.a, edge.b);
                edge.bridge = true;
                Some(edge)
            } else {
                None
            }
        })
        .collect()
}

fn find(components: &mut [usize], node: usize) -> usize {
    let mut root = node;
    while components[root] != root {
        components[root] = components[components[root]];
        root = components[root];
    }
    root
}

fn union(components: &mut [usize], a: usize, b: usize) {
    let (root_a, root_b) = (find(components, a), find(components, b));
    components[root_a] = root_b;
}
