//! Automated performance suite. Exits non-zero when any budget is exceeded, so
//! it can gate a commit or a pipeline. Budgets are generous absolute limits;
//! the growth checks are ratios and hold on any machine.

use city_core::city::{City, CityEvent, CityInput, CitySession, generate_city, generate_district};
use city_core::geometry::centroid;
use city_core::regions;
use city_tools::{number, parse_args};
use serde::Serialize;
use std::fs;
use std::hint::black_box;
use std::process::ExitCode;
use std::time::Instant;

const REGION: &str = "JP-KT";
const SEED: u64 = 42;
const RUNS: usize = 3;
const DISTRICT_SAMPLE: usize = 200;
const MAX_SUPERLINEAR_FACTOR: f64 = 2.0;

#[derive(Serialize)]
struct Measurement {
    name: String,
    value: f64,
    limit: f64,
    unit: &'static str,
    passed: bool,
}

struct Suite {
    budget_scale: f64,
    results: Vec<Measurement>,
}

impl Suite {
    fn record(&mut self, name: String, value: f64, limit: f64, unit: &'static str) {
        self.results.push(Measurement {
            passed: value <= limit,
            name,
            value,
            limit,
            unit,
        });
    }

    fn time(&mut self, name: String, value: f64, budget_ms: f64) {
        let limit = budget_ms * self.budget_scale;
        self.record(name, value, limit, "ms");
    }
}

fn input(population: u64) -> CityInput {
    CityInput {
        seed: SEED,
        population,
        region: Some(REGION.into()),
        profile: regions::find(REGION).expect("preset exists").profile,
        extra_networks: Vec::new(),
    }
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

fn elapsed_ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1_000.0
}

fn measure_macro(population: u64) -> (City, f64) {
    let times = (0..RUNS)
        .map(|_| {
            let start = Instant::now();
            black_box(generate_city(&input(population)).expect("valid city"));
            elapsed_ms(start)
        })
        .collect();
    (
        generate_city(&input(population)).expect("valid city"),
        median(times),
    )
}

fn measure_districts(city: &City) -> (f64, f64) {
    let step = (city.districts.len() / DISTRICT_SAMPLE).max(1);
    let mut times: Vec<f64> = city
        .districts
        .iter()
        .step_by(step)
        .map(|plan| {
            let start = Instant::now();
            black_box(generate_district(city, plan.id).expect("planned district"));
            elapsed_ms(start)
        })
        .collect();
    times.sort_by(f64::total_cmp);
    (times[times.len() / 2], times[times.len() - 1])
}

fn measure_strike(city: &City, radius: f64) -> f64 {
    let center = centroid(&city.boundary);
    let times = (0..RUNS)
        .map(|_| {
            let mut session = CitySession::new(city.clone());
            let start = Instant::now();
            black_box(
                session
                    .apply(&CityEvent::Destroy {
                        center,
                        radius,
                        max_depth: 3.0,
                    })
                    .expect("valid event"),
            );
            elapsed_ms(start)
        })
        .collect();
    median(times)
}

fn main() -> ExitCode {
    let args = parse_args();
    let mut suite = Suite {
        budget_scale: number(&args, "budget-scale", 1.0),
        results: Vec::new(),
    };

    let (small, small_ms) = measure_macro(5_000_000);
    let (large, large_ms) = measure_macro(40_000_000);
    suite.time("macro 5 mi".into(), small_ms, 200.0);
    suite.time("macro 40 mi".into(), large_ms, 1_000.0);

    let (p50, max) = measure_districts(&large);
    suite.time("distrito p50".into(), p50, 50.0);
    suite.time("distrito máximo".into(), max, 100.0);

    suite.time("golpe 150 m".into(), measure_strike(&large, 150.0), 500.0);
    suite.time(
        "golpe 2 km".into(),
        measure_strike(&large, 2_000.0),
        3_000.0,
    );

    let size_ratio = large.districts.len() as f64 / small.districts.len() as f64;
    suite.record(
        "crescimento macro (tempo ÷ distritos)".into(),
        (large_ms / small_ms) / size_ratio,
        MAX_SUPERLINEAR_FACTOR,
        "x",
    );

    let mut failed = false;
    println!("{:<40} {:>12} {:>12}", "medida", "valor", "limite");
    for result in &suite.results {
        let flag = if result.passed { "" } else { "  FALHOU" };
        failed |= !result.passed;
        println!(
            "{:<40} {:>9.2} {:<2} {:>9.2} {:<2}{flag}",
            result.name, result.value, result.unit, result.limit, result.unit
        );
    }

    let report = serde_json::to_string_pretty(&suite.results).expect("report serializes");
    if fs::create_dir_all("target/perf")
        .and_then(|()| fs::write("target/perf/report.json", report))
        .is_err()
    {
        eprintln!("não foi possível gravar target/perf/report.json");
        return ExitCode::FAILURE;
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
