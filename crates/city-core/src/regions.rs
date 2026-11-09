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
                mean_temperature: 21.0,
                annual_rainfall: 1500.0,
                bedrock_depth: 12.0,
                seismic_risk: 0.05,
                relief: 1.2,
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
                mean_temperature: 11.0,
                annual_rainfall: 1200.0,
                bedrock_depth: 10.0,
                seismic_risk: 0.1,
                relief: 0.6,
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
                mean_temperature: 11.0,
                annual_rainfall: 800.0,
                bedrock_depth: 20.0,
                seismic_risk: 0.05,
                relief: 0.6,
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
                mean_temperature: 15.0,
                annual_rainfall: 1500.0,
                bedrock_depth: 25.0,
                seismic_risk: 0.9,
                relief: 1.0,
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
                mean_temperature: 18.0,
                annual_rainfall: 1000.0,
                bedrock_depth: 15.0,
                seismic_risk: 0.0,
                relief: 0.5,
            },
        },
    ]
}

pub fn find(id: &str) -> Option<Region> {
    regions().into_iter().find(|region| region.id == id)
}
