# Diagnose a failing calculation

A timing experiment gives you two measurements, a distance and an elapsed time. Dividing them gives a speed. We'll use the same program to investigate a reading entered with the wrong unit and an elapsed time of zero. The compiler can catch the wrong unit before running the program. The division error depends on the value that reaches the calculation.

## Start from a known result

Save this program as `reading.aev`. Use exact `Real` quantities here so the zero-time experiment produces a division error.

```aevia,run
type Reading = { distance: Real[Length], elapsed: Real[Time] }

fn speed(distance: Real[Length], elapsed: Real[Time]): Real[Length/Time] =
    distance / elapsed

fn report(reading: Reading): String =
    let velocity = speed(reading.distance, reading.elapsed)
    "speed = {velocity}"

fn main(): IO Unit = do
    let reading: Reading = { distance = 12 m, elapsed = 3 s }
    println(report(reading))
```

Run `aevia check reading.aev` and then `aevia run reading.aev`. The check should succeed and the run should print `speed = 4 m/s`. Keep this input available while you make changes. It gives you a quick way to confirm that a fix for a bad reading still preserves the ordinary calculation.

## Read a compiler diagnostic in context

Change `elapsed = 3 s` to `elapsed = 3 m` and run the check again. The error starts with `E0401`, and the source label says `expected T, found L`. Read that label alongside `Reading`. The annotation gives the `elapsed` field the dimension of time, while the replacement literal has the dimension of length. The diagnostic can underline the whole record because that is where the annotation and the inferred field types meet. Compare each field with its declaration, then restore the original reading.

In an editor connected to `aevia-lsp` or `aevia lsp --stdio`, make the same edit and inspect the diagnostic on the open buffer. Hover over `speed` to see the types it accepts and produces. Use Go to Definition on `Reading` if the record is declared elsewhere. When comparing editor feedback with a shell command, save the file first so both are working from the same source.

## Isolate the calculation in the REPL

Start `aevia repl` from the directory containing the restored file. Try this sequence. Use the actual path if you started the REPL elsewhere.

```text
:load reading.aev
:type speed
speed(12 m, 3 s)
speed(1 m, 2 s)
:quit
```

The calls produce `4 m/s` and `0.5 m/s`. The type inspection shows that both arguments carry dimensions, and the result carries length divided by time. The second call also tests a fractional speed without having to edit the reporting code. Use `:help` for the available commands and `:reset` when you want a fresh session. You can open the same REPL under **Aevia: Open REPL** in VS Code.

## Follow a runtime error to its input

Copy the original program to `reading-zero.aev` and set `elapsed = 0 s`. Run `aevia check reading-zero.aev` and then `aevia run reading-zero.aev`. The check still succeeds. The run fails with `E0801` at `distance / elapsed` and includes call sites in `report` and `main`. Read upward from the failing expression until you find where the elapsed time entered the program.

To inspect an execution before it fails, save the zero-time file and put a line breakpoint on `distance / elapsed`. With the Aevia extension installed in VS Code, choose **Aevia: Debug Current File**. An editor that accepts a custom Debug Adapter Protocol configuration can start `aevia debug --stdio` and launch with the saved file's path in `program`. Use the stack to find the callers and Variables to see that `elapsed` is `0 s`. Put a breakpoint in `report` and use Step Into to follow the call into `speed`. Watches inspect binding names from the stopped frame.

The adapter uses a serial source evaluator. For programs that read input, set the `stdin` list in the launch configuration. Test your final model with the normal run command and the execution options you intend to use.

## Exercise — choose an input policy

Add a function that accepts a `Reading` and returns `Result<Real[Length/Time], String>`. Return an error for zero or negative elapsed times and a speed otherwise. Comparisons on exact `Real` values produce propositions. Use `decide(reading.elapsed > 0 s) == Some(true)` as the condition for accepting a reading, and use the same pattern to test equality of exact speeds. Change the report to display each outcome. Add tests for zero, a negative duration, and the original three-second reading. The first two should produce your chosen error message. The third should still produce `4 m/s`. Use the same three inputs to verify the result by hand, in the REPL, and with `aevia test`. [Checking scientific results](11-testing.md)
