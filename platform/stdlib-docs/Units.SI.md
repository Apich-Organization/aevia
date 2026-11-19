# `Units.SI`

Source: `stdlib/Units/SI.aev`

The unit registry lists SI units and accepted non-SI spellings. Use a unit after a number, as in `3 km`.

单位表包含 SI 单位及可用的非 SI 名称。把单位写在数字之后，例如 `3 km`。

The coherent SI value is the magnitude times the scale, plus the offset.

SI 相干单位下的值等于读数乘比例，再加偏置。

| Unit 单位 | Dimension 量纲 | Scale 比例 | Offset 偏置 | Prefix 可加前缀 | Aliases 别名 |
|---|---|---|---|---|---|
| `A` | `Current` | `1` | `0` | ✓ |  |
| `B` | `1` | `8` | `0` | ✓ | `byte` |
| `Ba` | `Mass/(Length*Time)` | `1/10` | `0` | — |  |
| `Bq` | `1/Time` | `1` | `0` | ✓ |  |
| `C` | `Current*Time` | `1` | `0` | ✓ |  |
| `Da` | `Mass` | `8302695333/5000000000000000000000000000000000000` | `0` | ✓ | `u` |
| `F` | `Current^2*Time^4/(Length^2*Mass)` | `1` | `0` | ✓ |  |
| `G` | `Mass/(Current*Time^2)` | `1/10000` | `0` | — |  |
| `Gal` | `Length/Time^2` | `1/100` | `0` | — |  |
| `Gy` | `Length^2/Time^2` | `1` | `0` | ✓ |  |
| `H` | `Length^2*Mass/(Current^2*Time^2)` | `1` | `0` | ✓ |  |
| `Hz` | `1/Time` | `1` | `0` | ✓ |  |
| `J` | `Length^2*Mass/Time^2` | `1` | `0` | ✓ |  |
| `K` | `Temperature` | `1` | `0` | ✓ | `kelvin` |
| `KiB` | `1` | `8192` | `0` | — |  |
| `L` | `Length^3` | `1/1000` | `0` | ✓ | `liter`, `litre` |
| `Mearth` | `Mass` | `5972200000000000000000000` | `0` | — |  |
| `Msun` | `Mass` | `1988920000000000000000000000000` | `0` | — |  |
| `Mx` | `Length^2*Mass/(Current*Time^2)` | `1/100000000` | `0` | — |  |
| `N` | `Length*Mass/Time^2` | `1` | `0` | ✓ |  |
| `Oe` | `Current/Length` | `250*pi^-1` | `0` | — |  |
| `P` | `Mass/(Length*Time)` | `1/10` | `0` | — |  |
| `Pa` | `Mass/(Length*Time^2)` | `1` | `0` | ✓ |  |
| `Rsun` | `Length` | `695700000` | `0` | — |  |
| `S` | `Current^2*Time^3/(Length^2*Mass)` | `1` | `0` | ✓ |  |
| `St` | `Length^2/Time` | `1/10000` | `0` | — |  |
| `Sv` | `Length^2/Time^2` | `1` | `0` | ✓ |  |
| `T` | `Mass/(Current*Time^2)` | `1` | `0` | ✓ |  |
| `V` | `Length^2*Mass/(Current*Time^3)` | `1` | `0` | ✓ |  |
| `W` | `Length^2*Mass/Time^3` | `1` | `0` | ✓ |  |
| `Wb` | `Length^2*Mass/(Current*Time^2)` | `1` | `0` | ✓ |  |
| `Wh` | `Length^2*Mass/Time^2` | `3600` | `0` | ✓ |  |
| `acre` | `Length^2` | `316160658/78125` | `0` | — |  |
| `angstrom` | `Length` | `1/10000000000` | `0` | — |  |
| `arcmin` | `1` | `1/10800*pi^1` | `0` | — |  |
| `arcsec` | `1` | `1/648000*pi^1` | `0` | — |  |
| `atm` | `Mass/(Length*Time^2)` | `101325` | `0` | — |  |
| `au` | `Length` | `149597870700` | `0` | — |  |
| `bar` | `Mass/(Length*Time^2)` | `100000` | `0` | ✓ |  |
| `bit` | `1` | `1` | `0` | ✓ |  |
| `cal` | `Length^2*Mass/Time^2` | `523/125` | `0` | ✓ |  |
| `cd` | `Luminosity` | `1` | `0` | ✓ |  |
| `d` | `Time` | `86400` | `0` | — | `day` |
| `deg` | `1` | `1/180*pi^1` | `0` | — |  |
| `degC` | `Temperature` | `1` | `5463/20` | — | `celsius` |
| `degF` | `Temperature` | `5/9` | `45967/180` | — | `fahrenheit` |
| `dyn` | `Length*Mass/Time^2` | `1/100000` | `0` | — |  |
| `eV` | `Length^2*Mass/Time^2` | `801088317/5000000000000000000000000000` | `0` | ✓ |  |
| `erg` | `Length^2*Mass/Time^2` | `1/10000000` | `0` | — |  |
| `ft` | `Length` | `381/1250` | `0` | — | `foot` |
| `g` | `Mass` | `1/1000` | `0` | ✓ | `gram` |
| `gal` | `Length^3` | `473176473/125000000000` | `0` | — |  |
| `h` | `Time` | `3600` | `0` | — | `hour` |
| `hp` | `Length^2*Mass/Time^3` | `37284993579113511/50000000000000` | `0` | — |  |
| `in` | `Length` | `127/5000` | `0` | — | `inch` |
| `jup` | `Mass` | `1898130000000000000000000000` | `0` | — |  |
| `kat` | `Amount/Time` | `1` | `0` | ✓ |  |
| `kn` | `Length/Time` | `463/900` | `0` | — |  |
| `l` | `Length^3` | `1/1000` | `0` | ✓ |  |
| `lb` | `Mass` | `45359237/100000000` | `0` | — |  |
| `lbf` | `Length*Mass/Time^2` | `8896443230521/2000000000000` | `0` | — |  |
| `lm` | `Luminosity` | `1` | `0` | ✓ |  |
| `lx` | `Luminosity/Length^2` | `1` | `0` | ✓ |  |
| `ly` | `Length` | `9460730472580800` | `0` | — |  |
| `m` | `Length` | `1` | `0` | ✓ | `meter`, `metre` |
| `mi` | `Length` | `201168/125` | `0` | — |  |
| `min` | `Time` | `60` | `0` | — | `minute` |
| `mmHg` | `Mass/(Length*Time^2)` | `26664477483/200000000` | `0` | — |  |
| `mol` | `Amount` | `1` | `0` | ✓ |  |
| `mph` | `Length/Time` | `1397/3125` | `0` | — |  |
| `nmi` | `Length` | `1852` | `0` | — |  |
| `ohm` | `Length^2*Mass/(Current^2*Time^3)` | `1` | `0` | ✓ |  |
| `oz` | `Mass` | `45359237/1600000000` | `0` | — |  |
| `pc` | `Length` | `30856775814913673` | `0` | ✓ |  |
| `psi` | `Mass/(Length*Time^2)` | `430922330823/62500000` | `0` | — |  |
| `rad` | `1` | `1` | `0` | ✓ |  |
| `s` | `Time` | `1` | `0` | ✓ | `second` |
| `sr` | `1` | `1` | `0` | ✓ |  |
| `t` | `Mass` | `1000` | `0` | ✓ |  |
| `week` | `Time` | `604800` | `0` | — |  |
| `yd` | `Length` | `1143/1250` | `0` | — |  |
| `yr` | `Time` | `31557600` | `0` | — |  |

This module declares nothing.
