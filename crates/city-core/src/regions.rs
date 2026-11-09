use crate::Profile;
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct Region {
    pub id: &'static str,
    pub label: &'static str,
    pub profile: Profile,
}

pub fn regions() -> Vec<Region> {
    vec![
        Region {
            id: "BR-SE",
            label: "Brasil (Sudeste)",
            profile: Profile {
                overhead_share: 0.8,
                piped_gas: false,
                park_share: 0.05,
                density: 0.5,
                block_shape: 0.0,
                street_irregularity: 0.45,
                rivers: 2,
                metro_density: 0.6,
                rail_density: 0.6,
            },
        },
        Region {
            id: "US-NE",
            label: "EUA (Nordeste)",
            profile: Profile {
                overhead_share: 0.5,
                piped_gas: true,
                park_share: 0.08,
                density: 0.5,
                block_shape: 0.0,
                street_irregularity: 0.03,
                rivers: 1,
                metro_density: 0.5,
                rail_density: 0.8,
            },
        },
        Region {
            id: "EU-W",
            label: "Europa Ocidental",
            profile: Profile {
                overhead_share: 0.05,
                piped_gas: true,
                park_share: 0.12,
                density: 0.5,
                block_shape: 0.35,
                street_irregularity: 0.7,
                rivers: 1,
                metro_density: 1.0,
                rail_density: 1.0,
            },
        },
        Region {
            id: "JP-KT",
            label: "Japão (Kanto)",
            profile: Profile {
                overhead_share: 0.85,
                piped_gas: true,
                park_share: 0.05,
                density: 0.5,
                block_shape: 0.2,
                street_irregularity: 0.55,
                rivers: 2,
                metro_density: 1.4,
                rail_density: 1.6,
            },
        },
        Region {
            id: "XX-HEX",
            label: "Colmeia (fictícia)",
            profile: Profile {
                overhead_share: 0.3,
                piped_gas: true,
                park_share: 0.1,
                density: 0.5,
                block_shape: 1.0,
                street_irregularity: 0.0,
                rivers: 1,
                metro_density: 1.0,
                rail_density: 1.0,
            },
        },
    ]
}

pub fn find(id: &str) -> Option<Region> {
    regions().into_iter().find(|region| region.id == id)
}
