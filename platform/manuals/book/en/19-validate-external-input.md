# Validate input before allocating work

A text field asks an experiment for its sample count. A valid integer may still be too small to summarize or too large to run under a chosen budget. Make both decisions before allocating a tensor or starting a solver. This keeps the parsing error separate from the model's policy.

## Parse, then apply the policy

~~~aevia,run
type SampleCount = | Count(Int) | NotAnInteger | OutsideRange

fn sample_count(text: String): SampleCount =
    match parse_int(text) with
    | Err(_) -> NotAnInteger
    | Ok(n) ->
        if n >= 2 && n <= 10000 then Count(n)
        else OutsideRange

fn report(input: String): String =
    match sample_count(input) with
    | Count(n) -> "accepted {n} samples"
    | NotAnInteger -> "enter a whole number"
    | OutsideRange -> "enter a count from 2 through 10000"

test "minimum sample count is allowed" = report("2") == "accepted 2 samples"
test "fractional text fails before range checking" = report("2.5") == "enter a whole number"
test "unbounded input cannot reach the allocation stage" =
    report("1000000000000000000000") == "enter a count from 2 through 10000"

fn main(): IO Unit = do
    println(report("24"))
    println(report("2.5"))
    println(report("0"))
~~~

Save the program as `sample_count.aev`. It prints an accepted count for `24`, a syntax message for `2.5`, and a range message for `0`. Run `aevia test sample_count.aev`. Notice the large decimal used in the third test. Aevia can represent that integer, so the range test must still protect the caller's allocation budget.

## Connect one external source

Extend `main` to read a count from the argument list described in [IO and data](05-io-and-data.md). Decide separately how to report a missing argument. Keep the string enclosed in its input context in any message, such as a job name or a record line. The browser examples use fixed text so both the accepted and rejected paths run without terminal input.

Only pass a count tagged `Count` to code that allocates samples. Put the range in one policy function when several commands share it. If two callers need different limits, accept the limit as a typed argument and test each caller's policy.

## Work through boundaries

Add tests for `10000`, `10001`, `-1` and the empty string. Predict the branch before running each test. For a measurement, use `parse_float` and introduce the chosen unit explicitly at the boundary. Give the model a dimensioned parameter so it can assume that conversion has happened. [Dimensions and units](03-dimensions-and-units.md)
