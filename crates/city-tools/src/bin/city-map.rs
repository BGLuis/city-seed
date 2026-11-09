//! Renders a city from above: one composite image plus one image per layer.

use city_core::city::{City, CityInput, generate_city, generate_district};
use city_core::environment::{Environment, Sample, Soil};
use city_core::geometry::{Point, point};
use city_core::networks::Network;
use city_core::parcels::LandUse;
use city_core::regions;
use city_core::streets::{StreetClass, StreetEdge, StreetNode};
use city_core::terrain::Terrain;
use city_tools::{number, parse_args};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path as FsPath;
use tiny_skia::{
    Color, FillRule, FilterQuality, LineCap, Paint, Path, PathBuilder, Pixmap, PixmapPaint,
    PremultipliedColorU8, Stroke, Transform,
};

type Rgb = [u8; 3];

const BACKGROUND: Rgb = [244, 241, 234];
const MAX_FIELD_CELLS: u32 = 1024;
const RIVER_WIDTH: f32 = 200.0;
const WATER: Rgb = [58, 124, 176];

struct Sheet {
    title: String,
    note: String,
    pixmap: Pixmap,
}

struct Atlas {
    sheets: BTreeMap<String, Sheet>,
    scale: f32,
    size: u32,
}

impl Atlas {
    fn blank(&mut self, id: &str, title: &str, note: &str) {
        let mut pixmap = Pixmap::new(self.size, self.size).expect("positive size");
        let [r, g, b] = BACKGROUND;
        pixmap.fill(Color::from_rgba8(r, g, b, 255));
        self.sheets.insert(
            id.into(),
            Sheet {
                title: title.into(),
                note: note.into(),
                pixmap,
            },
        );
    }

    fn fill(&mut self, id: &str, path: Option<Path>, color: Rgb) {
        let (Some(path), Some(sheet)) = (path, self.sheets.get_mut(id)) else {
            return;
        };
        sheet.pixmap.fill_path(
            &path,
            &paint(color),
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }

    fn stroke(&mut self, id: &str, path: Option<&Path>, width: f32, color: Rgb) {
        let (Some(path), Some(sheet)) = (path, self.sheets.get_mut(id)) else {
            return;
        };
        let stroke = Stroke {
            width: width.max(0.6),
            line_cap: LineCap::Round,
            ..Stroke::default()
        };
        sheet
            .pixmap
            .stroke_path(path, &paint(color), &stroke, Transform::identity(), None);
    }

    fn polygons<'a>(&self, polygons: impl Iterator<Item = &'a [Point]>) -> Option<Path> {
        let mut builder = PathBuilder::new();
        for polygon in polygons {
            for (index, p) in polygon.iter().enumerate() {
                let (x, y) = (p.x as f32 * self.scale, p.y as f32 * self.scale);
                if index == 0 {
                    builder.move_to(x, y);
                } else {
                    builder.line_to(x, y);
                }
            }
            builder.close();
        }
        builder.finish()
    }

    fn lines(&self, lines: impl Iterator<Item = (Point, Point)>) -> Option<Path> {
        let mut builder = PathBuilder::new();
        for (a, b) in lines {
            builder.move_to(a.x as f32 * self.scale, a.y as f32 * self.scale);
            builder.line_to(b.x as f32 * self.scale, b.y as f32 * self.scale);
        }
        builder.finish()
    }
}

fn paint(color: Rgb) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color_rgba8(color[0], color[1], color[2], 255);
    paint.anti_alias = true;
    paint
}

fn ramp(stops: &[Rgb], t: f64) -> Rgb {
    let scaled = t.clamp(0.0, 1.0) * (stops.len() - 1) as f64;
    let index = (scaled as usize).min(stops.len() - 2);
    let local = scaled - index as f64;
    std::array::from_fn(|c| {
        (f64::from(stops[index][c])
            + (f64::from(stops[index + 1][c]) - f64::from(stops[index][c])) * local) as u8
    })
}

fn land_color(land_use: LandUse) -> Rgb {
    match land_use {
        LandUse::Residential => [214, 205, 186],
        LandUse::Commercial => [196, 176, 200],
        LandUse::Industrial => [186, 164, 140],
        LandUse::Park => [150, 196, 130],
        LandUse::Forest => [86, 140, 92],
        LandUse::Wetland => [140, 186, 170],
        LandUse::Water => WATER,
        LandUse::Rail => [120, 112, 104],
    }
}

fn building_color(land_use: LandUse, floors: u32) -> Rgb {
    let base: Rgb = match land_use {
        LandUse::Commercial => [122, 104, 150],
        LandUse::Industrial => [130, 104, 80],
        _ => [150, 132, 110],
    };
    let darkening = (f64::from(floors) / 40.0).min(1.0) * 0.55;
    base.map(|c| (f64::from(c) * (1.0 - darkening)) as u8)
}

struct Field {
    id: &'static str,
    title: &'static str,
    stops: &'static [Rgb],
    value: fn(&Sample) -> f64,
    unit: &'static str,
}

const SOIL_COLORS: [(Soil, Rgb, &str); 4] = [
    (Soil::Rocky, [120, 116, 112], "rochoso"),
    (Soil::Clay, [186, 120, 84], "argila"),
    (Soil::Sand, [226, 200, 140], "areia"),
    (Soil::Alluvial, [108, 150, 150], "aluvial"),
];

const FIELDS: [Field; 7] = [
    Field {
        id: "terrain",
        title: "Relevo",
        stops: &[
            [70, 120, 90],
            [190, 180, 120],
            [150, 110, 80],
            [240, 240, 240],
        ],
        value: |s| s.elevation,
        unit: "m",
    },
    Field {
        id: "slope",
        title: "Declividade",
        stops: &[[240, 240, 220], [230, 170, 90], [140, 40, 40]],
        value: |s| s.slope,
        unit: "m/m",
    },
    Field {
        id: "rock_depth",
        title: "Profundidade da rocha",
        stops: &[[90, 80, 80], [190, 160, 120], [240, 230, 200]],
        value: |s| s.rock_depth,
        unit: "m",
    },
    Field {
        id: "temperature",
        title: "Temperatura média",
        stops: &[[60, 90, 190], [240, 240, 200], [200, 50, 40]],
        value: |s| s.temperature,
        unit: "°C",
    },
    Field {
        id: "rainfall",
        title: "Chuva anual",
        stops: &[[240, 230, 190], [120, 190, 190], [30, 70, 150]],
        value: |s| s.rainfall,
        unit: "mm",
    },
    Field {
        id: "vegetation",
        title: "Vegetação",
        stops: &[[230, 220, 190], [150, 190, 110], [30, 100, 50]],
        value: |s| s.vegetation,
        unit: "0 a 1",
    },
    Field {
        id: "flood_risk",
        title: "Risco de inundação",
        stops: &[[245, 245, 240], [120, 180, 220], [20, 50, 130]],
        value: |s| s.flood_risk,
        unit: "0 a 1",
    },
];

fn paint_cells(atlas: &mut Atlas, id: &str, cells: u32, color: impl Fn(u32, u32) -> Rgb) {
    let mut field = Pixmap::new(cells, cells).expect("positive size");
    for y in 0..cells {
        for x in 0..cells {
            let [r, g, b] = color(x, y);
            field.pixels_mut()[(y * cells + x) as usize] =
                PremultipliedColorU8::from_rgba(r, g, b, 255).expect("opaque colors are valid");
        }
    }
    let scale = atlas.size as f32 / cells as f32;
    let quality = if id == "soil" {
        FilterQuality::Nearest
    } else {
        FilterQuality::Bilinear
    };
    let sheet = atlas.sheets.get_mut(id).expect("sheet created");
    sheet.pixmap.draw_pixmap(
        0,
        0,
        field.as_ref(),
        &PixmapPaint {
            quality,
            ..PixmapPaint::default()
        },
        Transform::from_scale(scale, scale),
        None,
    );
}

fn paint_environment(atlas: &mut Atlas, city: &City, side: f64) {
    let terrain = Terrain::new(city.context.terrain_seed, city.profile.relief);
    let environment = Environment::new(
        city.context.terrain_seed,
        &city.profile,
        &terrain,
        &city.context.rivers,
        Some(&city.context.density),
    );
    let cells = (atlas.size / 2).clamp(64, MAX_FIELD_CELLS);
    let cell = side / f64::from(cells);
    let samples: Vec<Sample> = (0..cells * cells)
        .map(|i| {
            environment.sample(point(
                (f64::from(i % cells) + 0.5) * cell,
                (f64::from(i / cells) + 0.5) * cell,
            ))
        })
        .collect();

    for field in &FIELDS {
        let values: Vec<f64> = samples.iter().map(field.value).collect();
        let low = values.iter().copied().fold(f64::MAX, f64::min);
        let high = values.iter().copied().fold(f64::MIN, f64::max);
        let span = (high - low).max(1e-9);
        let note = format!("de {low:.2} a {high:.2} {}", field.unit);
        atlas.blank(field.id, field.title, &note);
        paint_cells(atlas, field.id, cells, |x, y| {
            ramp(field.stops, (values[(y * cells + x) as usize] - low) / span)
        });
    }

    let note = SOIL_COLORS
        .iter()
        .map(|(_, color, name)| format!("{name} (rgb {},{},{})", color[0], color[1], color[2]))
        .collect::<Vec<_>>()
        .join(", ");
    atlas.blank("soil", "Tipo de solo", &note);
    paint_cells(atlas, "soil", cells, |x, y| {
        let soil = samples[(y * cells + x) as usize].soil;
        SOIL_COLORS
            .iter()
            .find(|(kind, _, _)| *kind == soil)
            .expect("every soil has a color")
            .1
    });

    let density = (0..cells * cells)
        .map(|i| {
            city.context.density.at(point(
                (f64::from(i % cells) + 0.5) * cell,
                (f64::from(i / cells) + 0.5) * cell,
            ))
        })
        .collect::<Vec<_>>();
    atlas.blank("density", "Densidade urbana", "0 a 1");
    paint_cells(atlas, "density", cells, |x, y| {
        ramp(
            &[[245, 243, 235], [230, 170, 120], [140, 30, 60]],
            density[(y * cells + x) as usize],
        )
    });

    atlas.blank(
        "composite",
        "Cidade vista de cima",
        "todas as camadas de superfície",
    );
    let hillshade: Vec<f64> = samples.iter().map(|s| s.elevation).collect();
    let low = hillshade.iter().copied().fold(f64::MAX, f64::min);
    let span = (hillshade.iter().copied().fold(f64::MIN, f64::max) - low).max(1.0);
    paint_cells(atlas, "composite", cells, |x, y| {
        ramp(
            &[[196, 204, 176], [214, 208, 176], [226, 222, 208]],
            (hillshade[(y * cells + x) as usize] - low) / span,
        )
    });
}

fn edge_lines<'a>(
    edges: &'a [StreetEdge],
    nodes: &'a [StreetNode],
    keep: impl Fn(&StreetEdge) -> bool + 'a,
) -> impl Iterator<Item = (Point, Point)> + 'a {
    edges
        .iter()
        .filter(move |edge| keep(edge))
        .map(|edge| (nodes[edge.a].pos, nodes[edge.b].pos))
}

fn paint_streets(atlas: &mut Atlas, edges: &[StreetEdge], nodes: &[StreetNode], local_px: f32) {
    let scale = atlas.scale;
    let arterial_path = atlas.lines(edge_lines(edges, nodes, |e| {
        e.class == StreetClass::Arterial && !e.bridge
    }));
    let local_path = atlas.lines(edge_lines(edges, nodes, |e| {
        e.class == StreetClass::Local && !e.bridge
    }));
    let bridge_path = atlas.lines(edge_lines(edges, nodes, |e| e.bridge));
    for sheet in ["streets", "composite"] {
        atlas.stroke(
            sheet,
            local_path.as_ref(),
            (12.0 * scale).max(local_px),
            [132, 128, 122],
        );
        atlas.stroke(
            sheet,
            arterial_path.as_ref(),
            (24.0 * scale).max(1.0),
            [84, 80, 76],
        );
        atlas.stroke(
            sheet,
            bridge_path.as_ref(),
            (30.0 * scale).max(1.4),
            [190, 60, 40],
        );
    }
}

fn paint_network(atlas: &mut Atlas, network: &Network, color: Rgb, width: f32) {
    let lines = atlas.lines(network.segments.iter().map(|s| (s.pa, s.pb)));
    atlas.stroke(&network.id, lines.as_ref(), width, color);
}

fn hex_color(text: &str) -> Rgb {
    let digits = text.trim_start_matches('#');
    std::array::from_fn(|i| u8::from_str_radix(&digits[i * 2..i * 2 + 2], 16).unwrap_or(0))
}

fn paint_district(atlas: &mut Atlas, city: &City, id: usize) {
    let district = generate_district(city, id).expect("planned districts generate");
    let mut by_use: BTreeMap<u8, (LandUse, Vec<&[Point]>)> = BTreeMap::new();
    for lot in &district.lots {
        by_use
            .entry(lot.land_use as u8)
            .or_insert((lot.land_use, Vec::new()))
            .1
            .push(&lot.polygon);
    }
    for (land_use, polygons) in by_use.values() {
        let path = atlas.polygons(polygons.iter().copied());
        for sheet in ["land_use", "composite"] {
            atlas.fill(sheet, path.clone(), land_color(*land_use));
        }
    }

    paint_streets(atlas, &district.street_edges, &district.street_nodes, 0.5);

    let mut by_look: BTreeMap<(u8, u32), Vec<&[Point]>> = BTreeMap::new();
    for building in &district.buildings {
        by_look
            .entry((building.land_use as u8, building.floors.min(40)))
            .or_default()
            .push(&building.footprint);
    }
    for ((land_use, floors), polygons) in &by_look {
        let path = atlas.polygons(polygons.iter().copied());
        let land = district
            .buildings
            .iter()
            .find(|b| b.land_use as u8 == *land_use)
            .expect("group has a building")
            .land_use;
        for sheet in ["buildings", "composite"] {
            atlas.fill(sheet, path.clone(), building_color(land, *floors));
        }
    }

    for (network, spec) in district.networks.iter().zip(&district.specs) {
        paint_network(atlas, network, hex_color(&spec.color), 0.7);
    }
}

fn paint_corridors(atlas: &mut Atlas, city: &City) {
    let scale = atlas.scale;
    let polylines = |lines: &[Vec<Point>]| -> Vec<(Point, Point)> {
        lines
            .iter()
            .flat_map(|line| line.windows(2).map(|w| (w[0], w[1])))
            .collect()
    };
    let rivers = atlas.lines(polylines(&city.context.rivers).into_iter());
    let rails = atlas.lines(polylines(&city.context.rails).into_iter());
    for sheet in ["rivers", "composite"] {
        atlas.stroke(
            sheet,
            rivers.as_ref(),
            (RIVER_WIDTH * scale).max(2.0),
            WATER,
        );
    }
    atlas.stroke(
        "composite",
        rails.as_ref(),
        (30.0 * scale).max(1.0),
        [50, 46, 44],
    );
}

fn write_gallery(atlas: &Atlas, out: &FsPath, city: &City, code: &str) -> std::io::Result<()> {
    let order = [
        "composite",
        "land_use",
        "buildings",
        "streets",
        "rivers",
        "density",
    ];
    let mut ids: Vec<&str> = order
        .iter()
        .copied()
        .filter(|id| atlas.sheets.contains_key(*id))
        .collect();
    ids.extend(
        atlas
            .sheets
            .keys()
            .map(String::as_str)
            .filter(|id| !order.contains(id)),
    );
    let cards: String = ids
        .iter()
        .map(|id| {
            let sheet = &atlas.sheets[*id];
            format!(
                "<figure><a href=\"{id}.png\"><img src=\"{id}.png\" alt=\"{}\" loading=\"lazy\"></a><figcaption><b>{}</b><br>{}</figcaption></figure>",
                sheet.title, sheet.title, sheet.note
            )
        })
        .collect();
    let page = format!(
        "<!doctype html><html lang=\"pt-BR\"><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>{code}</title><style>body{{margin:0;padding:20px;font:14px system-ui;background:#f4f1ea;color:#222}}main{{display:grid;grid-template-columns:repeat(auto-fill,minmax(300px,1fr));gap:18px}}figure{{margin:0}}img{{width:100%;display:block;border:1px solid #ccc}}figcaption{{margin-top:6px}}</style><h1>{code}</h1><p>{} habitantes, {} distritos.</p><main>{cards}</main></html>",
        city.population,
        city.districts.len()
    );
    fs::write(out.join("index.html"), page)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_args();
    let seed: u64 = number(&args, "seed", 42);
    let population: u64 = number(&args, "population", 400_000);
    let size: u32 = number(&args, "size", 2048);
    let region_id = args.get("region").map_or("BR-SE", String::as_str);
    let out = FsPath::new(args.get("out").map_or("maps", String::as_str));
    let macro_only = args.contains_key("macro-only");

    let region = regions::find(region_id).ok_or(format!("unknown region {region_id}"))?;
    let mut profile = region.profile;
    if let Some(shape) = args.get("shape") {
        profile.block_shape = shape.parse()?;
    }
    let city = generate_city(&CityInput {
        seed,
        population,
        region: Some(region_id.into()),
        profile,
        extra_networks: Vec::new(),
    })
    .map_err(|error| error.to_string())?;

    let side = city.boundary[2].x;
    let mut atlas = Atlas {
        sheets: BTreeMap::new(),
        scale: size as f32 / side as f32,
        size,
    };
    paint_environment(&mut atlas, &city, side);
    atlas.blank(
        "land_use",
        "Uso do solo",
        "residencial, comercial, industrial, parque, mata, várzea, rio, ferrovia",
    );
    atlas.blank("buildings", "Prédios", "mais escuro é mais alto");
    atlas.blank("streets", "Ruas e pontes", "pontes em vermelho");
    atlas.blank("rivers", "Rios", "");
    for spec in &city.trunk_specs {
        atlas.blank(&spec.id, &spec.label, "tronco e rede local");
    }

    if !macro_only {
        for plan in &city.districts {
            paint_district(&mut atlas, &city, plan.id);
        }
    }
    paint_streets(&mut atlas, &city.edges, &city.nodes, 1.0);
    paint_corridors(&mut atlas, &city);
    for (network, spec) in city.networks.iter().zip(&city.trunk_specs) {
        paint_network(&mut atlas, network, hex_color(&spec.color), 2.0);
    }

    fs::create_dir_all(out)?;
    for (id, sheet) in &atlas.sheets {
        sheet.pixmap.save_png(out.join(format!("{id}.png")))?;
    }
    let code = format!("{region_id}:{seed}");
    write_gallery(&atlas, out, &city, &code)?;
    println!("{} camadas em {}", atlas.sheets.len(), out.display());
    Ok(())
}
