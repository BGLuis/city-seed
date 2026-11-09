use crate::geometry::{Point, clip_half_plane, edges, extent, longest_edge_direction, signed_area};
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
}

pub struct StreetLayout {
    pub nodes: Vec<StreetNode>,
    pub edges: Vec<StreetEdge>,
    pub blocks: Vec<Vec<Point>>,
    pub arterials: Vec<(Point, Point)>,
    pub index: SpatialIndex,
}

struct RawStreet {
    a: Point,
    b: Point,
    class: StreetClass,
}

pub const ON_LINE_TOLERANCE: f64 = 0.01;
const ARTERIAL_SPLIT_DEPTH: u32 = 2;

pub fn build(
    boundary: &[Point],
    block_area: f64,
    terrain: &Terrain,
    rng: &mut Rng,
) -> StreetLayout {
    let is_block =
        |polygon: &[Point], rng: &mut Rng| signed_area(polygon) <= block_area * rng.range(0.8, 1.3);
    build_with(
        boundary,
        is_block,
        ARTERIAL_SPLIT_DEPTH,
        block_area.sqrt(),
        terrain,
        rng,
    )
}

/// Splits `boundary` until `is_leaf` accepts each piece, then nodes the cut
/// lines into a planar graph whose T-junctions are shared by every street.
pub fn build_with(
    boundary: &[Point],
    mut is_leaf: impl FnMut(&[Point], &mut Rng) -> bool,
    arterial_depth: u32,
    index_cell: f64,
    terrain: &Terrain,
    rng: &mut Rng,
) -> StreetLayout {
    let mut raw: Vec<RawStreet> = edges(boundary)
        .map(|(a, b)| RawStreet {
            a,
            b,
            class: StreetClass::Arterial,
        })
        .collect();
    let mut blocks = Vec::new();
    split(
        boundary,
        0,
        arterial_depth,
        &mut is_leaf,
        rng,
        &mut raw,
        &mut blocks,
    );

    let mut index = SpatialIndex::new(index_cell);
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
                });
            }
        }
    }

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
        arterials: raw
            .iter()
            .filter(|s| s.class == StreetClass::Arterial)
            .map(|s| (s.a, s.b))
            .collect(),
        index,
    }
}

fn intern(index: &mut SpatialIndex, p: Point) {
    if !index.any_within(p, ON_LINE_TOLERANCE) {
        index.insert(p);
    }
}

fn split(
    polygon: &[Point],
    depth: u32,
    arterial_depth: u32,
    is_leaf: &mut impl FnMut(&[Point], &mut Rng) -> bool,
    rng: &mut Rng,
    raw: &mut Vec<RawStreet>,
    blocks: &mut Vec<Vec<Point>>,
) {
    if is_leaf(polygon, rng) {
        blocks.push(polygon.to_vec());
        return;
    }
    let axis = longest_edge_direction(polygon);
    let (low, high) = extent(polygon, axis);
    let cut = low + (high - low) * rng.range(0.38, 0.62);
    let origin = axis * cut;
    let near_side = clip_half_plane(polygon, origin, axis * -1.0);
    let far_side = clip_half_plane(polygon, origin, axis);

    let tangent = axis.left_normal();
    let (start, end) = extent(
        &near_side
            .iter()
            .copied()
            .filter(|p| (p.dot(axis) - cut).abs() < ON_LINE_TOLERANCE)
            .collect::<Vec<_>>(),
        tangent,
    );
    let class = if depth < arterial_depth {
        StreetClass::Arterial
    } else {
        StreetClass::Local
    };
    raw.push(RawStreet {
        a: origin + tangent * start,
        b: origin + tangent * end,
        class,
    });
    for side in [&near_side, &far_side] {
        split(side, depth + 1, arterial_depth, is_leaf, rng, raw, blocks);
    }
}
