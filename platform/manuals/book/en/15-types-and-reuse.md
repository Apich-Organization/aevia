# Reuse a model without losing its units

Suppose two instruments report a plausible interval. One measures a distance and the other measures a duration. In both cases you need the midpoint and the width. The arithmetic is the same, but it must keep the dimension of the input. Start with a model of the measurements, then give each experiment a readable name for its specialization.

## Give the shared calculation a dimension parameter

Save this as `intervals.aev`. The interval holds two endpoints of the same dimension. The constructor chooses an explicit result when the endpoints are in the wrong order.

```aevia,run
type Interval<d> = { lower: Float[d], upper: Float[d] }
type DistanceInterval = Interval<Length>
type TimingInterval = Interval<Time>
type IntervalResult<d> = | Accepted(Interval<d>) | Reversed

fn interval<d>(lower: Float[d], upper: Float[d]): IntervalResult<d> =
    if lower <= upper then Accepted({ lower = lower, upper = upper })
    else Reversed

fn midpoint<d>(bounds: Interval<d>): Float[d] =
    (bounds.lower + bounds.upper) / 2.0

fn width<d>(bounds: Interval<d>): Float[d] =
    bounds.upper - bounds.lower

fn describe<d>(reading: IntervalResult<d>): String =
    match reading with
    | Accepted(bounds) -> "midpoint = {midpoint(bounds)}, width = {width(bounds)}"
    | Reversed -> "lower must be at most upper"

test "a length midpoint preserves its dimension" =
    midpoint({ lower = 2.0 m, upper = 6.0 m }) == 4.0 m

fn main(): IO Unit = do
    let distance: DistanceInterval = { lower = 2.0 m, upper = 6.0 m }
    let timing: TimingInterval = { lower = 1.0 s, upper = 3.0 s }
    println(describe(interval(distance.lower, distance.upper)))
    println(describe(interval(timing.lower, timing.upper)))
    println(describe(interval(6.0 m, 2.0 m)))
```

Run `aevia run intervals.aev`. The report shows a midpoint and a width of `4.0 m` for the distance interval, a midpoint and a width of `2.0 s` for the timing interval, and a message for the reversed endpoints. Run `aevia test intervals.aev` to check the length midpoint against a result you can calculate by hand.

Look first at `midpoint<d>`. Each call uses one dimension for both endpoints, and the result has that same dimension `d`. The sum preserves it because the operands have equal dimensions. Dividing by the dimensionless number `2.0` leaves it unchanged. `width<d>` uses the same dimension relationship but computes a different quantity. A caller can pass an interval in length, time, or another dimension wherever this relationship makes sense.

## Use aliases to name the roles

`DistanceInterval` names `Interval<Length>` and `TimingInterval` names `Interval<Time>`. These are transparent aliases. A record with the appropriate fields can be passed to a function expecting the corresponding interval. The names make annotations easier to read when the two measurements occur in the same model. Handing a time interval to a length-only function still fails because expanding the aliases exposes different field types.

Compare the aliases with `IntervalResult<d>`. This variant has its own type identity, and its constructors express the two possible outcomes. The type checker can verify that a match covers both. The order of the endpoints is a relation between values, so `interval` checks it at run time. A record literal of type `DistanceInterval` can still hold reversed endpoints. Use `interval` at input boundaries where you need an explicit decision and keep `midpoint` and `width` for the cases that have been accepted.

## Test the edge of the abstraction

Try to pass `{ lower = 2.0 m, upper = 6.0 s }` to `midpoint`. Its fields cannot have a common dimension `d`. Now replace the body of `midpoint` with `bounds.lower + 1.0 m` and check the file. The body is rejected because adding a metre requires a length, while the declaration promises to work for any dimension. Restore the original body. If an instrument really requires a fixed offset of one metre, give that operation a length-specific signature.

The test uses small floating-point values whose midpoint is exactly representable. Choose a tolerance with the right dimension for data that accumulates rounding error. The same model can have a short length test and a separate timing test with a tolerance stated in seconds.

## Exercise — extend the model and share it

Write `pad<d>(bounds: Interval<d>, margin: Float[d]): Interval<d>`. Subtract the margin from the lower endpoint and add it to the upper endpoint. Choose what a negative margin means before using the function on real data. A positive margin of `0.5 m` should expand the distance interval to `1.5 m` through `6.5 m`. The same function should expand the timing interval to `0.5 s` through `3.5 s` when given `0.5 s`. Check that passing the time margin to the distance interval is rejected.

When the operations are ready to share, put the types, pure functions and tests in a model module. Keep input conversion and printing in the entry file. Document the dimension relationship and the ordering rule where callers first see the model. Then import the same functions from two small programs, one for each instrument, and run the module tests again. [Modules and packages](10-modules-and-packages.md)
