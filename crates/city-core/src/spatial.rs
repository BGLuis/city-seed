use crate::geometry::{Point, distance_to_segment};
use std::collections::HashMap;

/// Uniform grid over points. Results are always ordered by point index, so the
/// map's randomized iteration order can never leak into generation.
pub struct SpatialIndex {
    cell: f64,
    points: Vec<Point>,
    cells: HashMap<(i64, i64), Vec<usize>>,
}

impl SpatialIndex {
    pub fn new(cell: f64) -> Self {
        Self {
            cell,
            points: Vec::new(),
            cells: HashMap::new(),
        }
    }

    pub fn insert(&mut self, p: Point) -> usize {
        let index = self.points.len();
        self.points.push(p);
        self.cells.entry(self.key(p)).or_default().push(index);
        index
    }

    pub fn points(&self) -> &[Point] {
        &self.points
    }

    fn key(&self, p: Point) -> (i64, i64) {
        (
            (p.x / self.cell).floor() as i64,
            (p.y / self.cell).floor() as i64,
        )
    }

    fn neighborhood(&self, key: (i64, i64)) -> impl Iterator<Item = usize> + '_ {
        (-1..=1)
            .flat_map(move |dy| (-1..=1).map(move |dx| (key.0 + dx, key.1 + dy)))
            .filter_map(|cell| self.cells.get(&cell))
            .flatten()
            .copied()
    }

    /// `radius` must not exceed the cell size.
    pub fn any_within(&self, p: Point, radius: f64) -> bool {
        self.neighborhood(self.key(p))
            .any(|index| self.points[index].distance(p) < radius)
    }

    /// Nearest point, breaking distance ties towards the lowest index.
    pub fn nearest(&self, p: Point) -> Option<usize> {
        let (center_x, center_y) = self.key(p);
        let mut best: Option<(f64, usize)> = None;
        let mut seen = 0;
        let mut ring: i64 = 0;
        while seen < self.points.len() {
            for (x, y) in ring_cells(center_x, center_y, ring) {
                for &index in self.cells.get(&(x, y)).into_iter().flatten() {
                    seen += 1;
                    let distance = self.points[index].distance(p);
                    let closer = best
                        .is_none_or(|(d, i)| distance.total_cmp(&d).then(index.cmp(&i)).is_lt());
                    if closer {
                        best = Some((distance, index));
                    }
                }
            }
            if best.is_some_and(|(distance, _)| distance < ring as f64 * self.cell) {
                break;
            }
            ring += 1;
        }
        best.map(|(_, index)| index)
    }

    /// Points within `tolerance` of the segment, ascending by index.
    pub fn on_segment(&self, a: Point, b: Point, tolerance: f64) -> Vec<usize> {
        let steps = (a.distance(b) / self.cell).ceil() as usize + 1;
        let mut found: Vec<usize> = (0..=steps)
            .flat_map(|step| {
                let sample = a + (b - a) * (step as f64 / steps as f64);
                self.neighborhood(self.key(sample))
            })
            .filter(|index| distance_to_segment(self.points[*index], a, b) < tolerance)
            .collect();
        found.sort_unstable();
        found.dedup();
        found
    }
}

fn ring_cells(x: i64, y: i64, ring: i64) -> Vec<(i64, i64)> {
    if ring == 0 {
        return vec![(x, y)];
    }
    let mut cells = Vec::with_capacity(8 * ring as usize);
    for offset in -ring..=ring {
        cells.push((x + offset, y - ring));
        cells.push((x + offset, y + ring));
    }
    for offset in (1 - ring)..ring {
        cells.push((x - ring, y + offset));
        cells.push((x + ring, y + offset));
    }
    cells
}
