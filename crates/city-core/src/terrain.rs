use crate::geometry::{Point, point};
use crate::rng::Rng;
use serde::Serialize;

#[derive(Clone, Copy)]
pub struct Terrain {
    seed: u64,
    relief: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct TerrainGrid {
    pub origin: Point,
    pub cell: f64,
    pub cols: usize,
    pub rows: usize,
    pub heights: Vec<f64>,
}

const GRID_CELLS: usize = 96;

/// Trigonometry is avoided on purpose: libm results differ across platforms,
/// which would make the same seed produce different cities in WASM and Unity.
pub fn noise(seed: u64, x: f64, y: f64) -> f64 {
    let (floor_x, floor_y) = (x.floor(), y.floor());
    let (ix, iy) = (floor_x as i64, floor_y as i64);
    let (tx, ty) = (smooth(x - floor_x), smooth(y - floor_y));
    let top = lerp(lattice(seed, ix, iy), lattice(seed, ix + 1, iy), tx);
    let bottom = lerp(lattice(seed, ix, iy + 1), lattice(seed, ix + 1, iy + 1), tx);
    lerp(top, bottom, ty)
}

fn lattice(seed: u64, ix: i64, iy: i64) -> f64 {
    let mixed = seed
        ^ (ix as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (iy as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    Rng::new(mixed).unit()
}

fn smooth(t: f64) -> f64 {
    t * t * (3.0 - 2.0 * t)
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

impl Terrain {
    pub fn new(seed: u64, relief: f64) -> Self {
        Self {
            seed: Rng::derive(seed, "terrain").next_u64(),
            relief,
        }
    }

    pub fn height(&self, p: Point) -> f64 {
        self.relief
            * (120.0 * noise(self.seed ^ 3, p.x / 30_000.0, p.y / 30_000.0)
                + 40.0 * noise(self.seed, p.x / 900.0, p.y / 900.0)
                + 14.0 * noise(self.seed ^ 1, p.x / 280.0, p.y / 280.0)
                + 4.0 * noise(self.seed ^ 2, p.x / 90.0, p.y / 90.0))
    }

    /// The broad regional slope alone, without the local hills and valleys.
    pub fn regional_height(&self, p: Point) -> f64 {
        self.relief * 120.0 * noise(self.seed ^ 3, p.x / 30_000.0, p.y / 30_000.0)
    }

    pub fn relief(&self) -> f64 {
        self.relief
    }

    pub fn grid(&self, boundary: &[Point]) -> TerrainGrid {
        let min_x = boundary.iter().map(|p| p.x).fold(f64::MAX, f64::min);
        let max_x = boundary.iter().map(|p| p.x).fold(f64::MIN, f64::max);
        let min_y = boundary.iter().map(|p| p.y).fold(f64::MAX, f64::min);
        let max_y = boundary.iter().map(|p| p.y).fold(f64::MIN, f64::max);
        let cell = (max_x - min_x).max(max_y - min_y) / GRID_CELLS as f64;
        let origin = point(min_x, min_y);
        let cols = ((max_x - min_x) / cell).ceil() as usize + 1;
        let rows = ((max_y - min_y) / cell).ceil() as usize + 1;
        let heights = (0..rows)
            .flat_map(|row| {
                (0..cols).map(move |col| {
                    self.height(origin + point(col as f64 * cell, row as f64 * cell))
                })
            })
            .collect();
        TerrainGrid {
            origin,
            cell,
            cols,
            rows,
            heights,
        }
    }
}
