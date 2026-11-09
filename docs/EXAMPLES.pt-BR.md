# Exemplos de Cidades

<p align="center"><a href="EXAMPLES.md">English</a> · <b>Português (BR)</b></p>

[← Voltar ao README](../README.pt-BR.md)

Todas as imagens desta página foram exportadas com `cargo maps`. Rode o comando mostrado em cada cidade para gerá-la de novo. O resultado vai para `maps/`: `composite.png` é a vista de cima, e há uma imagem por camada. Abra `maps/index.html` para ver todas.

> As imagens daqui foram reduzidas a 64 cores para manter o repositório leve. Os arquivos que você gerar têm todas as cores. Conforme o gerador evolui, a mesma seed pode produzir outra cidade.

## Opções

| Opção | Significado | Padrão |
| :--- | :--- | :--- |
| `--seed` | Número que determina a cidade | `42` |
| `--population` | População, de 1 mil a 40 milhões | `400000` |
| `--region` | Perfil regional (veja a tabela abaixo) | `BR-SE` |
| `--shape` | Substitui a forma de quarteirão do perfil, de `0` (retângulos) a `1` (hexágonos) | a do perfil |
| `--size` | Largura e altura da imagem em pixels | `2048` |
| `--out` | Pasta de saída | `maps` |
| `--macro-only` | Desenha só o plano macro, sem gerar os distritos | desligado |

## Como ler a vista de cima

| Cor | Significado |
| :--- | :--- |
| Bege | Residencial |
| Lilás e roxo | Comercial |
| Marrom | Industrial |
| Verde-claro | Parque |
| Verde-escuro | Mata |
| Verde-água | Várzea |
| Azul | Rio |
| Linhas vermelhas | Pontes |

## Cidades

### Bairro pequeno

5 mil pessoas. Quarteirões europeus irregulares, um rio e um pequeno núcleo comercial.

```sh
cargo maps --seed 7 --population 5000 --region EU-W --size 1024
```

<img src="examples/neighborhood.png" alt="Bairro pequeno" width="640" />

### Cidade hexagonal

30 mil pessoas. Quarteirões hexagonais regulares do perfil fictício, com mata em uma das bordas.

```sh
cargo maps --seed 11 --population 30000 --region XX-HEX --size 1024
```

<img src="examples/hex-town.png" alt="Cidade hexagonal" width="640" />

### Cidade grande brasileira

400 mil pessoas. Um afluente encontra o rio principal, várzeas acompanham os vales e a mata cerca a área urbana.

```sh
cargo maps --seed 42 --population 400000 --region BR-SE --size 1024
```

<img src="examples/br-city.png" alt="Cidade grande brasileira" width="640" />

### Metrópole europeia

2 milhões de pessoas. Avenidas orgânicas, um rio com muitas pontes e linhas de trem chegando ao centro.

```sh
cargo maps --seed 21 --population 2000000 --region EU-W --size 1024
```

<img src="examples/eu-metro.png" alt="Metrópole europeia" width="640" />

### Metrópole norte-americana

5 milhões de pessoas. Uma malha retangular quase perfeita.

```sh
cargo maps --seed 5 --population 5000000 --region US-NE --size 1024
```

<img src="examples/us-metro.png" alt="Metrópole norte-americana" width="640" />

### Hipermetrópole

40 milhões de pessoas, o maior tamanho aceito. Vários centros ligados por uma rede densa de trens.

```sh
cargo maps --seed 42 --population 40000000 --region JP-KT --size 1024
```

<img src="examples/jp-hyper.png" alt="Hipermetrópole" width="640" />

## De retângulos a hexágonos

A mesma cidade, mudando só o `--shape`. Estas imagens são a camada `streets.png`.

| `--shape 0`: Retângulos | `--shape 0.5`: Intermediário | `--shape 1`: Hexágonos |
| :---: | :---: | :---: |
| <img src="examples/shape-0-streets.png" alt="Retângulos" /> | <img src="examples/shape-05-streets.png" alt="Intermediário" /> | <img src="examples/shape-1-streets.png" alt="Hexágonos" /> |

```sh
cargo maps --seed 11 --population 30000 --region XX-HEX --shape 0.5 --size 1024
```

## Uma imagem por camada

Cada exportação também grava cada camada separadamente. Estas vêm da cidade brasileira e da hipermetrópole acima.

| Tipo de solo (`soil.png`) | Risco de inundação (`flood_risk.png`) |
| :---: | :---: |
| <img src="examples/br-city-soil.png" alt="Tipo de solo" /> | <img src="examples/br-city-flood_risk.png" alt="Risco de inundação" /> |

| Energia (`power.png`) | Esgoto (`sewer.png`) |
| :---: | :---: |
| <img src="examples/br-city-power.png" alt="Energia" /> | <img src="examples/br-city-sewer.png" alt="Esgoto" /> |

| Terreno (`terrain.png`) | Metrô (`metro.png`) |
| :---: | :---: |
| <img src="examples/br-city-terrain.png" alt="Terreno" /> | <img src="examples/jp-hyper-metro.png" alt="Metrô" /> |

## Perfis regionais

O `--region` escolhe um destes conjuntos de valores, definidos em `crates/city-core/src/regions.rs`.

| Parâmetro | `BR-SE` | `US-NE` | `EU-W` | `JP-KT` | `XX-HEX` |
| :--- | ---: | ---: | ---: | ---: | ---: |
| Parcela de energia aérea | 0.8 | 0.5 | 0.05 | 0.85 | 0.3 |
| Gás encanado | false | true | true | true | true |
| Parcela de parques | 0.05 | 0.08 | 0.12 | 0.05 | 0.1 |
| Forma do quarteirão (0 retângulo, 1 hexágono) | 0 | 0 | 0.35 | 0.2 | 1 |
| Irregularidade das ruas | 0.45 | 0.03 | 0.7 | 0.55 | 0 |
| Rios | 2 | 1 | 1 | 2 | 1 |
| Densidade do metrô | 0.6 | 0.5 | 1 | 1.4 | 1 |
| Densidade dos trens | 0.6 | 0.8 | 1 | 1.6 | 1 |
| Temperatura média (°C) | 21 | 11 | 11 | 15 | 18 |
| Chuva anual (mm) | 1500 | 1200 | 800 | 1500 | 1000 |
| Profundidade da rocha (m) | 12 | 10 | 20 | 25 | 15 |
| Risco sísmico (0 a 1) | 0.05 | 0.1 | 0.05 | 0.9 | 0 |
| Relevo | 1.2 | 0.6 | 0.6 | 1 | 0.5 |
