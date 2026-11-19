# Parallel execution and profiling

Parallelism is a good fit when every input can be processed independently and the work per item outweighs the scheduling cost. Write the serial computation first and preserve its result as a reference.

## Map an independent model over candidate inputs

Each candidate input below is a distance. The cost per candidate is small here so you can compare the results quickly. Replace the pure callback with a more expensive numerical model when measuring use of workers.

```aevia,run
fn score(x: Float[Length]): Float[Length^2] =
    (x - 2.5 m)^2

fn main(): IO Unit = do
    let candidates = [1.0 m, 2.0 m, 3.0 m, 4.0 m]
    println(map(score, candidates))
    println(par_map(score, candidates))
```

Run this file locally with `aevia --jobs 1 run candidates.aev` and again with `--jobs 4`. Compare the values at each index. The map preserves input order. For reductions, pay attention to regrouping and use a combination operation suited to the method.

## Measure the right part

Separate file and terminal work from the part you want to measure. Use a fixed fixture, a warm-up, repeat runs, and common settings for threads and optimization. Verify the result on each configuration before comparing time. `aevia bench --help` describes the tool's benchmark declarations and baseline format.

An optimized build of the toolchain and the CLI option `--release` address different stages. Build settings affect the interpreter executable. The CLI option enables optimizations for the checked program. Report both, along with enabled backend features, when sharing measurements.

## Explore further

Increase the number of inputs and the cost per input separately. Record where the parallel version becomes faster, and whether memory consumption increases. For tasks that perform IO, inspect the semantics of `spawn` and `await` and keep the side effects independent.
