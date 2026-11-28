//! The reported REPL expression works through the real CLI and file runner.
use std::io::Write;
use std::process::Command;
use std::process::Stdio;

#[test]
fn repl_and_scripts_accept_exact_decimal_unit_notation() {
    for optimized in [false, true] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_aevia"));
        command.args(["--color", "never"]);
        if optimized {
            command.arg("--release");
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(b"sqrt(17.0 m^1.5)\n:type 1 m^0.1\n:type 1 m^-2.5e-1\n:units L^1.5\n")
            .unwrap();
        let result = child.wait_with_output().unwrap();
        let out = String::from_utf8_lossy(&result.stdout);
        let err = String::from_utf8_lossy(&result.stderr);
        assert!(result.status.success(), "{out}\n{err}");
        assert!(
            out.contains("Float[L^(3/4)] = 4.123105625617661 m^(3/4)"),
            "{out}"
        );
        assert!(out.contains("Real[L^(1/10)]"), "{out}");
        assert!(out.contains("Real[1/L^(1/4)]"), "{out}");
    }
    let file =
        std::env::temp_dir().join(format!("aevia-decimal-exponent-{}.aev", std::process::id()));
    std::fs::write(
        &file,
        "let value: Float[L^0.75] = sqrt(17.0 m^1.5)\nfn main(): IO Unit = do\n    println(value)\n    println(\"inline={sqrt(17.0 m^1.5)}\")\n",
    )
    .unwrap();
    for optimized in [false, true] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_aevia"));
        command.args(["--color", "never", "run"]).arg(&file);
        if optimized {
            command.arg("--release");
        }
        let result = command.output().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(String::from_utf8_lossy(&result.stdout).contains("4.123105625617661 m^(3/4)"));
    }
    std::fs::remove_file(file).unwrap();
}
