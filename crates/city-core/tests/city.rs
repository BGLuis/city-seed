use city_core::city::{
    City, CityEvent, CityInput, CitySession, city_hash, generate_city, generate_district,
};
use city_core::district_hash;
use city_core::geometry::{Point, centroid};
use city_core::networks::Network;
use city_core::sim::{HazardKind, Simulation};
use city_core::{GenError, Profile};

fn input(seed: u64, population: u64) -> CityInput {
    CityInput {
        seed,
        population,
        profile: Profile::default(),
        extra_networks: Vec::new(),
    }
}

fn city(seed: u64) -> City {
    generate_city(&input(seed, 400_000)).expect("valid city")
}

fn network<'a>(city: &'a City, id: &str) -> (usize, &'a Network) {
    let index = city
        .networks
        .iter()
        .position(|n| n.id == id)
        .expect("network");
    (index, &city.networks[index])
}

fn detail_all(session: &mut CitySession) {
    for id in 0..session.city().districts.len() {
        session.detail(id).expect("district generates");
    }
}

fn cut_trunk(id: &str, segment: usize) -> CityEvent {
    CityEvent::CutTrunk {
        network: id.into(),
        segment,
    }
}

#[test]
fn a_small_population_is_a_single_neighbourhood_and_a_megacity_has_thousands_of_districts() {
    assert_eq!(
        generate_city(&input(9, 3_000))
            .expect("valid")
            .districts
            .len(),
        1
    );
    let megacity = generate_city(&input(9, 40_000_000)).expect("valid");
    assert!(
        megacity.districts.len() > 1_000,
        "{}",
        megacity.districts.len()
    );
}

#[test]
fn populations_beyond_the_target_are_rejected() {
    assert_eq!(
        generate_city(&input(1, 40_000_001)).err(),
        Some(GenError::InvalidPopulation)
    );
    assert_eq!(
        generate_city(&input(1, 999)).err(),
        Some(GenError::InvalidPopulation)
    );
}

#[test]
fn a_district_does_not_depend_on_what_was_generated_before_it() {
    let first = city(5);
    let second = city(5);
    assert_eq!(city_hash(&first), city_hash(&second));
    let count = first.districts.len();
    let forward: Vec<u64> = (0..count)
        .map(|id| district_hash(&generate_district(&first, id).expect("generates")))
        .collect();
    let backward: Vec<u64> = (0..count)
        .rev()
        .map(|id| district_hash(&generate_district(&second, id).expect("generates")))
        .collect();
    assert_eq!(forward, backward.into_iter().rev().collect::<Vec<_>>());
}

#[test]
fn neighbouring_districts_share_the_same_ground() {
    let city = city(5);
    let a = generate_district(&city, 0).expect("generates");
    let b = generate_district(&city, 1).expect("generates");
    for node in a.street_nodes.iter().chain(&b.street_nodes) {
        let macro_twin = city.nodes.iter().find(|n| n.pos.distance(node.pos) < 0.01);
        if let Some(twin) = macro_twin {
            assert_eq!(twin.elevation, node.elevation);
        }
    }
}

#[test]
fn every_district_is_generated_and_served_by_every_network() {
    let city = city(11);
    assert!(city.districts.len() > 20);
    for plan in &city.districts {
        let district = generate_district(&city, plan.id).expect("district generates");
        assert!(
            !district.buildings.is_empty(),
            "district {} is empty",
            plan.id
        );
        let snapshot = Simulation::new(&district).snapshot(&district);
        assert!(
            snapshot.outages.values().all(Vec::is_empty),
            "district {}",
            plan.id
        );
    }
    let snapshot = CitySession::new(city).snapshot();
    assert!(snapshot.trunk.outages.values().all(Vec::is_empty));
}

#[test]
fn sewage_descends_from_every_street_through_the_trunk_to_a_treatment_plant() {
    let city = city(11);
    let (_, trunk) = network(&city, "sewer");
    for segment in &trunk.segments {
        assert!(segment.za > segment.zb, "trunk pipe does not descend");
    }
    for leaving in &trunk.segments {
        for arriving in trunk.segments.iter().filter(|s| s.b == leaving.a) {
            assert!(arriving.zb >= leaving.za || trunk.powered_nodes.contains(&leaving.a));
        }
    }
    for plan in &city.districts {
        let district = generate_district(&city, plan.id).expect("generates");
        let local = district
            .networks
            .iter()
            .find(|n| n.id == "sewer")
            .expect("sewer");
        let port = plan
            .ports
            .iter()
            .find(|p| p.network == "sewer")
            .expect("port");
        let trunk_level = port.level.expect("gravity ports carry a level");
        let outfall = local
            .segments
            .iter()
            .find(|s| local.sources.contains(&s.b))
            .expect("district drains out");
        assert!(
            outfall.zb >= trunk_level,
            "district {} drains below its trunk",
            plan.id
        );
    }
}

#[test]
fn losing_a_substation_blacks_out_exactly_the_districts_it_feeds() {
    let city = city(11);
    let (index, power) = network(&city, "power");
    assert!(power.sources.len() > 1);
    let link = power
        .segments
        .iter()
        .position(|s| s.street_edge.is_none())
        .expect("substation link");

    let mut fed = vec![power.segments[link].b];
    let mut cursor = 0;
    while cursor < fed.len() {
        let node = fed[cursor];
        fed.extend(power.segments.iter().filter(|s| s.a == node).map(|s| s.b));
        cursor += 1;
    }
    let expected: Vec<usize> = city
        .districts
        .iter()
        .filter(|plan| fed.contains(&plan.ports[index].node))
        .map(|plan| plan.id)
        .collect();
    assert!(!expected.is_empty() && expected.len() < city.districts.len());

    let mut session = CitySession::new(city);
    let snapshot = session.apply(&cut_trunk("power", link)).expect("valid");
    assert_eq!(snapshot.trunk.outages["power"], expected);
}

#[test]
fn the_trunk_verdict_matches_every_building_once_districts_are_detailed() {
    let city = city(11);
    let (_, power) = network(&city, "power");
    let link = power
        .segments
        .iter()
        .position(|s| s.street_edge.is_none())
        .expect("substation link");
    let mut session = CitySession::new(city);
    detail_all(&mut session);
    let snapshot = session.apply(&cut_trunk("power", link)).expect("valid");

    let dark = &snapshot.trunk.outages["power"];
    assert!(!dark.is_empty());
    for (id, local) in &snapshot.districts {
        let buildings = session.detail(*id).expect("detailed").buildings.len();
        let expected = if dark.contains(id) { buildings } else { 0 };
        assert_eq!(local.outages["power"].len(), expected, "district {id}");
    }
}

#[test]
fn a_district_detailed_after_the_failure_still_sees_it() {
    let city = city(11);
    let (_, power) = network(&city, "power");
    let link = power
        .segments
        .iter()
        .position(|s| s.street_edge.is_none())
        .expect("substation link");
    let mut session = CitySession::new(city);
    let snapshot = session.apply(&cut_trunk("power", link)).expect("valid");
    let dark = snapshot.trunk.outages["power"][0];
    let buildings = session.detail(dark).expect("generates").buildings.len();
    assert_eq!(
        session.snapshot().districts[&dark].outages["power"].len(),
        buildings
    );
}

#[test]
fn a_looped_trunk_survives_any_single_street_cut() {
    let city = city(11);
    let (_, gas) = network(&city, "gas");
    let streets: Vec<usize> = (0..gas.segments.len())
        .filter(|s| gas.segments[*s].street_edge.is_some())
        .collect();
    let mut session = CitySession::new(city);
    for segment in streets {
        let snapshot = session.apply(&cut_trunk("gas", segment)).expect("valid");
        assert!(
            snapshot.trunk.outages["gas"].is_empty(),
            "segment {segment}"
        );
        session.reset();
    }
}

#[test]
fn a_regional_blackout_stops_trunk_lifts_without_breaking_a_pipe() {
    let city = (0..12)
        .map(|seed| generate_city(&input(seed, 3_000_000)).expect("valid"))
        .find(|c| !network(c, "sewer").1.powered_nodes.is_empty())
        .expect("some city needs trunk lifts");
    let (_, power) = network(&city, "power");
    let links: Vec<usize> = (0..power.segments.len())
        .filter(|s| power.segments[*s].street_edge.is_none())
        .collect();
    let mut session = CitySession::new(city);
    let mut last = session.snapshot();
    for link in links {
        last = session.apply(&cut_trunk("power", link)).expect("valid");
    }
    assert!(last.trunk.broken["sewer"].is_empty());
    assert!(!last.trunk.outages["sewer"].is_empty());
}

#[test]
fn a_strike_on_a_border_details_and_damages_both_sides_only() {
    let city = city(11);
    let first = city.districts[0].polygon.clone();
    let border: Point = (first[0] + first[1]) * 0.5;
    let total = city.districts.len();
    let mut session = CitySession::new(city);
    let snapshot = session
        .apply(&CityEvent::Destroy {
            center: border,
            radius: 120.0,
            max_depth: 1.0,
        })
        .expect("valid");
    assert!(snapshot.districts.len() >= 2 || border.y == 0.0 || border.x == 0.0);
    assert!(snapshot.districts.len() < total);
    assert!(snapshot.districts.values().any(|d| !d.destroyed.is_empty()));
}

#[test]
fn an_explosion_reaches_every_district_it_touches() {
    let mut request = input(11, 400_000);
    request.profile.overhead_share = 1.0;
    let city = generate_city(&request).expect("valid");
    let center = centroid(&city.districts[3].polygon);
    let polygons: Vec<Vec<Point>> = city.districts.iter().map(|d| d.polygon.clone()).collect();
    let mut session = CitySession::new(city);
    let snapshot = session
        .apply(&CityEvent::Destroy {
            center,
            radius: 700.0,
            max_depth: 1.0,
        })
        .expect("valid");

    let explosions: Vec<Point> = snapshot
        .districts
        .values()
        .chain([&snapshot.trunk])
        .flat_map(|s| &s.hazards)
        .filter(|h| h.kind == HazardKind::Explosion)
        .map(|h| h.at)
        .collect();
    assert!(!explosions.is_empty());
    for at in explosions {
        for (id, polygon) in polygons.iter().enumerate() {
            let inside = polygon
                .iter()
                .zip(polygon.iter().cycle().skip(1))
                .all(|(a, b)| (*b - *a).cross(at - *a) >= 0.0);
            if inside {
                assert!(
                    snapshot.districts.contains_key(&id),
                    "district {id} was skipped"
                );
            }
        }
    }
}

#[test]
fn replaying_the_same_events_reproduces_the_same_damage() {
    let events = [
        CityEvent::Destroy {
            center: Point {
                x: 5_000.0,
                y: 5_000.0,
            },
            radius: 300.0,
            max_depth: 2.0,
        },
        cut_trunk("power", 0),
    ];
    let run = |reverse_detail: bool| {
        let mut session = CitySession::new(city(11));
        if reverse_detail {
            detail_all(&mut session);
        }
        let mut last = None;
        for event in &events {
            last = Some(session.apply(event).expect("valid"));
        }
        serde_json::to_string(&last.expect("events ran").trunk).expect("serializes")
    };
    assert_eq!(run(false), run(false));
    assert_eq!(run(false), run(true));
}
