use crate::geometry::{
    Point, clip_half_plane, distance_to_segment, edges, extent, longest_edge_direction, signed_area,
};
use crate::rng::Rng;
use crate::terrain::Terrain;
use serde::Serialize;

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
}

struct RawStreet {
    a: Point,
    b: Point,
    class: StreetClass,
}

const ON_LINE_TOLERANCE: f64 = 0.01;
const ARTERIAL_SPLIT_DEPTH: u32 = 2;

pub fn build(
    boundary: &[Point],
    block_area: f64,
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
    split(boundary, 0, block_area, rng, &mut raw, &mut blocks);

    let mut nodes: Vec<Point> = Vec::new();
    for street in &raw {
        intern(&mut nodes, street.a);
        intern(&mut nodes, street.b);
    }

    let mut street_edges: Vec<StreetEdge> = Vec::new();
    for street in &raw {
        let axis = street.b - street.a;
        let mut along: Vec<(f64, usize)> = nodes
            .iter()
            .enumerate()
            .filter(|(_, p)| distance_to_segment(**p, street.a, street.b) < ON_LINE_TOLERANCE)
            .map(|(i, p)| ((*p - street.a).dot(axis), i))
            .collect();
        along.sort_by(|l, r| l.0.total_cmp(&r.0));
        for pair in along.windows(2) {
            let (a, b) = (pair[0].1.min(pair[1].1), pair[0].1.max(pair[1].1));
            if !street_edges.iter().any(|e| e.a == a && e.b == b) {
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
        nodes: nodes
            .into_iter()
            .map(|pos| StreetNode {
                pos,
                elevation: terrain.height(pos),
            })
            .collect(),
        edges: street_edges,
        blocks,
        arterials: raw
            .iter()
            .filter(|s| s.class == StreetClass::Arterial)
            .map(|s| (s.a, s.b))
            .collect(),
    }
}

fn intern(nodes: &mut Vec<Point>, p: Point) {
    if !nodes.iter().any(|n| n.distance(p) < ON_LINE_TOLERANCE) {
        nodes.push(p);
    }
}

fn split(
    polygon: &[Point],
    depth: u32,
    block_area: f64,
    rng: &mut Rng,
    raw: &mut Vec<RawStreet>,
    blocks: &mut Vec<Vec<Point>>,
) {
    if signed_area(polygon) <= block_area * rng.range(0.8, 1.3) {
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
    let class = if depth < ARTERIAL_SPLIT_DEPTH {
        StreetClass::Arterial
    } else {
        StreetClass::Local
    };
    raw.push(RawStreet {
        a: origin + tangent * start,
        b: origin + tangent * end,
        class,
    });
    split(&near_side, depth + 1, block_area, rng, raw, blocks);
    split(&far_side, depth + 1, block_area, rng, raw, blocks);
}
