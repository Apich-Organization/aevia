# Statistics and measurement

A summary describes both the data and a decision about what to include. Before computing it, record the units, sample selection and meaning of missing data. Use an estimator whose sensitivity matches the question you ask.

## Compare mean with median

For a group of length readings, keep all entries in the same physical dimension. One unusually large value shifts the mean, while the median depends on the middle of the ordered sample.

```aevia,run
fn main(): IO Unit = do
    let readings = [0.98 m, 1.01 m, 1.02 m, 1.04 m, 2.5 m]
    let center = median(readings)
    println(mean(readings))
    println(center)
    println(map(fn x => abs(x - center), readings))
```

Compare the two summaries with the 2.5 m reading removed. The mean answers a question about total divided by count. The median answers a question about central rank. Keep both numbers when reporting the effect of an unusual observation.

## Keep uncertainty separate from sampling variability

A stated instrumental uncertainty describes a measurement process. A standard deviation across samples describes spread in a dataset. Model these separately. Consult [Measurement](/stdlib/Measurement?lang=en) for measurement values and uncertainty propagation, and [Stats](/stdlib/Stats?lang=en) for sample summaries and tests. If observations share a source of error, account for correlation before using a formula that assumes independence.

## Check small and extreme samples

Try a one-element tensor, an even-length tensor, and a tensor with all values equal. Compare empty-input behavior against the individual operation's documentation. Values near a rejection threshold should be tested on both sides. For randomized work, record seeds and sample sizes together with the method.
