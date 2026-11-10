# Example Cities

<p align="center"><b>English</b> · <a href="EXAMPLES.pt-BR.md">Português (BR)</a></p>

[← Back to the README](../README.md)

The maps on this page were exported with `cargo maps` and the 3D images were captured from the browser viewer. Run the command shown with a city to generate it again. The result goes to `maps/`: `composite.png` is the top view and there is one image per layer. Open `maps/index.html` to browse them all.

> The maps here were reduced to 64 colors to keep the repository small. The files you generate have full color. As the generator evolves, the same seed may produce a different city.

## Options

| Option | Meaning | Default |
| :--- | :--- | :--- |
| `--seed` | Number that determines the city | `42` |
| `--population` | Population, from 1 thousand to 40 million | `400000` |
| `--region` | Regional preset (see the table below) | `BR-SE` |
| `--shape` | Overrides the preset's block shape, from `0` (rectangles) to `1` (hexagons) | from the preset |
| `--size` | Image width and height in pixels | `2048` |
| `--out` | Output folder | `maps` |
| `--macro-only` | Draws only the macro plan, without generating the districts | off |

## Reading the top view

| Color | Meaning |
| :--- | :--- |
| Beige | Residential |
| Lilac and purple | Commercial |
| Brown | Industrial |
| Light green | Park |
| Dark green | Forest |
| Teal | Wetland |
| Blue | River |
| Red lines | Bridges |

## Cities

### Small neighborhood

5 thousand people. Irregular European blocks, one river and a small commercial core.

```sh
cargo maps --seed 7 --population 5000 --region EU-W --size 1024
```

<img src="examples/neighborhood.png" alt="Small neighborhood" width="640" />

**3D view:**

```text
http://localhost:8765/3d.html?seed=7&population=5000&region=EU-W&distance=1300&pitch=0.6
```

<img src="examples/3d-neighborhood.jpg" alt="3D view" width="640" />

### Hexagonal town

30 thousand people. Regular hexagonal blocks from the fictional preset, with woodland on one edge.

```sh
cargo maps --seed 11 --population 30000 --region XX-HEX --size 1024
```

<img src="examples/hex-town.png" alt="Hexagonal town" width="640" />

**3D view:**

```text
http://localhost:8765/3d.html?seed=11&population=30000&region=XX-HEX&distance=2200&pitch=0.6
```

<img src="examples/3d-hex-town.jpg" alt="3D view" width="640" />

### Large Brazilian city

400 thousand people. A tributary joins the main river, wetlands follow the valleys and forest surrounds the urban area.

```sh
cargo maps --seed 42 --population 400000 --region BR-SE --size 1024
```

<img src="examples/br-city.png" alt="Large Brazilian city" width="640" />

**3D view:**

```text
http://localhost:8765/3d.html?seed=42&population=400000&region=BR-SE
```

<img src="examples/3d-br-city.jpg" alt="3D view" width="640" />

### European metropolis

2 million people. Organic avenues, a river with many bridges and rail lines reaching the center.

```sh
cargo maps --seed 21 --population 2000000 --region EU-W --size 1024
```

<img src="examples/eu-metro.png" alt="European metropolis" width="640" />

**3D view:**

```text
http://localhost:8765/3d.html?seed=21&population=2000000&region=EU-W
```

<img src="examples/3d-eu-metro.jpg" alt="3D view" width="640" />

### North American metropolis

5 million people. An almost perfect rectangular grid.

```sh
cargo maps --seed 5 --population 5000000 --region US-NE --size 1024
```

<img src="examples/us-metro.png" alt="North American metropolis" width="640" />

**3D view:**

```text
http://localhost:8765/3d.html?seed=5&population=5000000&region=US-NE
```

<img src="examples/3d-us-metro.jpg" alt="3D view" width="640" />

### Hypermetropolis

40 million people, the largest supported size. Several centers connected by a dense rail network.

```sh
cargo maps --seed 42 --population 40000000 --region JP-KT --size 1024
```

<img src="examples/jp-hyper.png" alt="Hypermetropolis" width="640" />

**3D view:**

```text
http://localhost:8765/3d.html?seed=42&population=40000000&region=JP-KT&distance=1500&pitch=0.4
```

<img src="examples/3d-jp-hyper.jpg" alt="3D view" width="640" />

## From rectangles to hexagons

The same city with only `--shape` changed. These images are the `streets.png` layer.

| `--shape 0`: Rectangles | `--shape 0.5`: In between | `--shape 1`: Hexagons |
| :---: | :---: | :---: |
| <img src="examples/shape-0-streets.png" alt="Rectangles" /> | <img src="examples/shape-05-streets.png" alt="In between" /> | <img src="examples/shape-1-streets.png" alt="Hexagons" /> |

```sh
cargo maps --seed 11 --population 30000 --region XX-HEX --shape 0.5 --size 1024
```

## One image per layer

Each export also writes every layer separately. These come from the Brazilian city and from the hypermetropolis above.

| Soil type (`soil.png`) | Flood risk (`flood_risk.png`) |
| :---: | :---: |
| <img src="examples/br-city-soil.png" alt="Soil type" /> | <img src="examples/br-city-flood_risk.png" alt="Flood risk" /> |

| Power (`power.png`) | Sewage (`sewer.png`) |
| :---: | :---: |
| <img src="examples/br-city-power.png" alt="Power" /> | <img src="examples/br-city-sewer.png" alt="Sewage" /> |

| Terrain (`terrain.png`) | Metro (`metro.png`) |
| :---: | :---: |
| <img src="examples/br-city-terrain.png" alt="Terrain" /> | <img src="examples/jp-hyper-metro.png" alt="Metro" /> |

## The 3D view

Each address on this page opens that exact city and camera. Start the viewer first:

```sh
./scripts/build-viewer.sh
python3 -m http.server 8765 --directory viewer
```

| Address parameter | Meaning | Default |
| :--- | :--- | :--- |
| `seed` | Number that determines the city | `42` |
| `population` | Population | `5000000` |
| `region` | Regional preset | `JP-KT` |
| `distance` | Camera distance in meters | `2500` |
| `pitch` | Camera tilt in radians | `0.55` |
| `yaw` | Camera direction in radians | `-1` |
| `view` | `underground` opens the underground view | surface |

### Street level

Buildings, parks and overhead power lines up close.

```text
http://localhost:8765/3d.html?seed=42&population=5000000&region=JP-KT&distance=500&pitch=0.32
```

<img src="examples/3d-jp-metro-street.jpg" alt="Street level" width="640" />

### Underground

With the surface hidden, each network shows in its own color. Press **Subsolo** in the panel to get this view.

```text
http://localhost:8765/3d.html?seed=42&population=40000000&region=JP-KT&view=underground
```

<img src="examples/3d-jp-hyper-underground.jpg" alt="Underground" width="640" />

### Whole city

From far away the city uses its low-detail version: one tile per district, colored by density.

```text
http://localhost:8765/3d.html?seed=42&population=40000000&region=JP-KT&distance=138000&pitch=0.9
```

<img src="examples/3d-jp-hyper-overview.jpg" alt="Whole city" width="640" />

## Regional presets

`--region` selects one of these sets of values, defined in `crates/city-core/src/regions.rs`.

| Parameter | `BR-SE` | `US-NE` | `EU-W` | `JP-KT` | `XX-HEX` |
| :--- | ---: | ---: | ---: | ---: | ---: |
| Share of overhead power | 0.8 | 0.5 | 0.05 | 0.85 | 0.3 |
| Piped gas | false | true | true | true | true |
| Share of parks | 0.05 | 0.08 | 0.12 | 0.05 | 0.1 |
| Block shape (0 rectangle, 1 hexagon) | 0 | 0 | 0.35 | 0.2 | 1 |
| Street irregularity | 0.45 | 0.03 | 0.7 | 0.55 | 0 |
| Rivers | 2 | 1 | 1 | 2 | 1 |
| Metro density | 0.6 | 0.5 | 1 | 1.4 | 1 |
| Rail density | 0.6 | 0.8 | 1 | 1.6 | 1 |
| Mean temperature (°C) | 21 | 11 | 11 | 15 | 18 |
| Annual rainfall (mm) | 1500 | 1200 | 800 | 1500 | 1000 |
| Bedrock depth (m) | 12 | 10 | 20 | 25 | 15 |
| Seismic risk (0 to 1) | 0.05 | 0.1 | 0.05 | 0.9 | 0 |
| Relief | 1.2 | 0.6 | 0.6 | 1 | 0.5 |
