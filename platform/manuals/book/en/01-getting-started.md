# Getting started with Aevia

Aevia checks physical dimensions as part of the program's type. In this book you will start with a quantity, write a model whose types express the equations, check it against known cases, and package it for reuse. The [language specification](/spec?lang=en) defines the rules and the [standard library](/stdlib?lang=en) describes the callable functions.

## Use the toolchain

Install the Aevia toolchain from the registry's [installation page](/cli?lang=en). Put its binaries on your PATH and run `aevia --version`. The CLI and the optional registry client are separate executables. Running a local example requires only `aevia`. Package downloads and publishing also use `aevia-pkg`.

Start the REPL with `aevia`. Enter `2 m + 30 cm`, then `:type 2 m + 30 cm`. The type describes length. Enter `2 m + 30 s` and read the expected and actual dimensions in the diagnostic. Use `:help` to see the commands and `:quit` to end the session.

## Write and run a file

Save this program as `fall.aev`, or select **Run** in the interactive edition.

```aevia,run
fn main(): IO Unit = do
    let height: Float[Length] = 20.0 m
    let gravity: Float[Length/Time^2] = 9.80665 m/s^2
    let elapsed = sqrt(2.0 * height / gravity)
    println(elapsed)
```

Run `aevia check fall.aev` to check it without starting `main`. Then run `aevia run fall.aev`. Decimal literals are floating-point in this context. The intermediate (2h/g) has dimension time squared, so the result is close to 2.02 seconds. Replace `height` with `20.0 s` and annotate `elapsed` as `Float[Time]` to diagnose the wrong quantity at the point where you require a time.

## Learn in the browser and offline

A runnable block is a complete program. Browser runs can print results and diagnostics with bounded execution and no file or network access. Use the CLI for import trees, input files, and long-running computations. The single-page and downloadable editions contain the same lessons. Save each runnable example as its own file when working offline.

Next, learn how Aevia represents a calculation in [Values and functions](02-values-and-functions.md).
