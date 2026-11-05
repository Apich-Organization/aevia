//! The before/after number for the `f64` matmul backend.
//!
//! Run it in release, because the question is what a user waits for:
//!
//! ```console
//! CARGO_BUILD_JOBS=4 cargo run -p aevia-std --release --example matmul_bench
//! ```
//!
//! "before" is [`aevia_std::linalg::legacy::matmul`], the hand-written triple
//! loop the runtime's `@` operator still runs; "after" is
//! [`aevia_std::linalg::matmul`], the `faer` kernel. Both are given the same
//! 256×256 operands and are run enough times that the number is a median, not a
//! single sample that happened to be scheduled well.
//!
//! `RAYON_NUM_THREADS` is honoured: `faer` reads the global rayon pool.

use std::time::Duration;
use std::time::Instant;

use aevia_std::linalg;

const N: usize = 256;
const REPEATS: usize = 15;

fn operand(seed: u64) -> Vec<f64> {
    let mut state = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
    (0..N * N)
        .map(|_| {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let unit = ((state >> 11) as f64) / ((1u64 << 53) as f64);
            unit * 2.0 - 1.0
        })
        .collect()
}

fn median(mut samples: Vec<Duration>) -> Duration {
    samples.sort_unstable();
    samples[samples.len() / 2]
}

fn report(
    label: &str,
    elapsed: Duration,
) {
    let seconds = elapsed.as_secs_f64();
    let flops = 2.0 * (N as f64).powi(3);
    println!(
        "{label:<10} {seconds:>8.3} ms   {:>7.2} GFLOP/s",
        flops / seconds / 1e9
    );
}

/// Median wall time of `operation`, over [`REPEATS`] samples.
fn time(mut operation: impl FnMut()) -> Duration {
    let mut samples = Vec::with_capacity(REPEATS);
    for _ in 0..REPEATS {
        let start = Instant::now();
        operation();
        samples.push(start.elapsed());
    }
    median(samples)
}

fn main() {
    let a = operand(1);
    let b = operand(2);

    // One untimed call each, so neither side pays for a cold cache or a first
    // allocation inside the measurement.
    let _ = linalg::legacy::matmul(N, N, N, &a, &b);
    let _ = linalg::matmul(N, N, N, &a, &b).expect("faer matmul");

    let before = time(|| {
        std::hint::black_box(linalg::legacy::matmul(N, N, N, &a, &b));
    });
    let after = time(|| {
        std::hint::black_box(linalg::matmul(N, N, N, &a, &b).expect("faer matmul"));
    });

    println!("{N}x{N} f64 matmul, {REPEATS} samples, median of each:");
    report("before", before);
    report("after", after);
    println!(
        "speedup    {:.1}x",
        before.as_secs_f64() / after.as_secs_f64()
    );

    // The factorisations moved too, so they get the same treatment: a move
    // justified by "it should be faster" is not justified.
    println!("\n{N}x{N} f64 factorisations, median of {REPEATS}:");
    let rhs: Vec<f64> = a[..N].to_vec();
    for (label, old, new) in [
        (
            "det",
            time(|| drop(std::hint::black_box(linalg::legacy::det(N, N, &a)))),
            time(|| drop(std::hint::black_box(linalg::det(N, N, &a)))),
        ),
        (
            "inv",
            time(|| drop(std::hint::black_box(linalg::legacy::inv(N, N, &a)))),
            time(|| drop(std::hint::black_box(linalg::inv(N, N, &a)))),
        ),
        (
            "solve",
            time(|| drop(std::hint::black_box(linalg::legacy::solve(N, N, &a, &rhs)))),
            time(|| drop(std::hint::black_box(linalg::solve(N, N, &a, &rhs)))),
        ),
        (
            "lu",
            time(|| drop(std::hint::black_box(linalg::legacy::lu(N, N, &a)))),
            time(|| drop(std::hint::black_box(linalg::lu(N, N, &a)))),
        ),
        (
            "qr",
            time(|| drop(std::hint::black_box(linalg::legacy::qr(N, N, &a)))),
            time(|| drop(std::hint::black_box(linalg::qr(N, N, &a)))),
        ),
    ] {
        println!(
            "{label:<6} before {:>8.3} ms   after {:>8.3} ms   {:>5.2}x",
            old.as_secs_f64() * 1e3,
            new.as_secs_f64() * 1e3,
            old.as_secs_f64() / new.as_secs_f64()
        );
    }
}
