<p align="center">
  <picture><source media="(prefers-color-scheme: dark)" srcset="https://shieldcn.dev/header/graph.svg?title=City+Seed&subtitle=Cidades+procedurais+por+seed%2C+de+bairros+a+megal%C3%B3poles&align=left&mode=dark" /><img alt="City Seed" src="https://shieldcn.dev/header/graph.svg?title=City+Seed&subtitle=Cidades+procedurais+por+seed%2C+de+bairros+a+megal%C3%B3poles&align=left&mode=light" /></picture>
</p>

<p align="center"><b>Português (BR)</b> · <a href="README.md">English</a></p>

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

# 📖 Sobre

City Seed é um gerador procedural e determinístico de cidades escrito em Rust. A partir de uma seed, de uma população (de um bairro pequeno até cerca de 40 milhões de pessoas) e de um perfil regional, ele sempre produz a mesma cidade.

- **Malha urbana:** ruas com quarteirões que vão de retângulos a hexágonos, lotes, prédios, parques, rios e terreno.
- **Infraestrutura:** energia (aérea ou subterrânea), água, gás, esgoto, drenagem, metrô e trem. As redes são definidas por dados, então novas redes podem ser adicionadas sem mudar o gerador.
- **Ambiente natural:** temperatura, chuva, profundidade da rocha, tipo de solo, risco sísmico e relevo mudam o que é construído e onde.
- **Simulação de falhas:** regras simples propagam os danos, como falta de água e de luz, risco elétrico e explosões de gás.
- **Detalhe sob demanda:** a cidade inteira é planejada em nível macro, e cada distrito só é gerado quando necessário.

O núcleo não depende de nenhum framework. O `city-ffi` o expõe por uma ABI C que troca JSON, e a mesma biblioteca é compilada para WebAssembly no visualizador web.

| Crate ou pasta | Finalidade |
| :--- | :--- |
| `crates/city-core` | Geração e simulação de falhas |
| `crates/city-ffi` | ABI C e integração com WebAssembly |
| `crates/city-tools` | Exportação de mapas (`cargo maps`) e testes de desempenho (`cargo perf`) |
| `viewer/` | Visualizador web em JavaScript puro e canvas |

# 📋 Motivo

Foi criado esse repositório para aprender a criação de seeds para criação de hipercentros urbanos.

# 💻 Como iniciar

### Requisitos

- [Rust](https://www.rust-lang.org/tools/install) (edition 2024)
- [Python 3](https://www.python.org/downloads/) (apenas para o visualizador web)

### Instalação

1. Clone o repositório:

   ```sh
   git clone https://github.com/BGLuis/city-seed.git
   ```

2. Entre no diretório do projeto:

   ```sh
   cd city-seed
   ```

3. Compile o workspace e rode os testes:

   ```sh
   cargo build --release
   cargo test --workspace
   ```

4. Exporte a vista de cima e uma imagem por camada para `maps/` (abra `maps/index.html`):

   ```sh
   cargo maps --seed 42 --population 400000 --region BR-SE
   ```

5. Rode os testes de desempenho, que terminam com erro quando um limite é ultrapassado e gravam `target/perf/report.json`:

   ```sh
   cargo perf
   ```

6. Compile e sirva o visualizador web, depois abra <http://localhost:8765>:

   ```sh
   rustup target add wasm32-unknown-unknown
   ./scripts/build-viewer.sh
   python3 -m http.server 8765 --directory viewer
   ```

# 🤝 Contribuidores

<a href="https://github.com/BGLuis/city-seed/graphs/contributors">
  <picture><source media="(prefers-color-scheme: dark)" srcset="https://shieldcn.dev/contributors/BGLuis/city-seed.svg?title=false&preset=transparent&border=false&mode=dark" /><img alt="Contribuidores" src="https://shieldcn.dev/contributors/BGLuis/city-seed.svg?title=false&preset=transparent&border=false&mode=light" /></picture>
</a>

# 📄 Licença

Distribuído sob a licença [MIT](LICENSE).
