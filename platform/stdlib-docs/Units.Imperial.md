# `Units.Imperial`

Source: `stdlib/Units/Imperial.aev`

Use imperial and US customary units in quantities such as `6 ft`. `gal` is the US liquid gallon.

英制与美制单位可以直接用于物理量，如 `6 ft`。`gal` 采用美制液量加仑。

The coherent SI value is the magnitude times the scale, plus the offset.

SI 相干单位下的值等于读数乘比例，再加偏置。

| Unit 单位 | Dimension 量纲 | Scale 比例 | Offset 偏置 | Prefix 可加前缀 | Aliases 别名 |
|---|---|---|---|---|---|
| `acre` | `Length^2` | `316160658/78125` | `0` | — |  |
| `degF` | `Temperature` | `5/9` | `45967/180` | — | `fahrenheit` |
| `ft` | `Length` | `381/1250` | `0` | — | `foot` |
| `gal` | `Length^3` | `473176473/125000000000` | `0` | — |  |
| `hp` | `Length^2*Mass/Time^3` | `37284993579113511/50000000000000` | `0` | — |  |
| `in` | `Length` | `127/5000` | `0` | — | `inch` |
| `lb` | `Mass` | `45359237/100000000` | `0` | — |  |
| `lbf` | `Length*Mass/Time^2` | `8896443230521/2000000000000` | `0` | — |  |
| `mi` | `Length` | `201168/125` | `0` | — |  |
| `mph` | `Length/Time` | `1397/3125` | `0` | — |  |
| `oz` | `Mass` | `45359237/1600000000` | `0` | — |  |
| `psi` | `Mass/(Length*Time^2)` | `430922330823/62500000` | `0` | — |  |
| `yd` | `Length` | `1143/1250` | `0` | — |  |

This module declares nothing.
