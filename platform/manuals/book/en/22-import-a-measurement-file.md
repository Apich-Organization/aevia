# From a CSV fixture to a physical result

Process one file with a reproducible fixture, a typed boundary and a pure calculation. The file header records unit metadata. The numeric values become ordinary floats, so the model explicitly converts input values to quantities in metres per second.

## Check the calculation in isolation

The following browser example exercises the same arithmetic as the file program.

~~~aevia,run
fn summarize(raw: Tensor<1, Float>): Float[Length/Time] =
    mean(raw * 1.0 m/s)

test "the average retains the speed dimension" =
    summarize([1.0, 2.0, 3.0]) == 2.0 m/s

fn main(): IO Unit = do
    println(summarize([1.0, 2.0, 3.0]))
~~~

## Read the real file locally

Create `run.csv` beside the program:

~~~csv
time [s],speed [m/s]
0,1
1,2
2,3
~~~

Save the following as `traffic.aev` in the same working directory. Run `aevia --caps fs run traffic.aev`. It prints `2.0 m/s`. The file example needs the local CLI and an explicit file capability.

~~~aevia
fn summarize(raw: Tensor<1, Float>): Float[Length/Time] =
    mean(raw * 1.0 m/s)

fn main(): IO Unit = do
    loaded <- read_csv<{ time: Float, speed: Float }>("run.csv")
    match loaded with
    | Err(error) -> println(error)
    | Ok(table) ->
        match column(table, "speed") with
        | Err(error) -> println(error)
        | Ok(raw) -> println(summarize(raw))
~~~

## Exercise the failure boundaries

Rename the file to see an IO error. Restore it, then remove one field from a row to see the parser reject the malformed data. Restore the row and try values in kilometres per hour. Change the conversion in `summarize` to match the input rather than changing the label only. Treat the units in a file as data that must agree with the expected import policy. This example assumes the known fixture's units. When a producer can send different units, inspect `schema(table)` and choose an explicit conversion or reject the file.

A caller with unknown columns can read a dynamic table and use `column_dyn`. Keep the model signature specific once the columns and conversion are known, and compare the result with the pure test before scaling up to a large dataset. [Data library](/stdlib/Data?lang=en)
