use city_core::city::{City, CityInput, generate_city, generate_district};
use city_core::environment::{Environment, Soil};
use city_core::geometry::{centroid, point};
use city_core::networks::{Network, Topology};
use city_core::parcels::LandUse;
use city_core::sim::Simulation;
use city_core::terrain::Terrain;
use city_core::{District, DistrictInput, GenError, Profile, generate};

fn district(seed: u64, tune: impl Fn(&mut Profile)) -> District {
    let mut input = DistrictInput::square(seed, 1_500.0);
    tune(&mut input.profile);
    generate(&input).expect("valid")
}

fn city(population: u64, tune: impl Fn(&mut Profile)) -> City {
    let mut profile = Profile::default();
    tune(&mut profile);
    generate_city(&CityInput {
        seed: 33,
        population,
        region: None,
        profile,
        extra_networks: Vec::new(),
    })
    .expect("valid")
}

fn network<'a>(district: &'a District, id: &str) -> &'a Network {
    district
        .networks
        .iter()
        .find(|n| n.id == id)
        .expect("network")
}

fn count_lots(district: &District, land_use: LandUse) -> usize {
    district
        .lots
        .iter()
        .filter(|l| l.land_use == land_use)
        .count()
}

fn assert_nothing_built_on(district: &District, land_use: LandUse) {
    for (id, lot) in district.lots.iter().enumerate() {
        if lot.land_use == land_use {
            assert!(!district.buildings.iter().any(|b| b.lot == id));
        }
    }
}

#[test]
fn cold_climates_bury_every_network_below_the_frost_line() {
    let warm = district(4, |p| p.mean_temperature = 22.0);
    let frozen = district(4, |p| p.mean_temperature = -6.0);
    for id in ["water", "gas", "sewer", "drainage"] {
        let shallow = warm.specs.iter().find(|s| s.id == id).expect("spec").depth;
        let deep = frozen
            .specs
            .iter()
            .find(|s| s.id == id)
            .expect("spec")
            .depth;
        assert!(deep >= shallow + 2.0, "{id}: {shallow} -> {deep}");
    }
    let snapshot = Simulation::new(&frozen).snapshot(&frozen);
    assert!(snapshot.outages.values().all(Vec::is_empty));
}

#[test]
fn shallow_bedrock_forces_more_sewage_lifts_and_caps_every_trench() {
    let (mut shallow_total, mut deep_total) = (0, 0);
    for seed in 0..12 {
        let shallow = district(seed, |p| p.bedrock_depth = 4.0);
        let deep = district(seed, |p| p.bedrock_depth = 40.0);
        shallow_total += network(&shallow, "sewer").powered_nodes.len();
        deep_total += network(&deep, "sewer").powered_nodes.len();

        let spec = shallow
            .specs
            .iter()
            .find(|s| s.id == "sewer")
            .expect("spec");
        let Topology::Gravity { max_depth, .. } = spec.topology else {
            panic!("sewer is gravity-fed");
        };
        assert!(
            network(&shallow, "sewer")
                .segments
                .iter()
                .all(|s| s.depth <= max_depth)
        );
    }
    assert!(
        shallow_total > deep_total,
        "{shallow_total} vs {deep_total}"
    );
}

#[test]
fn steep_hillsides_stay_wooded() {
    let flat = district(9, |p| p.relief = 0.2);
    let mountain = district(9, |p| p.relief = 6.0);
    assert_eq!(count_lots(&flat, LandUse::Forest), 0);
    assert!(count_lots(&mountain, LandUse::Forest) > 0);
    assert_nothing_built_on(&mountain, LandUse::Forest);
    assert!(mountain.buildings.len() < flat.buildings.len());
}

#[test]
fn floodplains_stay_empty_only_where_it_rains() {
    let flat = |rain: f64| {
        city(400_000, move |p| {
            p.rivers = 2;
            p.relief = 0.2;
            p.annual_rainfall = rain;
        })
    };
    let wet = flat(3_000.0);
    let wet_lots: usize = wet
        .districts
        .iter()
        .map(|plan| {
            let district = generate_district(&wet, plan.id).expect("generates");
            assert_nothing_built_on(&district, LandUse::Wetland);
            count_lots(&district, LandUse::Wetland)
        })
        .sum();
    assert!(wet_lots > 0);

    let dry = flat(100.0);
    for plan in &dry.districts {
        let district = generate_district(&dry, plan.id).expect("generates");
        assert_eq!(count_lots(&district, LandUse::Wetland), 0);
    }
}

#[test]
fn only_rainy_cities_build_storm_drains() {
    let arid = district(2, |p| p.annual_rainfall = 100.0);
    let humid = district(2, |p| p.annual_rainfall = 1_800.0);
    assert!(!arid.networks.iter().any(|n| n.id == "drainage"));
    assert!(humid.networks.iter().any(|n| n.id == "drainage"));
}

#[test]
fn storm_drains_only_flow_downhill_unless_pumped() {
    let district = district(5, |_| {});
    let drains = network(&district, "drainage");
    for segment in &drains.segments {
        assert!(segment.za > segment.zb);
    }
    for leaving in &drains.segments {
        for arriving in drains.segments.iter().filter(|s| s.b == leaving.a) {
            assert!(arriving.zb >= leaving.za || drains.powered_nodes.contains(&leaving.a));
        }
    }
}

#[test]
fn earthquakes_and_soft_ground_lower_the_skyline() {
    let tallest = |seismic: f64| {
        let built = city(5_000_000, move |p| {
            p.seismic_risk = seismic;
            p.density = 1.0;
        });
        let center = built
            .districts
            .iter()
            .max_by(|a, b| a.density.total_cmp(&b.density))
            .expect("districts");
        let district = generate_district(&built, center.id).expect("generates");
        district
            .buildings
            .iter()
            .map(|b| b.floors)
            .max()
            .expect("buildings")
    };
    assert!(tallest(1.0) < tallest(0.0));

    let soft = district(3, |p| p.bedrock_depth = 60.0);
    for building in &soft.buildings {
        let limit = match soft.lots[building.lot].soil {
            Soil::Rocky => 120,
            Soil::Clay => 40,
            Soil::Sand => 25,
            Soil::Alluvial => 12,
        };
        assert!(building.floors <= limit);
    }
}

#[test]
fn soil_follows_rivers_and_bedrock() {
    let profile = Profile {
        bedrock_depth: 2.0,
        ..Profile::default()
    };
    let terrain = Terrain::new(1, 1.0);
    let river = vec![vec![point(0.0, 0.0), point(10_000.0, 0.0)]];
    let environment = Environment::new(1, &profile, &terrain, &river, None);
    assert_eq!(environment.soil(point(5_000.0, 200.0)), Soil::Alluvial);
    assert_eq!(environment.soil(point(5_000.0, 5_000.0)), Soil::Rocky);
}

#[test]
fn higher_ground_is_colder_and_dense_cores_are_warmer() {
    let flat = Profile::default();
    let hills = Profile {
        relief: 4.0,
        ..Profile::default()
    };
    let low = Terrain::new(7, 0.1);
    let high = Terrain::new(7, hills.relief);
    let p = point(3_000.0, 3_000.0);
    let cold = Environment::new(7, &hills, &high, &[], None).sample(p);
    let warm = Environment::new(7, &flat, &low, &[], None).sample(p);
    assert!(cold.temperature < warm.temperature);

    let built = city(5_000_000, |_| {});
    let terrain = Terrain::new(built.context.terrain_seed, built.profile.relief);
    let urban = Environment::new(
        built.context.terrain_seed,
        &built.profile,
        &terrain,
        &[],
        Some(&built.context.density),
    );
    let rural = Environment::new(
        built.context.terrain_seed,
        &built.profile,
        &terrain,
        &[],
        None,
    );
    let core = centroid(&built.boundary);
    assert!(urban.sample(core).temperature > rural.sample(core).temperature + 1.0);
}

#[test]
fn deserts_grow_less_than_rainforests() {
    let terrain = Terrain::new(2, 1.0);
    let p = point(4_000.0, 4_000.0);
    let vegetation = |rain: f64, temperature: f64| {
        let profile = Profile {
            annual_rainfall: rain,
            mean_temperature: temperature,
            ..Profile::default()
        };
        Environment::new(2, &profile, &terrain, &[], None)
            .sample(p)
            .vegetation
    };
    assert!(vegetation(100.0, 30.0) < vegetation(2_500.0, 24.0));
}

#[test]
fn neighbouring_districts_read_the_same_soil() {
    let built = city(400_000, |p| p.rivers = 1);
    let first = generate_district(&built, 0).expect("generates");
    let second = generate_district(&built, 1).expect("generates");
    let terrain = Terrain::new(built.context.terrain_seed, built.profile.relief);
    let environment = Environment::new(
        built.context.terrain_seed,
        &built.profile,
        &terrain,
        &built.context.rivers,
        Some(&built.context.density),
    );
    for district in [first, second] {
        for lot in district
            .lots
            .iter()
            .filter(|l| l.land_use == LandUse::Residential)
        {
            assert_eq!(lot.soil, environment.soil(centroid(&lot.polygon)));
        }
    }
}

#[test]
fn metro_avoids_tunnelling_through_river_mud() {
    let built = city(8_000_000, |p| {
        p.metro_density = 2.0;
        p.rivers = 3;
    });
    let terrain = Terrain::new(built.context.terrain_seed, built.profile.relief);
    let environment = Environment::new(
        built.context.terrain_seed,
        &built.profile,
        &terrain,
        &built.context.rivers,
        Some(&built.context.density),
    );
    let metro = built
        .networks
        .iter()
        .find(|n| n.id == "metro")
        .expect("metro");
    let in_mud: Vec<_> = metro
        .segments
        .iter()
        .filter(|s| {
            environment.soil(s.pa) == Soil::Alluvial && environment.soil(s.pb) == Soil::Alluvial
        })
        .collect();
    assert!(!in_mud.is_empty(), "no line reaches a river");
    assert!(in_mud.iter().all(|s| s.overhead));
}

#[test]
fn impossible_natural_values_are_rejected() {
    for tune in [
        |p: &mut Profile| p.seismic_risk = 1.5,
        |p: &mut Profile| p.annual_rainfall = -1.0,
        |p: &mut Profile| p.bedrock_depth = 0.0,
        |p: &mut Profile| p.relief = -1.0,
    ] {
        let mut input = DistrictInput::square(1, 1_000.0);
        tune(&mut input.profile);
        assert_eq!(generate(&input).err(), Some(GenError::InvalidProfile));
    }
}
