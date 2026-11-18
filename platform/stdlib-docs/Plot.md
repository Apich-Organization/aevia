# `Plot`

Source: `stdlib/Plot.aev`

The `Plot` module of §8.20.

## `let combine`

```aevia
let combine = combine
```

**Signature:** `combine : Tensor<1, Series> -> PlotSpec`
## `let default_plot_opts`

```aevia
let default_plot_opts = default_plot_opts
```

**Signature:** `default_plot_opts : PlotOpts`
## `let errorbar`

```aevia
let errorbar = errorbar
```

**Signature:** `errorbar : (Tensor<1, Scalar<ρ,d>>, Tensor<1, Scalar<ρ,e>>, Tensor<1, Scalar<ρ,e>>) -> Series`
## `let errorbar_with`

```aevia
let errorbar_with = errorbar_with
```

**Signature:** `errorbar_with : 同上 + PlotOpts`
## `let heatmap`

```aevia
let heatmap = heatmap
```

**Signature:** `heatmap : (Tensor<2, Scalar<ρ,d>>) -> Series`
## `let heatmap_with`

```aevia
let heatmap_with = heatmap_with
```

**Signature:** `heatmap_with : (Tensor<2, Scalar<ρ,d>>, PlotOpts) -> Series`
## `let hist`

```aevia
let hist = hist
```

**Signature:** `hist : (Tensor<1, Scalar<ρ,d>>, Int) -> Series`
## `let hist_with`

```aevia
let hist_with = hist_with
```

**Signature:** `hist_with : (Tensor<1, Scalar<ρ,d>>, Int, PlotOpts) -> Series`
## `let label`

```aevia
let label = label
```

**Signature:** `label : (Series, String) -> Series`
## `let line`

```aevia
let line = line
```

**Signature:** `line : (Tensor<1, Scalar<ρ,d>>, Tensor<1, Scalar<ρ,e>>) -> Series`
## `let line_with`

```aevia
let line_with = line_with
```

**Signature:** `line_with : (Tensor<1, Scalar<ρ,d>>, Tensor<1, Scalar<ρ,e>>, PlotOpts) -> Series`
## `let plot`

```aevia
let plot = plot
```

**Signature:** `plot : PlotSpec -> IO Unit`
## `let save`

```aevia
let save = save
```

**Signature:** `save : (String, PlotSpec) -> IO Unit`
## `let scatter`

```aevia
let scatter = scatter
```

**Signature:** `scatter : 同上（默认参数）`
## `let scatter_with`

```aevia
let scatter_with = scatter_with
```

**Signature:** `scatter_with : 同上（带 PlotOpts）`
