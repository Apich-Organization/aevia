//! Discoverable command help, kept separate from command execution.

use std::process::ExitCode;

const GLOBAL: &str = "Global options (before or after the command; program arguments follow --):
  --release          Enable optimized execution while retaining runtime checks
  --jobs N           Parallel evaluation workers (default: available CPUs / AEVIA_JOBS)
  --max-stack MiB    Interpreter stack size (default: 1024 MiB)
  --budget STEPS     Limit tail calls and IO execution steps (E0805 on exhaustion)
  --caps LIST        Allow only fs,net,env,rand,gpu; --caps= grants none
  --locked           Require an existing, up-to-date package lock file
  --color WHEN       auto, always, never (default: auto; respects NO_COLOR, TERM=dumb)
  -h, --help         Show help for this command
  -V, --version      Show the toolchain version";

pub(crate) fn overview() {
    println!(
        "aevia — scientific computing with dimensions as types\n\nUsage:\n  aevia                         Start an interactive REPL\n  aevia <command> [options]      Work with a program or package\n  aevia help <command>          Show command options and examples\n\nStart here:\n  aevia                         Try expressions, definitions, and pasted programs\n  aevia init motion             Create a package, then: cd motion && aevia run\n  aevia run examples/kinematics.aev   Run a source file with a zero-argument main\n  aevia check --all             Check every module in the current package\n\nCommon workflows:\n  aevia run --example orbit     Run a package example\n  aevia test --doc src          Validate doc fences (compile_fail asserts rejection)\n  aevia doc --out docs          Generate API documentation\n  aevia publish --dry-run       Validate a package without uploading\n  aevia bench --save-baseline baseline.json\n  aevia bench --compare-baseline baseline.json --threshold 5%\n  Benchmark protocol: release-built toolchain, same machine and IR setting, median of N.\n\nRun and inspect:\n  run       Run a file, the package entry, or a package example\n  repl      Interactive session with :help, :type, :load, and multiline input\n  check     Type-check files without running them\n  watch     Check a file again when its dependencies change\n  test      Run test declarations or documentation examples\n  bench     Measure @bench functions and compare saved baselines\n\nSource tools:\n  lint      Run style, correctness, and compiler lints\n  fmt       Format source; --check verifies formatting without writing\n  doc       Generate Markdown API documentation\n  lsp       Start the language server for an editor\n  rssn      Inspect the scientific kernel and generate bindings\n\nPackages:\n  init      Create a new package\n  add       Add a path or registry dependency\n  update    Resolve dependency versions and update aevia.lock\n  fetch     Download locked registry dependencies\n  tree      Display the dependency tree\n  clean     Remove generated build products\n  publish   Check, sign, and upload a package\n  docs      Locate package documentation\n  pkg       Run the separate registry client (aevia-pkg)\n  login, search, info, yank, verify  Registry account and package operations\n\n{GLOBAL}\n\nInteractive quick start:\n  let speed = 3\n  speed * 14\n  :type speed\n  :help\n\nFiles and packages:\n  Source files use .aev. With no file, run/check use the entry from aevia.toml.\n  check/run cache checked modules under .aevia/cache; --no-cache bypasses it.\n  Registry packages are resolved from cached metadata and verified sources in .aevia/registry.\n  Registry networking requires aevia-pkg on PATH (or AEVIA_PKG).\n\nExit status:\n  0 success; 1 compilation, runtime, or validation failure; 2 invalid command usage.\n  Use aevia <command> --help for detailed options and examples."
    );
}

/// Help never reads or edits a package, and never swallows program arguments.
pub(crate) fn route(args: &[String]) -> Option<ExitCode> {
    if args.first().is_some_and(|arg| arg == "help") {
        if args.len() > 2 {
            eprintln!("aevia help: expected at most one command, as in `aevia help run`");
            return Some(ExitCode::from(2));
        }
        return Some(show(args.get(1).map(String::as_str)));
    }
    let before_separator = args.iter().take_while(|arg| arg.as_str() != "--");
    if before_separator
        .into_iter()
        .any(|arg| arg == "--help" || arg == "-h")
    {
        return Some(show(
            args.first()
                .map(String::as_str)
                .filter(|arg| *arg != "--help" && *arg != "-h"),
        ));
    }
    None
}

fn show(command: Option<&str>) -> ExitCode {
    let Some(command) = command else {
        overview();
        return ExitCode::SUCCESS;
    };
    let text = match command {
        | "run" => {
            "aevia run — compile and run a program\n\nUsage:\n  aevia run [FILE.aev] [--no-cache] [-- PROGRAM_ARGS...]\n  aevia run --example NAME [-- PROGRAM_ARGS...]\n\nOptions:\n  --example NAME   Use examples/NAME.aev from the current package\n  --no-cache       Recompile without reading or writing checked-module caches\n  -- ARGS...       Pass remaining arguments to the program (available via args())\n\nWith no file, use the package entry from aevia.toml. A file and --example cannot\nbe combined. Programs must define a zero-argument main; IO main is executed.\n\nExamples:\n  aevia run hello.aev\n  aevia run --release simulation.aev\n  aevia run --example orbit -- --steps 100"
        },
        | "repl" => {
            "aevia repl — explore expressions and programs interactively\n\nUsage:\n  aevia\n  aevia repl [GLOBAL_OPTIONS]\n\nStart without a command to enter the REPL. Definitions remain available until\n:reset or exit. Redefining a let or fn name overwrites it for later code, including\na new type or dimension. Expressions display their inferred type and value. Paste whole\nblocks or programs; use :help inside the session for input and editing commands.\n\nExamples:\n  let scale = 6\n  scale * 7\n  :type scale\n  :load examples/kinematics.aev\n  :quit\n\nPackage/stdlib imports use the current package; :load FILE resolves its own\npackage and directory. :imports lists modules; :reset refreshes frozen sources.\nCtrl-C interrupts evaluation and returns to the prompt (E0812).\n\nOutput and documentation:\n  :pretty 1/2         Two-dimensional terminal mathematics\n  :typst 1/2          Typst math source (:latex for LaTeX)\n  :format pretty     Set the normal result format; :format show restores it\n  :repr \"hello\"      Source representation\n  :help sqrt          Item signature, documentation, and examples\n  :doc M.function     Documentation for an imported item\n  :apropos matrix     Search names and documentation\n  :help :show         Help for a REPL command\n\nPiped input is supported: aevia < session.aev. Prompts are hidden and failed\nsubmissions return a failure exit status."
        },
        | "check" => {
            "aevia check — type-check without evaluating a program\n\nUsage:\n  aevia check [FILE.aev ...] [--no-cache] [-A|-W|-D|-F LINT]\n  aevia check --all [--no-cache] [-A|-W|-D|-F LINT]\n\nOptions:\n  --all                Check every source module in the current package, including unused ones\n  --no-cache           Bypass the checked-program and checked-module disk caches\n  -A, --allow LINT     Suppress a compiler lint or group (e.g. warnings)\n  -W, --warn LINT      Emit a compiler lint or group as a warning\n  -D, --deny LINT      Promote a compiler lint or group (e.g. warnings) to an error\n  -F, --forbid LINT    Forbid a compiler lint or group (cannot be lowered)\n\nWith no file, check the package entry and its imports. Multiple files and quoted\nglob patterns are supported. --all operates on the package and takes no files.\n\nExamples:\n  aevia check model.aev\n  aevia check -D warnings --all\n  aevia --locked check --all"
        },
        | "lint" => {
            "aevia lint — run compiler, correctness, style, and pedantic lints\n\nUsage:\n  aevia lint [FILE.aev ...] [OPTIONS]\n  aevia lint --all [OPTIONS]\n  aevia lint --list\n  aevia lint --explain CODE_OR_NAME\n\nOptions:\n  --all                      Lint every module in the current package\n  --fix                      Apply machine-applicable fixes in place\n  --allow-dirty              Allow --fix with uncommitted git changes\n  --format human|json|sarif  Select output format (default: human)\n  -A, --allow LINT           Allow a lint or group\n  -W, --warn LINT            Warn on a lint or group\n  -D, --deny LINT            Promote a lint or group (e.g. warnings) to an error\n  -F, --forbid LINT          Forbid a lint or group (cannot be lowered)\n  --list                     List all built-in lints and default levels\n  --explain LINT             Show detailed explanation for a lint\n  --no-cache                 Bypass the checked-module disk cache\n\nExamples:\n  aevia lint --all\n  aevia lint -D warnings --format sarif --all\n  aevia lint --fix src/main.aev"
        },
        | "watch" => {
            "aevia watch — recheck when source dependencies change\n\nUsage:\n  aevia watch [FILE.aev] [--once | --runs N]\n\nOptions:\n  --once     Check once and exit\n  --runs N   Stop after N check cycles (useful for scripted sessions)\n\nThe default file is the package entry. Leave the watcher with Ctrl-C.\n\nExamples:\n  aevia watch\n  aevia watch simulation.aev --once"
        },
        | "test" => {
            "aevia test — run tests or validate documentation examples\n\nUsage:\n  aevia test [FILE.aev] [--filter PATTERN]\n  aevia test --doc [FILE_OR_DIR ...] [--filter PATTERN]\n\nOptions:\n  --filter PATTERN   Run only matching test or snippet names\n  --doc              Compile aevia fenced blocks in ## documentation comments\n\nNormal mode runs test declarations in the file (the package entry by default).\nDoc snippets compile independently with their declaring module's imports. Their\nfirst line may be compile_fail or compile_fail E0401 to assert rejection.\n\nExamples:\n  aevia test --filter matrix\n  aevia test --doc src"
        },
        | "bench" => {
            "aevia bench — measure @bench functions and detect regressions\n\nUsage:\n  aevia bench [FILE_OR_DIR ...] [OPTIONS]\n\nOptions:\n  --filter PATTERN         Measure matching benchmarks only\n  --iterations N           Timed calls per benchmark (default: 100, minimum: 1)\n  --warmup N               Untimed calls before measurement (default: 3)\n  --save-baseline FILE     Write medians and measurement protocol as JSON\n  --compare-baseline FILE  Compare against stored medians; fail on regressions\n  --threshold RATIO        Allowed slowdown (default: 0.05; 5% also accepted)\n\nUse the same release-built toolchain, machine, and protocol for comparisons.\nResults report the median and a 95% confidence interval. With no files, the\ncurrent package's source tree is used.\n\nExamples:\n  aevia bench --release --save-baseline baseline.json\n  aevia bench --release --compare-baseline baseline.json --threshold 5%"
        },
        | "fmt" => {
            "aevia fmt — format Aevia source\n\nUsage:\n  aevia fmt [FILE_OR_DIR ...] [--check]\n\nOptions:\n  --check   Report files needing formatting and exit 1; do not write them\n\nWith no files, format the current package's src directory. Explicit directories\nare searched for .aev files. Parsing failures leave the source file intact.\n\nExamples:\n  aevia fmt src\n  aevia fmt --check 'examples/*.aev'"
        },
        | "doc" => {
            "aevia doc — generate Markdown API documentation\n\nUsage:\n  aevia doc [FILE_OR_DIR ...] [--out DIR] [--open]\n\nOptions:\n  --out DIR   Destination directory (default: package docs/ or ./docs)\n  --open      Open generated documentation when an interactive terminal is available\n\nWith no files, document the package's src tree. ## comments become API docs.\nValidate their executable examples with aevia test --doc.\n\nExamples:\n  aevia doc --out docs\n  aevia doc src --open"
        },
        | "init" => {
            "aevia init — create a package\n\nUsage:\n  aevia init NAME\n\nNames use lower-case letters, digits, and underscores. Creates NAME/aevia.toml,\nNAME/aevia.lock, and NAME/src/main.aev. Existing directories are never overwritten.\n\nExample:\n  aevia init kinematics\n  cd kinematics\n  aevia run"
        },
        | "add" => {
            "aevia add — add a package dependency and update the lock file\n\nUsage:\n  aevia add PATH [--as IMPORT_NAME] [--offline]\n  aevia add OWNER/NAME[@REQUIREMENT] [--as IMPORT_NAME] [--offline]\n\nOptions:\n  --as NAME   Select the module import root\n  --offline   Use cached registry information without network access\n\nA path must contain aevia.toml. Registry names require an owner. --locked rejects\nchanges that would modify the lock file. Networking uses the aevia-pkg client.\n\nExamples:\n  aevia add ../geometry --as Geometry\n  aevia add alice/math@0.1 --offline"
        },
        | "update" => {
            "aevia update — resolve compatible dependency versions\n\nUsage:\n  aevia update [OWNER/NAME ...] [--offline]\n\nOptions:\n  --offline   Use the cached sparse registry index without fetching\n\nUpdates aevia.lock for the current package. Name dependencies to update only\nthose packages; omit names to update all. --locked rejects lock-file changes.\n\nExamples:\n  aevia update\n  aevia update alice/math --offline"
        },
        | "fetch" => {
            "aevia fetch — download and verify locked registry dependencies\n\nUsage:\n  aevia fetch [--offline] [--force]\n\nOptions:\n  --offline   Require all needed registry content to be cached\n  --force     Refetch packages even when their sources are present\n\nUses the current package's aevia.lock and the separate aevia-pkg client.\n\nExample:\n  aevia fetch --offline"
        },
        | "tree" => {
            "aevia tree — display the current package's dependencies\n\nUsage:\n  aevia tree [--offline]\n\nOptions:\n  --offline   Resolve from cached registry metadata without fetching\n\nShows both path and registry packages.\n\nExample:\n  aevia tree --offline"
        },
        | "clean" => {
            "aevia clean — remove generated build products\n\nUsage:\n  aevia clean [--registry]\n\nOptions:\n  --registry   Also remove downloaded registry content\n\nRemoves the package's .aevia build directory while preserving .aevia/registry\nunless --registry is specified. Source files and aevia.lock are kept.\n\nExamples:\n  aevia clean\n  aevia clean --registry"
        },
        | "lsp" => {
            "aevia lsp — serve the language server over standard IO\n\nUsage:\n  aevia lsp [--stdio]\n\nOptions:\n  --stdio   Explicitly select standard input/output (the only transport)\n\nFor editor configuration, set the server executable to aevia and its arguments\nto [\"lsp\", \"--stdio\"]. Standard output carries protocol frames only.\n\nExample:\n  aevia lsp --stdio"
        },
        | "rssn" => {
            "aevia rssn — inspect and generate scientific kernel bindings\n\nUsage:\n  aevia rssn [--audit]\n  aevia rssn --emit --out DIR [--namespaces] [--audit]\n\nOptions:\n  --audit        Report typed and dynamic binding coverage\n  --emit         Generate the Rssn module (requires an output directory)\n  --out DIR      Select destination; implies --emit\n  --namespaces   Generate split namespace modules (requires --emit --out DIR)\n\nWithout --emit, report the kernel version, manifest, and available operations.\n\nExamples:\n  aevia rssn --audit\n  aevia rssn --emit --out generated --namespaces"
        },
        | "publish" => {
            "aevia publish — validate, sign, and upload the current package\n\nUsage:\n  aevia publish [--dry-run] [--no-verify]\n\nOptions:\n  --dry-run     Prepare and validate without uploading\n  --no-verify   Skip the local source verification checks\n\nRequires the aevia-pkg registry client and a configured signing identity.\n\nExample:\n  aevia publish --dry-run"
        },
        | "docs" => {
            "aevia docs — locate a package's hosted documentation\n\nUsage:\n  aevia docs [OWNER/NAME[@VERSION]] [--open]\n\nOptions:\n  --open   Open the documentation URL in a browser\n\nWith no package, use the current package. AEVIA_REGISTRY selects the registry.\n\nExample:\n  aevia docs alice/math --open"
        },
        | "pkg" => {
            "aevia pkg — use the separate registry client\n\nUsage:\n  aevia pkg COMMAND [OPTIONS]\n\nThe client handles login, signing keys, registry queries, and package uploads.\nInstall aevia-pkg on PATH, beside aevia, or set AEVIA_PKG to its executable.\nFor client-specific flags, run aevia-pkg COMMAND --help directly.\n\nExamples:\n  aevia pkg status\n  aevia pkg key"
        },
        | "login" | "search" | "info" | "yank" | "verify" | "logout" | "key" | "status"
        | "lock" => {
            println!(
                "aevia {command} — registry client operation\n\nUsage:\n  aevia {command} [ARGUMENTS]\n\nThis command is forwarded to the separate aevia-pkg client. Install it on PATH\nor set AEVIA_PKG. Run aevia-pkg {command} --help for all client-specific flags.\n\nCommon registry commands:\n  aevia login                         Bind a signing key to your account\n  aevia search QUERY                  Search published packages\n  aevia info OWNER/NAME               Inspect a package\n  aevia yank OWNER/NAME@VERSION        Withdraw a version (--undo restores it)\n  aevia verify OWNER/NAME@VERSION      Check a downloaded signature"
            );
            return ExitCode::SUCCESS;
        },
        | unknown => {
            eprintln!(
                "aevia help: unknown command `{unknown}`; use `aevia --help` to list commands"
            );
            return ExitCode::from(2);
        },
    };
    println!("{text}\n\n{GLOBAL}");
    ExitCode::SUCCESS
}
