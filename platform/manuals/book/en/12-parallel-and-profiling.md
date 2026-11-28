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

`--jobs` sets the language worker limit for this invocation. Without it, Aevia uses `AEVIA_JOBS`, then `RAYON_NUM_THREADS` or the CPUs available to the process. The pool grows up to that limit as the work grows. This example runs on the calling thread at either setting. To use a single thread for language evaluation and RSSN's global CPU pool, run with `RAYON_NUM_THREADS=1` and `--jobs 1`.

`par_tabulate(n, f)` calls a pure function on the indices `0` through `n - 1` and returns the results in that order. For large inputs, workers generate indices in batches. Small workloads and functions that cannot be passed to workers are evaluated locally.

Large dense exact matrices can also use this worker budget in `det`, `inv`, and `linsolve`. Each pivot is processed in order, and workers update independent rows. These operations return deterministic exact results.

Reading a large Fortran-order NPY array can use the active RSSN CPU pool for the row-major conversion, including arrays with three or more varying axes. Measure the conversion separately from disk reads when comparing worker counts.

## Choose and inspect the backend

```console
aevia devices
aevia --backend auto run candidates.aev
aevia --backend cpu --jobs 1 run candidates.aev
aevia --rssn-backend cpu-parallel run candidates.aev
```

`auto` is the default. It uses the Core IR interpreter and can compile eligible
hot scalar functions with RSSN's Cranelift JIT. Larger dense `Float` matrix
products may also run on a GPU that supports native F64. `--backend gpu`
considers smaller products and can use wgpu for exact integer-valued `Float`
products within its precision and range.
An absent adapter, a denied `gpu` capability or incompatible work leaves the
calculation on the CPU. `cpu-parallel` keeps work on the host and uses the
configured CPU worker count. `ir` and `ast` select the reference evaluators for
comparison. In a REPL, evaluator switches govern new evaluation and definitions.

`--rssn-backend` selects the process-wide RSSN backend used to compile portable
expressions. Its `auto` setting starts with the CPU tape and can compile hot
expressions in the background when the host supports Cranelift. It uses the
interpreter on other hosts. `aevia devices` lists the available selectors and
the current choice. In the REPL, use `:rssn-backend` to inspect it and
`:rssn-backend auto` to restore it. `--backend` controls the Aevia evaluator and
matrix routing independently.

The CLI or embedding application controls RSSN's process default. Aevia
source can choose a backend handle for a specific compilation. GPU discovery
and execution require the `gpu` capability, including calls that reuse a GPU
handle or compiled GPU program inside a narrower `with_cap` block.

Run `bazel build //:toolchain_minimal` to build a CLI that omits the optional
RSSN JIT, wgpu and NumPy integration. The executable is
`bazel-bin/toolchain_minimal/bin/aevia` (with `.exe` on Windows).

## Measure the right part

Separate file and terminal work from the part you want to measure. Use a fixed fixture, a warm-up, repeat runs, and common settings for threads and optimization. Verify the result on each configuration before comparing time. `aevia bench --help` describes the tool's benchmark declarations and baseline format.

An optimized build of the toolchain and the CLI option `--release` address different stages. Build settings affect the interpreter executable. The CLI option enables optimizations for the checked program. Report both, along with enabled backend features, when sharing measurements.

## Sample a program on Windows

The `aevia-prof` development tool samples a child process on x86-64 Windows
without elevation. Build a CLI with profiling symbols from the repository root.
In PowerShell, set `$profiler` to the path of your `aevia-prof.exe` sampler and
pass the symbolized CLI as its first argument.

```powershell
bazel build //:toolchain_profiling
$env:AEVIA_PROF_FOLDED = "candidates.folded"
& $profiler .\bazel-bin\toolchain_profiling\bin\aevia.exe run candidates.aev
Remove-Item Env:AEVIA_PROF_FOLDED
```

The stderr summary reports self samples at the leaf and inclusive samples along the stack, counting each function at most once per sample. Set `AEVIA_PROF_TOP` to change the number of summary rows and `AEVIA_PROF_INTERVAL_US` to change the sampling interval. The folded export uses the same busy samples and records normalized symbol names without summary truncation, in caller-to-leaf order. Give each export a new path. The profiler preserves the child's output and exit status, and reports an export error on stderr.

## Explore further

Increase the number of inputs and the cost per input separately. Record where the parallel version becomes faster, and whether memory consumption increases. For tasks that perform IO, inspect the semantics of `spawn` and `await` and keep the side effects independent.

## Check your understanding

Run the same fixed task with two worker counts and save the results before measuring. Then increase the task size while keeping the model constant. Compare where parallel execution starts to help, and record the workload and feature flags with the timing. [Reproducible parameter sweeps](21-sweep-and-compare.md)
