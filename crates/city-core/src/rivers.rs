use crate::geometry::{Point, point};
use crate::networks::Frontier;
use crate::rng::Rng;
use crate::terrain::{Terrain, noise};
use std::collections::BinaryHeap;

const GRID: usize = 64;
const LOWLAND_PREFERENCE: f64 = 8.0;
const MEANDER_STRENGTH: f64 = 7.0;
const MEANDER_SCALE: f64 = 7.0;
const DIAGONAL_STEP: f64 = std::f64::consts::SQRT_2;
const SMOOTHING_PASSES: usize = 3;

/// Traces each river as the cheapest walk across the city that favours low
/// ground. Later rivers stop where they meet an earlier one, as tributaries do.
pub fn generate(seed: u64, count: u32, side: f64, terrain: &Terrain) -> Vec<Vec<Point>> {
    let mut rng = Rng::derive(seed, "rivers");
    let meander_seed = rng.next_u64();
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
        .enumerate()
        .map(|(i, h)| {
            let (x, y) = ((i % GRID) as f64, (i / GRID) as f64);
            let meander = noise(meander_seed, x / MEANDER_SCALE, y / MEANDER_SCALE);
            1.0 + LOWLAND_PREFERENCE * (h - low) / span + MEANDER_STRENGTH * meander
        })
        .collect();

    let mut taken = vec![false; GRID * GRID];
    (0..count)
        .map(|_| {
            let along = (rng.next_u64() % GRID as u64) as usize;
            let across = (rng.next_u64() % GRID as u64) as usize;
            let (start, end) = if rng.chance(0.5) {
                (along, GRID * (GRID - 1) + across)
            } else {
                (along * GRID, across * GRID + GRID - 1)
            };
            let cells = cheapest_walk(start, end, &step_cost, &taken);
            let joins_another = taken[cells[cells.len() - 1]];
            let raw: Vec<Point> = cells.iter().map(|c| center(c % GRID, c / GRID)).collect();
            for cell in cells {
                taken[cell] = true;
            }
            smoothed(extend_past_edges(&raw, side, joins_another))
        })
        .collect()
}

fn neighbors(node: usize) -> impl Iterator<Item = (usize, f64)> {
    let (x, y) = ((node % GRID) as i64, (node / GRID) as i64);
    (-1i64..=1)
        .flat_map(move |dy| (-1i64..=1).map(move |dx| (dx, dy)))
        .filter(|offset| *offset != (0, 0))
        .filter_map(move |(dx, dy)| {
            let (nx, ny) = (x + dx, y + dy);
            let inside = (0..GRID as i64).contains(&nx) && (0..GRID as i64).contains(&ny);
            let step = if dx != 0 && dy != 0 {
                DIAGONAL_STEP
            } else {
                1.0
            };
            inside.then_some((ny as usize * GRID + nx as usize, step))
        })
}

fn cheapest_walk(start: usize, end: usize, step_cost: &[f64], taken: &[bool]) -> Vec<usize> {
    let mut distance = vec![f64::MAX; step_cost.len()];
    let mut previous = vec![usize::MAX; step_cost.len()];
    let mut frontier = BinaryHeap::new();
    distance[start] = 0.0;
    frontier.push(Frontier {
        cost: 0.0,
        node: start,
    });
    let mut last = end;
    while let Some(Frontier { cost, node }) = frontier.pop() {
        if cost > distance[node] {
            continue;
        }
        if node == end || taken[node] {
            last = node;
            break;
        }
        for (next, step) in neighbors(node) {
            let candidate = cost + step_cost[next] * step;
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
    let mut path = vec![last];
    while let Some(&current) = path.last().filter(|current| **current != start) {
        path.push(previous[current]);
    }
    path.reverse();
    path
}

/// Pushes the open ends outside the city so the river visibly enters and leaves.
fn extend_past_edges(points: &[Point], side: f64, joins_another: bool) -> Vec<Point> {
    let middle = point(side / 2.0, side / 2.0);
    let outward = |p: Point| p + (p - middle).normalized() * (side * 0.05);
    let mut extended = vec![outward(points[0])];
    extended.extend_from_slice(points);
    if !joins_another {
        extended.push(outward(points[points.len() - 1]));
    }
    extended
}

fn smoothed(mut points: Vec<Point>) -> Vec<Point> {
    for _ in 0..SMOOTHING_PASSES {
        let mut next = vec![points[0]];
        next.extend(
            points
                .windows(3)
                .map(|w| (w[0] * 0.25) + (w[1] * 0.5) + (w[2] * 0.25)),
        );
        next.push(points[points.len() - 1]);
        points = next;
    }
    points
}
