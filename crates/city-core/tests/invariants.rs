use city_core::geometry::point;
use city_core::networks::{Network, NetworkSpec, Topology};
use city_core::sim::{Event, HazardKind, Simulation, Snapshot};
use city_core::{District, DistrictInput, GenError, Port, district_hash, generate};

const SEEDS: [u64; 6] = [1, 7, 42, 1337, 90210, 424242];

fn city(seed: u64) -> District {
    generate(&DistrictInput::square(seed, 1200.0)).expect("valid input")
}

fn network<'a>(district: &'a District, id: &str) -> &'a Network {
    district
        .networks
        .iter()
        .find(|n| n.id == id)
        .expect("network exists")
}

fn trunk_index(network: &Network) -> usize {
    network
        .segments
        .iter()
        .position(|s| s.street_edge.is_none() && (s.a == network.source || s.b == network.source))
        .expect("network has a trunk")
}

fn cut(district: &District, id: &str, segment: usize) -> Snapshot {
    let event = Event::Cut {
        network: id.into(),
        segment,
    };
    Simulation::new(district)
        .apply(district, &event)
        .expect("valid event")
}

fn count(snapshot: &Snapshot, kind: HazardKind) -> usize {
    snapshot.hazards.iter().filter(|h| h.kind == kind).count()
}

fn telecom() -> NetworkSpec {
    NetworkSpec {
        id: "telecom".into(),
        label: "Internet".into(),
        color: "#b48cff".into(),
        topology: Topology::Radial,
        depth: 0.45,
        lateral_offset: -3.5,
        overhead_height: None,
        requires: None,
        on_rupture: Vec::new(),
    }
}

#[test]
fn same_seed_reproduces_the_exact_city_and_other_seeds_do_not() {
    assert_eq!(district_hash(&city(42)), district_hash(&city(42)));
    assert_ne!(district_hash(&city(42)), district_hash(&city(43)));
}

#[test]
fn a_fresh_city_has_every_building_served_by_every_network() {
    for seed in SEEDS {
        let district = city(seed);
        assert!(district.buildings.len() > 200, "seed {seed} is too empty");
        let snapshot = Simulation::new(&district).snapshot(&district);
        for (id, lost) in &snapshot.outages {
            assert!(lost.is_empty(), "seed {seed}: {} lost {id}", lost.len());
        }
    }
}

#[test]
fn sewage_only_flows_downhill_unless_a_pump_lifts_it() {
    for seed in SEEDS {
        let district = city(seed);
        let sewer = network(&district, "sewer");
        for segment in &sewer.segments {
            assert!(
                segment.za > segment.zb,
                "seed {seed}: pipe does not descend"
            );
        }
        for leaving in &sewer.segments {
            for arriving in sewer.segments.iter().filter(|s| s.b == leaving.a) {
                assert!(
                    arriving.zb >= leaving.za || sewer.powered_nodes.contains(&leaving.a),
                    "seed {seed}: node {} rises without a pump",
                    leaving.a
                );
            }
        }
    }
}

#[test]
fn cutting_a_trunk_disconnects_only_that_network() {
    let district = city(42);
    let water = network(&district, "water");
    let snapshot = cut(&district, "water", trunk_index(water));
    assert_eq!(snapshot.outages["water"].len(), district.buildings.len());
    assert!(snapshot.outages["power"].is_empty());
    assert!(snapshot.outages["gas"].is_empty());
}

#[test]
fn a_looped_network_survives_any_single_street_cut() {
    for seed in SEEDS {
        let district = city(seed);
        let gas = network(&district, "gas");
        for (index, segment) in gas.segments.iter().enumerate() {
            if segment.street_edge.is_some() {
                let snapshot = cut(&district, "gas", index);
                assert!(
                    snapshot.outages["gas"].is_empty(),
                    "seed {seed}: segment {index}"
                );
            }
        }
    }
}

#[test]
fn cutting_a_radial_feeder_darkens_exactly_its_downstream_buildings() {
    let district = city(42);
    let power = network(&district, "power");
    let (index, _) = power
        .segments
        .iter()
        .enumerate()
        .filter(|(_, s)| s.street_edge.is_some())
        .max_by_key(|(_, s)| power.segments.iter().filter(|o| o.a == s.b).count())
        .expect("feeder exists");

    let mut downstream = vec![power.segments[index].b];
    let mut cursor = 0;
    while cursor < downstream.len() {
        let node = downstream[cursor];
        downstream.extend(power.segments.iter().filter(|s| s.a == node).map(|s| s.b));
        cursor += 1;
    }

    let expected: Vec<usize> = district
        .buildings
        .iter()
        .enumerate()
        .filter(|(_, b)| downstream.contains(&b.street_node))
        .map(|(i, _)| i)
        .collect();
    assert_eq!(cut(&district, "power", index).outages["power"], expected);
}

#[test]
fn a_blackout_stops_pumps_without_any_pipe_breaking() {
    let district = SEEDS
        .into_iter()
        .map(city)
        .find(|d| !network(d, "sewer").powered_nodes.is_empty())
        .expect("some seed needs a sewage lift");
    let power = network(&district, "power");
    let snapshot = cut(&district, "power", trunk_index(power));
    assert!(snapshot.broken["sewer"].is_empty());
    assert!(!snapshot.outages["sewer"].is_empty());
}

#[test]
fn a_gas_rupture_alone_leaks_but_does_not_explode() {
    let district = city(42);
    let segment = network(&district, "gas")
        .segments
        .iter()
        .position(|s| s.street_edge.is_some())
        .expect("gas main exists");
    let snapshot = cut(&district, "gas", segment);
    assert_eq!(count(&snapshot, HazardKind::GasLeak), 1);
    assert_eq!(count(&snapshot, HazardKind::Explosion), 0);
}

#[test]
fn a_fallen_overhead_line_ignites_a_nearby_gas_leak() {
    let mut input = DistrictInput::square(42, 1200.0);
    input.profile.overhead_share = 1.0;
    let district = generate(&input).expect("valid input");
    let event = Event::Destroy {
        center: point(600.0, 600.0),
        radius: 40.0,
        max_depth: 1.0,
    };
    let snapshot = Simulation::new(&district)
        .apply(&district, &event)
        .expect("valid event");
    assert!(count(&snapshot, HazardKind::ElectricRisk) > 0);
    assert!(count(&snapshot, HazardKind::Explosion) > 0);
}

#[test]
fn a_shallow_strike_does_not_reach_deep_networks() {
    let mut input = DistrictInput::square(42, 1200.0);
    input.profile.piped_gas = false;
    let district = generate(&input).expect("valid input");
    let event = Event::Destroy {
        center: point(600.0, 600.0),
        radius: 150.0,
        max_depth: 1.0,
    };
    let snapshot = Simulation::new(&district)
        .apply(&district, &event)
        .expect("valid event");
    assert!(!snapshot.broken["power"].is_empty());
    assert!(snapshot.broken["water"].is_empty());
    assert!(snapshot.broken["sewer"].is_empty());
}

#[test]
fn a_new_network_needs_only_a_spec() {
    let mut input = DistrictInput::square(42, 1200.0);
    input.extra_networks.push(telecom());
    let district = generate(&input).expect("valid input");
    let internet = network(&district, "telecom");
    let snapshot = cut(&district, "telecom", trunk_index(internet));
    assert_eq!(snapshot.outages["telecom"].len(), district.buildings.len());
    assert!(snapshot.outages["power"].is_empty());
}

#[test]
fn networks_that_would_collide_at_crossings_are_rejected() {
    let mut input = DistrictInput::square(42, 1200.0);
    let mut clashing = telecom();
    clashing.depth = 0.8;
    input.extra_networks.push(clashing);
    assert!(matches!(
        generate(&input),
        Err(GenError::DepthConflict(_, _))
    ));
}

#[test]
fn a_port_chosen_by_the_macro_level_is_honoured() {
    let mut input = DistrictInput::square(42, 1200.0);
    let at = point(-50.0, 600.0);
    input.ports.push(Port {
        network: "power".into(),
        at,
    });
    let district = generate(&input).expect("valid input");
    let power = network(&district, "power");
    assert_eq!(power.segments[trunk_index(power)].pa, at);
}

#[test]
fn a_city_wide_strike_settles() {
    let district = city(1337);
    let event = Event::Destroy {
        center: point(600.0, 600.0),
        radius: 2000.0,
        max_depth: 50.0,
    };
    let snapshot = Simulation::new(&district)
        .apply(&district, &event)
        .expect("valid event");
    assert_eq!(snapshot.destroyed.len(), district.buildings.len());
}

#[test]
fn invalid_boundaries_are_rejected() {
    let mut input = DistrictInput::square(1, 1200.0);
    input.boundary.reverse();
    assert_eq!(generate(&input).err(), Some(GenError::InvalidBoundary));
}
