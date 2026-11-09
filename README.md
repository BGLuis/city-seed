<p align="center">
  <picture><source media="(prefers-color-scheme: dark)" srcset="https://shieldcn.dev/header/graph.svg?title=City+Seed&subtitle=Seeded+procedural+cities%2C+from+neighborhoods+to+megacities&align=left&mode=dark" /><img alt="City Seed" src="https://shieldcn.dev/header/graph.svg?title=City+Seed&subtitle=Seeded+procedural+cities%2C+from+neighborhoods+to+megacities&align=left&mode=light" /></picture>
</p>

<p align="center"><b>English</b> · <a href="README.pt-BR.md">Português (BR)</a></p>

<p align="center">
  <picture><source media="(prefers-color-scheme: dark)" srcset="https://shieldcn.dev/github/stars/BGLuis/city-seed.svg?variant=secondary&size=sm&mode=dark" /><img alt="GitHub Stars" src="https://shieldcn.dev/github/stars/BGLuis/city-seed.svg?variant=secondary&size=sm&mode=light" /></picture>
  <picture><source media="(prefers-color-scheme: dark)" srcset="https://shieldcn.dev/github/forks/BGLuis/city-seed.svg?variant=secondary&size=sm&mode=dark" /><img alt="GitHub Forks" src="https://shieldcn.dev/github/forks/BGLuis/city-seed.svg?variant=secondary&size=sm&mode=light" /></picture>
  <picture><source media="(prefers-color-scheme: dark)" srcset="https://shieldcn.dev/github/watchers/BGLuis/city-seed.svg?variant=secondary&size=sm&mode=dark" /><img alt="Watchers" src="https://shieldcn.dev/github/watchers/BGLuis/city-seed.svg?variant=secondary&size=sm&mode=light" /></picture>
  <picture><source media="(prefers-color-scheme: dark)" srcset="https://shieldcn.dev/github/contributors/BGLuis/city-seed.svg?theme=emerald&size=sm&mode=dark" /><img alt="Contributors" src="https://shieldcn.dev/github/contributors/BGLuis/city-seed.svg?theme=emerald&size=sm&mode=light" /></picture>
  <picture><source media="(prefers-color-scheme: dark)" srcset="https://shieldcn.dev/github/license/BGLuis/city-seed.svg?variant=ghost&size=sm&mode=dark" /><img alt="License" src="https://shieldcn.dev/github/license/BGLuis/city-seed.svg?variant=ghost&size=sm&mode=light" /></picture>
</p>

<p align="center">
  <img alt="Rust" src="https://shieldcn.dev/badge/Language-Rust-000000.svg?logo=rust&variant=branded&size=sm" />
  <img alt="WebAssembly" src="https://shieldcn.dev/badge/Runtime-WebAssembly-654FF0.svg?logo=webassembly&variant=branded&size=sm" />
  <img alt="JavaScript" src="https://shieldcn.dev/badge/Language-JavaScript-F7DF1E.svg?logo=javascript&variant=branded&size=sm" />
</p>

<p align="center"><a href="docs/EXAMPLES.md"><b>🏙️ See example cities</b></a></p>

# 📖 About

City Seed is a deterministic procedural city generator written in Rust. Given a seed, a population (from a small neighborhood up to about 40 million people) and a regional profile, it always produces the same city.

- **Urban fabric:** streets with blocks that range from rectangles to hexagons, lots, buildings, parks, rivers and terrain.
- **Infrastructure:** power (overhead or underground), water, gas, sewage, drainage, metro and rail. Networks are defined by data, so new ones can be added without changing the generator.
- **Natural environment:** temperature, rainfall, bedrock depth, soil type, seismic risk and relief change what gets built and where.
- **Failure simulation:** simple rules propagate damage, such as water and power outages, electrical risk and gas explosions.
- **On-demand detail:** the whole city is planned at a macro level, and each district is generated only when needed.

The core has no framework dependency. `city-ffi` exposes it through a C ABI that exchanges JSON, and the same library is compiled to WebAssembly for the browser viewer.

| Crate or folder | Purpose |
| :--- | :--- |
| `crates/city-core` | Generation and failure simulation |
| `crates/city-ffi` | C ABI and WebAssembly bindings |
| `crates/city-tools` | Map export (`cargo maps`) and performance suite (`cargo perf`) |
| `viewer/` | Browser viewer in plain JavaScript and canvas |

# 📋 Motivation

This repository was created to learn how to create seeds for generating urban hyper-centers.

# 💻 Getting Started

### Requirements

- [Rust](https://www.rust-lang.org/tools/install) (edition 2024)
- [Python 3](https://www.python.org/downloads/) (browser viewer only)

### Installation

1. Clone the repository:

   ```sh
   git clone https://github.com/BGLuis/city-seed.git
   ```

2. Enter the project directory:

   ```sh
   cd city-seed
   ```

3. Build the workspace and run the tests:

   ```sh
   cargo build --release
   cargo test --workspace
   ```

4. Export a top view and one image per layer to `maps/` (open `maps/index.html`):

   ```sh
   cargo maps --seed 42 --population 400000 --region BR-SE
   ```

5. Run the performance suite, which exits with an error when a budget is exceeded and writes `target/perf/report.json`:

   ```sh
   cargo perf
   ```

6. Build and serve the browser viewer, then open <http://localhost:8765>:

   ```sh
   rustup target add wasm32-unknown-unknown
   ./scripts/build-viewer.sh
   python3 -m http.server 8765 --directory viewer
   ```

# 🤝 Contributors

<a href="https://github.com/BGLuis/city-seed/graphs/contributors">
  <picture><source media="(prefers-color-scheme: dark)" srcset="https://shieldcn.dev/contributors/BGLuis/city-seed.svg?title=false&preset=transparent&border=false&mode=dark" /><img alt="Contributors" src="https://shieldcn.dev/contributors/BGLuis/city-seed.svg?title=false&preset=transparent&border=false&mode=light" /></picture>
</a>

# 📄 License

Distributed under the [MIT](LICENSE) license.
