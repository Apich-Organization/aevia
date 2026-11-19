# IO, data and errors

Keep the numerical calculation in pure functions. Have `main` read external input, choose an error policy, and report the result. An `IO a` is an action yielding an `a`. Within a `do` block, bind the result of running an action with `<-`. Use `let` to bind an ordinary expression.

## Pass arguments through the command line

Save the following as `arguments.aev` and run `aevia run arguments.aev -- station-17 evening`. The browser version exercises the empty-argument case.

```aevia,run
fn main(): IO Unit = do
    let supplied = args()
    println(length(supplied))
    println(supplied)
```

The `--` delimiter separates program arguments from tool options. Decode arguments into model parameters at this boundary so the rest of the program can require numbers and physical units.

## Make a deliberate import boundary

For a local CSV or JSON analysis, first read the module documentation in [Data](/stdlib/Data?lang=en). Use a schema that states the expected columns, types and units. Treat unreadable data, an incompatible schema and invalid values as separate outcomes. Before a bulk run, check a few rows and use a tiny fixture with a known answer. Keep exact identifiers as text when leading zeros or long identifiers matter.

The execution capability for local file actions is `fs`. To try a script with only file capability, use `aevia --caps fs run path/to/program.aev`. Multiple capability names are comma-separated. `with_cap` narrows permissions in a scoped action. Use a process sandbox when running untrusted programs.

## Separate results from presentation

Return a structured result from the model layer, such as `{ value = estimate, samples = n }`. Present it with `println` in `main`, or produce a dataset with the standard library. When a pipeline evaluates many inputs, preserve the originating label for each error. A message that identifies the invalid file or column is more actionable than a general failure.

The exercise is to write a pure validator for a record containing a station label and a reading. Feed it three explicit examples before connecting any file input. See [Checking scientific results](11-testing.md) for test declarations.
