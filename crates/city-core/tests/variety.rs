use city_core::city::{City, CityEvent, CityInput, CitySession, generate_city, generate_district};
use city_core::geometry::{Point, is_convex_ccw, point, segments_cross, signed_area};
use city_core::networks::Network;
use city_core::parcels::LandUse;
use city_core::regions;
use city_core::rng::Rng;
use city_core::sim::{Event, Simulation};
use city_core::streets::{self, Lattice, StreetClass, StreetOptions};
use city_core::terrain::Terrain;
use city_core::{DistrictInput, GenError, Profile, generate};

fn blocks(shape: f64, irregularity: f64, seed: u64) -> (Vec<Vec<Point>>, f64) {
    let size = 2_000.0;
    let boundary = vec![
        point(0.0, 0.0),
        point(size, 0.0),
        point(size, size),
        point(0.0, size),
    ];
    let layout = streets::build(
        &boundary,
        &StreetOptions {
            lattice: Lattice {
                block_area: 16_000.0,
                shape,
                irregularity,
            },
            interior: StreetClass::Local,
            bridge_every: 5,
            rivers: &[],
        },
        &Terrain::new(seed, 1.0),
        &mut Rng::derive(seed, "test"),
    );
    (layout.blocks, size)
}

fn on_city_edge(polygon: &[Point], size: f64) -> bool {
    polygon
        .iter()
        .any(|p| p.x < 1.0 || p.y < 1.0 || p.x > size - 1.0 || p.y > size - 1.0)
}

/// Sides that are really there, ignoring the slivers rounding leaves behind.
fn sides(polygon: &[Point]) -> usize {
    (0..polygon.len())
        .filter(|i| polygon[*i].distance(polygon[(i + 1) % polygon.len()]) > 1.0)
        .count()
}

fn interior_side_counts(shape: f64, irregularity: f64) -> Vec<usize> {
    let (cells, size) = blocks(shape, irregularity, 3);
    cells
        .iter()
        .filter(|cell| !on_city_edge(cell, size))
        .map(|cell| sides(cell))
        .collect()
}

fn custom(population: u64, tune: impl Fn(&mut Profile)) -> City {
    let mut profile = Profile::default();
    tune(&mut profile);
    generate_city(&CityInput {
        seed: 21,
        population,
        region: None,
        profile,
        extra_networks: Vec::new(),
    })
    .expect("valid city")
}

fn network<'a>(city: &'a City, id: &str) -> &'a Network {
    city.networks.iter().find(|n| n.id == id).expect("network")
}

fn cut(id: &str, segment: usize) -> CityEvent {
    CityEvent::CutTrunk {
        network: id.into(),
        segment,
    }
}

#[test]
fn shape_zero_gives_rectangles() {
    let counts = interior_side_counts(0.0, 0.0);
    assert!(!counts.is_empty());
    assert!(counts.iter().all(|c| *c == 4), "{counts:?}");
}

#[test]
fn shape_one_gives_regular_hexagons() {
    let (cells, size) = blocks(1.0, 0.0, 3);
    let interior: Vec<&Vec<Point>> = cells.iter().filter(|c| !on_city_edge(c, size)).collect();
    assert!(!interior.is_empty());
    for cell in interior {
        assert_eq!(sides(cell), 6);
        let lengths: Vec<f64> = (0..6)
            .map(|i| cell[i].distance(cell[(i + 1) % 6]))
            .collect();
        let (low, high) = lengths
            .iter()
            .fold((f64::MAX, f64::MIN), |(l, h), v| (l.min(*v), h.max(*v)));
        assert!(high - low < 0.01, "sides differ: {lengths:?}");
    }
}

#[test]
fn shapes_between_are_stretched_hexagons() {
    let (cells, size) = blocks(0.5, 0.0, 3);
    let interior: Vec<&Vec<Point>> = cells.iter().filter(|c| !on_city_edge(c, size)).collect();
    for cell in interior {
        assert_eq!(sides(cell), 6);
        let lengths: Vec<f64> = (0..6)
            .map(|i| cell[i].distance(cell[(i + 1) % 6]))
            .collect();
        let shortest = lengths.iter().copied().fold(f64::MAX, f64::min);
        let longest = lengths.iter().copied().fold(f64::MIN, f64::max);
        assert!(longest > shortest * 1.3, "looks regular: {lengths:?}");
    }
}

#[test]
fn irregularity_mixes_rectangles_pentagons_and_hexagons() {
    let counts = interior_side_counts(0.3, 0.8);
    for wanted in [4, 5, 6] {
        assert!(
            counts.contains(&wanted),
            "no {wanted}-sided block in {counts:?}"
        );
    }
}

#[test]
fn every_shape_partitions_the_ground_into_convex_blocks() {
    for step in 0..=10 {
        for irregularity in [0.0, 0.5, 1.0] {
            let (cells, size) = blocks(step as f64 / 10.0, irregularity, step);
            let area: f64 = cells.iter().map(|c| signed_area(c)).sum();
            assert!((area - size * size).abs() < 1.0, "lost ground: {area}");
            assert!(cells.iter().all(|c| is_convex_ccw(c)));
        }
    }
}

#[test]
fn every_shape_still_builds_a_fully_served_district() {
    for step in 0..=10 {
        let mut input = DistrictInput::square(step, 1_200.0);
        input.profile.block_shape = step as f64 / 10.0;
        let district = generate(&input).expect("valid");
        assert!(district.buildings.len() > 100, "shape {step}");
        let snapshot = Simulation::new(&district).snapshot(&district);
        assert!(snapshot.outages.values().all(Vec::is_empty), "shape {step}");
    }
}

#[test]
fn out_of_range_shapes_are_rejected() {
    let mut input = DistrictInput::square(1, 1_200.0);
    input.profile.block_shape = 1.5;
    assert_eq!(generate(&input).err(), Some(GenError::InvalidProfile));
}

#[test]
fn a_river_takes_its_blocks_and_only_bridges_cross_it() {
    let city = custom(400_000, |p| p.rivers = 1);
    let river = city.context.rivers[0].clone();
    let mut water_lots = 0;
    let mut bridges = 0;
    for plan in &city.districts {
        let district = generate_district(&city, plan.id).expect("generates");
        for (id, lot) in district.lots.iter().enumerate() {
            if lot.land_use == LandUse::Water {
                water_lots += 1;
                assert!(!district.buildings.iter().any(|b| b.lot == id));
            }
        }
        for edge in &district.street_edges {
            let a = district.street_nodes[edge.a].pos;
            let b = district.street_nodes[edge.b].pos;
            let crosses = river.windows(2).any(|w| segments_cross(a, b, w[0], w[1]));
            assert_eq!(crosses, edge.bridge, "district {}", plan.id);
            bridges += usize::from(edge.bridge);
        }
        let snapshot = Simulation::new(&district).snapshot(&district);
        assert!(
            snapshot.outages.values().all(Vec::is_empty),
            "district {} split",
            plan.id
        );
    }
    assert!(water_lots > 0 && bridges > 0, "{water_lots} {bridges}");
}

#[test]
fn a_fallen_bridge_takes_every_pipe_it_carries_but_a_street_strike_does_not() {
    let city = custom(400_000, |p| p.rivers = 1);
    let district = city
        .districts
        .iter()
        .map(|plan| generate_district(&city, plan.id).expect("generates"))
        .find(|d| {
            d.street_edges
                .iter()
                .any(|e| e.bridge && e.class == StreetClass::Local)
        })
        .expect("river crosses a district");
    let middle = |edge: usize| {
        let e = &district.street_edges[edge];
        (district.street_nodes[e.a].pos + district.street_nodes[e.b].pos) * 0.5
    };
    let bridge = (0..district.street_edges.len())
        .find(|e| {
            district.street_edges[*e].bridge
                && district.street_edges[*e].class == StreetClass::Local
        })
        .expect("bridge");
    let street = (0..district.street_edges.len())
        .find(|e| !district.street_edges[*e].bridge && middle(*e).distance(middle(bridge)) > 300.0)
        .expect("street");

    let strike = |at: Point| {
        let event = Event::Destroy {
            center: at,
            radius: 1.0,
            max_depth: 0.0,
        };
        Simulation::new(&district)
            .apply(&district, &event)
            .expect("valid")
    };
    let on_bridge = strike(middle(bridge));
    assert!(!on_bridge.broken["water"].is_empty());
    assert!(!on_bridge.broken["gas"].is_empty());
    let on_street = strike(middle(street));
    assert!(on_street.broken["water"].is_empty());
    assert!(on_street.broken["gas"].is_empty());
}

#[test]
fn the_same_seed_differs_between_regions_and_unknown_regions_fail() {
    let build = |region: &str| {
        generate_city(&CityInput {
            seed: 1,
            population: 400_000,
            region: Some(region.into()),
            profile: regions::find(region).expect("region").profile,
            extra_networks: Vec::new(),
        })
    };
    let brazil = build("BR-SE").expect("valid");
    let japan = build("JP-KT").expect("valid");
    assert_ne!(brazil.seed, japan.seed);
    assert!(!brazil.networks.iter().any(|n| n.id == "gas"));
    assert!(japan.networks.iter().any(|n| n.id == "gas"));

    let mut unknown = CityInput {
        seed: 1,
        population: 400_000,
        region: Some("ZZ".into()),
        profile: Profile::default(),
        extra_networks: Vec::new(),
    };
    assert_eq!(
        generate_city(&unknown).err(),
        Some(GenError::UnknownRegion("ZZ".into()))
    );
    unknown.region = None;
    assert!(generate_city(&unknown).is_ok());
}

#[test]
fn every_region_builds_a_valid_city() {
    for region in regions::regions() {
        let city = generate_city(&CityInput {
            seed: 8,
            population: 2_000_000,
            region: Some(region.id.into()),
            profile: region.profile.clone(),
            extra_networks: Vec::new(),
        })
        .expect("valid");
        for plan in city.districts.iter().step_by(25) {
            let district = generate_district(&city, plan.id).expect(region.id);
            let snapshot = Simulation::new(&district).snapshot(&district);
            assert!(
                snapshot.outages.values().all(Vec::is_empty),
                "{}",
                region.id
            );
        }
    }
}

#[test]
fn small_cities_have_no_metro_and_large_ones_do() {
    let lines = |p: &mut Profile| {
        p.metro_density = 1.0;
        p.rail_density = 1.0;
    };
    let small = custom(100_000, lines);
    assert!(
        !small
            .networks
            .iter()
            .any(|n| n.id == "metro" || n.id == "rail")
    );
    let large = custom(5_000_000, lines);
    assert!(large.networks.iter().any(|n| n.id == "metro"));
    assert!(large.networks.iter().any(|n| n.id == "rail"));
}

#[test]
fn a_blackout_stops_the_metro_without_breaking_a_tunnel() {
    let city = custom(5_000_000, |p| p.metro_density = 1.0);
    let power = network(&city, "power");
    let links: Vec<usize> = (0..power.segments.len())
        .filter(|s| power.segments[*s].street_edge.is_none())
        .collect();
    let mut session = CitySession::new(city);
    let mut last = session.snapshot();
    for link in links {
        last = session.apply(&cut("power", link)).expect("valid");
    }
    assert!(last.trunk.broken["metro"].is_empty());
    assert_eq!(
        last.trunk.outages["metro"].len(),
        session.city().districts.len()
    );
}

#[test]
fn a_tunnel_survives_a_surface_strike_but_not_a_deep_one() {
    let city = custom(5_000_000, |p| p.metro_density = 1.0);
    let metro = network(&city, "metro");
    let tunnel = metro
        .segments
        .iter()
        .position(|s| !s.overhead && s.depth > 10.0)
        .expect("tunnel");
    let at = (metro.segments[tunnel].pa + metro.segments[tunnel].pb) * 0.5;
    let depth = metro.segments[tunnel].depth;

    let strike = |max_depth: f64, city: City| {
        let mut session = CitySession::new(city);
        session
            .apply(&CityEvent::Destroy {
                center: at,
                radius: 5.0,
                max_depth,
            })
            .expect("valid")
    };
    let surface = strike(3.0, city.clone());
    assert!(!surface.trunk.broken["metro"].contains(&tunnel));
    let deep = strike(depth + 1.0, city);
    assert!(deep.trunk.broken["metro"].contains(&tunnel));
}

#[test]
fn viaducts_stand_above_ground_where_the_valley_drops() {
    let city = (0..20)
        .map(|seed| {
            let profile = Profile {
                metro_density: 3.0,
                ..Profile::default()
            };
            generate_city(&CityInput {
                seed,
                population: 20_000_000,
                region: None,
                profile,
                extra_networks: Vec::new(),
            })
            .expect("valid")
        })
        .find(|c| network(c, "metro").segments.iter().any(|s| s.overhead))
        .expect("some line needs a viaduct");
    for segment in network(&city, "metro")
        .segments
        .iter()
        .filter(|s| s.overhead)
    {
        assert_eq!(segment.depth, 0.0);
    }
}

#[test]
fn isolating_a_station_stops_trains_only_for_the_districts_it_serves() {
    let city = custom(5_000_000, |p| p.rail_density = 1.0);
    let rail = network(&city, "rail");
    let index = city
        .networks
        .iter()
        .position(|n| n.id == "rail")
        .expect("rail");
    let station = rail
        .powered_nodes
        .iter()
        .copied()
        .find(|node| {
            let degree = rail
                .segments
                .iter()
                .filter(|s| s.a == *node || s.b == *node)
                .count();
            let serves = city.districts.iter().any(|d| d.ports[index].node == *node);
            degree == 2 && serves && !rail.sources.contains(node)
        })
        .expect("mid-line station");
    let around: Vec<usize> = (0..rail.segments.len())
        .filter(|s| rail.segments[*s].a == station || rail.segments[*s].b == station)
        .collect();
    let expected: Vec<usize> = city
        .districts
        .iter()
        .filter(|d| d.ports[index].node == station)
        .map(|d| d.id)
        .collect();

    let mut session = CitySession::new(city);
    let mut last = session.snapshot();
    for segment in around {
        last = session.apply(&cut("rail", segment)).expect("valid");
    }
    assert_eq!(last.trunk.outages["rail"], expected);
}

#[test]
fn rail_corridors_replace_buildings_where_the_line_runs() {
    let city = custom(5_000_000, |p| p.rail_density = 1.0);
    assert!(!city.context.rails.is_empty());
    let mut corridor_blocks = 0;
    for plan in &city.districts {
        let district = generate_district(&city, plan.id).expect("generates");
        for (id, lot) in district.lots.iter().enumerate() {
            if lot.land_use == LandUse::Rail {
                corridor_blocks += 1;
                assert!(!district.buildings.iter().any(|b| b.lot == id));
            }
        }
    }
    assert!(corridor_blocks > 0);
}

#[test]
fn a_user_network_cannot_declare_transit() {
    let mut spec = city_core::networks::default_specs(&Profile::default()).remove(0);
    spec.id = "monorail".into();
    spec.depth = 40.0;
    spec.topology = city_core::networks::Topology::Transit;
    let result = generate_city(&CityInput {
        seed: 1,
        population: 400_000,
        region: None,
        profile: Profile::default(),
        extra_networks: vec![spec],
    });
    assert_eq!(
        result.err(),
        Some(GenError::TransitNotConfigurable("monorail".into()))
    );
}
