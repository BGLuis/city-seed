use crate::geometry::{Point, point};
use crate::networks::Frontier;
use crate::rng::Rng;
use crate::terrain::Terrain;
use std::collections::BinaryHeap;

const GRID: usize = 64;
const LOWLAND_PREFERENCE: f64 = 8.0;

/// Traces each river as the cheapest walk across the city that favours low ground.
pub fn generate(seed: u64, count: u32, side: f64, terrain: &Terrain) -> Vec<Vec<Point>> {
    let mut rng = Rng::derive(seed, "rivers");
    let cell = side / GRID as f64;
    let center = |x: usize, y: usize| point((x as f64 + 0.5) * cell, (y as f64 + 0.5) * cell);
    let heights: Vec<f64> = (0..GRID * GRID)
        .map(|i| terrain.height(center(i % GRID, i / GRID)))
        .collect();
    let low = heights.iter().copied().fold(f64::MAX, f64::min);
    let high = heights.iter().copied().fold(f64::MIN, f64::max);
    let span = (high - low).max(1.0);
    let step_cost: Vec<f64> = heights
        .iter()
        .map(|h| 1.0 + LOWLAND_PREFERENCE * (h - low) / span)
        .collect();

    (0..count)
        .map(|_| {
            let along = (rng.next_u64() % GRID as u64) as usize;
            let across = (rng.next_u64() % GRID as u64) as usize;
            let (start, end) = if rng.chance(0.5) {
                (along, GRID * (GRID - 1) + across)
            } else {
                (along * GRID, across * GRID + GRID - 1)
            };
            let cells = cheapest_walk(start, end, &step_cost);
            let raw: Vec<Point> = cells.iter().map(|c| center(c % GRID, c / GRID)).collect();
            let extended = extend_past_edges(&raw, side);
            smoothed(&extended)
        })
        .collect()
}

fn cheapest_walk(start: usize, end: usize, step_cost: &[f64]) -> Vec<usize> {
    let mut distance = vec![f64::MAX; step_cost.len()];
    let mut previous = vec![usize::MAX; step_cost.len()];
    let mut frontier = BinaryHeap::new();
    distance[start] = 0.0;
    frontier.push(Frontier {
        cost: 0.0,
        node: start,
    });
    while let Some(Frontier { cost, node }) = frontier.pop() {
        if cost > distance[node] {
            continue;
        }
        let (x, y) = (node % GRID, node / GRID);
        let mut neighbors = Vec::with_capacity(4);
        if x > 0 {
            neighbors.push(node - 1);
        }
        if x + 1 < GRID {
            neighbors.push(node + 1);
        }
        if y > 0 {
            neighbors.push(node - GRID);
        }
        if y + 1 < GRID {
            neighbors.push(node + GRID);
        }
        for next in neighbors {
            let candidate = cost + step_cost[next];
            if candidate < distance[next] {
                distance[next] = candidate;
                previous[next] = node;
                frontier.push(Frontier {
                    cost: candidate,
                    node: next,
                });
            }
        }
    }
    let mut path = vec![end];
    while let Some(&last) = path.last().filter(|last| **last != start) {
        path.push(previous[last]);
    }
    path.reverse();
    path
}

/// Pushes the endpoints outside the city so the river visibly enters and leaves.
fn extend_past_edges(points: &[Point], side: f64) -> Vec<Point> {
    let outward = |p: Point| {
        let middle = point(side / 2.0, side / 2.0);
        p + (p - middle).normalized() * (side * 0.05)
    };
    let mut extended = vec![outward(points[0])];
    extended.extend_from_slice(points);
    extended.push(outward(points[points.len() - 1]));
    extended
}

fn smoothed(points: &[Point]) -> Vec<Point> {
    let mut result = vec![points[0]];
    result.extend(
        points
            .windows(3)
            .map(|w| (w[0] * 0.25) + (w[1] * 0.5) + (w[2] * 0.25)),
    );
    result.push(points[points.len() - 1]);
    result
}
