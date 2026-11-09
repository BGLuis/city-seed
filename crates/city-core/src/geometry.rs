use serde::{Deserialize, Serialize};
use std::ops::{Add, Mul, Sub};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

/// Polygons produced by repeated clipping carry rounding error at collinear vertices.
const CONVEXITY_TOLERANCE: f64 = 1e-3;

pub fn point(x: f64, y: f64) -> Point {
    Point { x, y }
}

impl Add for Point {
    type Output = Point;
    fn add(self, other: Point) -> Point {
        point(self.x + other.x, self.y + other.y)
    }
}

impl Sub for Point {
    type Output = Point;
    fn sub(self, other: Point) -> Point {
        point(self.x - other.x, self.y - other.y)
    }
}

impl Mul<f64> for Point {
    type Output = Point;
    fn mul(self, factor: f64) -> Point {
        point(self.x * factor, self.y * factor)
    }
}

impl Point {
    pub fn dot(self, other: Point) -> f64 {
        self.x * other.x + self.y * other.y
    }

    pub fn cross(self, other: Point) -> f64 {
        self.x * other.y - self.y * other.x
    }

    pub fn length(self) -> f64 {
        self.dot(self).sqrt()
    }

    pub fn distance(self, other: Point) -> f64 {
        (self - other).length()
    }

    pub fn normalized(self) -> Point {
        self * (1.0 / self.length())
    }

    pub fn left_normal(self) -> Point {
        point(-self.y, self.x)
    }
}

pub fn signed_area(polygon: &[Point]) -> f64 {
    edges(polygon).map(|(a, b)| a.cross(b)).sum::<f64>() / 2.0
}

pub fn edges(polygon: &[Point]) -> impl Iterator<Item = (Point, Point)> + '_ {
    (0..polygon.len()).map(|i| (polygon[i], polygon[(i + 1) % polygon.len()]))
}

pub fn centroid(polygon: &[Point]) -> Point {
    let sum = polygon.iter().fold(point(0.0, 0.0), |acc, p| acc + *p);
    sum * (1.0 / polygon.len() as f64)
}

pub fn is_convex_ccw(polygon: &[Point]) -> bool {
    let count = polygon.len();
    (0..count).all(|i| {
        let a = polygon[i];
        let b = polygon[(i + 1) % count];
        let c = polygon[(i + 2) % count];
        (b - a).cross(c - b) >= -CONVEXITY_TOLERANCE
    })
}

/// Keeps the part of a convex polygon where `(p - origin) · normal >= 0`.
pub fn clip_half_plane(polygon: &[Point], origin: Point, normal: Point) -> Vec<Point> {
    let mut clipped = Vec::with_capacity(polygon.len() + 1);
    for (p, q) in edges(polygon) {
        let side_p = (p - origin).dot(normal);
        let side_q = (q - origin).dot(normal);
        if side_p >= 0.0 {
            clipped.push(p);
        }
        if side_p * side_q < 0.0 {
            clipped.push(p + (q - p) * (side_p / (side_p - side_q)));
        }
    }
    clipped
}

pub fn clip_band(polygon: &[Point], axis: Point, low: f64, high: f64) -> Vec<Point> {
    let lower = clip_half_plane(polygon, axis * low, axis);
    clip_half_plane(&lower, axis * high, axis * -1.0)
}

/// Moves each edge of a convex CCW polygon inward by its own distance.
pub fn inset_edges(polygon: &[Point], distances: &[f64]) -> Vec<Point> {
    let mut inset = polygon.to_vec();
    for ((a, b), distance) in edges(polygon).zip(distances) {
        let inward = (b - a).normalized().left_normal();
        inset = clip_half_plane(&inset, a + inward * *distance, inward);
    }
    inset
}

pub fn distance_to_segment(p: Point, a: Point, b: Point) -> f64 {
    let span = b - a;
    let squared = span.dot(span);
    if squared == 0.0 {
        return p.distance(a);
    }
    let t = ((p - a).dot(span) / squared).clamp(0.0, 1.0);
    p.distance(a + span * t)
}

pub fn longest_edge_direction(polygon: &[Point]) -> Point {
    let (a, b) = edges(polygon)
        .max_by(|e1, e2| (e1.1 - e1.0).length().total_cmp(&(e2.1 - e2.0).length()))
        .expect("polygon has edges");
    (b - a).normalized()
}

pub fn extent(polygon: &[Point], axis: Point) -> (f64, f64) {
    polygon.iter().fold((f64::MAX, f64::MIN), |(low, high), p| {
        let along = p.dot(axis);
        (low.min(along), high.max(along))
    })
}
