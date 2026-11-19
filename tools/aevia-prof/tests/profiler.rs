//! Exercises the profiler as a child process, including its exit status and output.

use std::process::Command;

#[test]
fn reports_when_no_program_can_be_run() {
    let output = Command::new(env!("CARGO_BIN_EXE_aevia-prof"))
        .output()
        .expect("start aevia-prof");
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let error = String::from_utf8(output.stderr).expect("UTF-8 diagnostic");
    if cfg!(all(windows, target_arch = "x86_64")) {
        assert!(error.contains("usage: aevia-prof"), "{error}");
    } else {
        assert!(error.contains("requires x86-64 Windows"), "{error}");
    }
}

#[cfg(all(windows, target_arch = "x86_64"))]
#[test]
fn rejects_an_unavailable_program() {
    let program = std::env::current_exe()
        .expect("the test executable")
        .with_file_name("aevia-prof-fixture-does-not-exist.exe");
    let output = Command::new(env!("CARGO_BIN_EXE_aevia-prof"))
        .arg(program)
        .output()
        .expect("start aevia-prof");
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot start"));
}

#[cfg(all(windows, target_arch = "x86_64"))]
#[test]
fn profiles_a_real_child_and_propagates_its_exit_code() {
    let output = Command::new(env!("CARGO_BIN_EXE_aevia-prof"))
        .arg(std::env::current_exe().expect("the test executable"))
        .args(["--exact", "sampling_fixture", "--nocapture"])
        .env("AEVIA_PROF_FIXTURE", "1")
        .env("AEVIA_PROF_TOP", "2")
        .env("AEVIA_PROF_INTERVAL_US", "2000")
        .env_remove("AEVIA_PROF_DEBUG")
        .output()
        .expect("run a sampling session");
    assert_eq!(output.status.code(), Some(37), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stdout).contains("sampling fixture output"));
    let report = String::from_utf8_lossy(&output.stderr);
    for heading in ["== self (", "== inclusive ("] {
        let samples = report
            .lines()
            .find_map(|line| line.strip_prefix(heading))
            .and_then(|line| line.split(' ').next())
            .and_then(|count| count.parse::<u64>().ok());
        assert!(samples.is_some_and(|count| count > 0), "{report}");
    }
    let rows = report.lines().filter(|line| line.contains('%')).count();
    assert!((2..=4).contains(&rows), "{report}");
}

#[cfg(all(windows, target_arch = "x86_64"))]
#[test]
fn sampling_fixture() {
    if std::env::var_os("AEVIA_PROF_FIXTURE").is_none() {
        return;
    }
    println!("sampling fixture output");
    let start = std::time::Instant::now();
    let mut state = 1u64;
    while start.elapsed() < std::time::Duration::from_millis(600) {
        for _ in 0..4096 {
            state = std::hint::black_box(state.wrapping_mul(11).wrapping_add(41));
        }
    }
    std::process::exit(37);
}
