//! CLI definition and dispatch.

use crate::commands;
use crate::diagnostics::AeviaResult;
use clap::{Parser, Subcommand};
use colored::Colorize;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "aevia",
    version,
    about = "Aevia: scientific DSL with dimensional analysis",
    after_help = "Run a single file without a subcommand:\n  aevia src/main.ae    # same as: aevia run src/main.ae"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<CliCommand>,

    /// `.ae` file to run when no subcommand is given (shorthand for `aevia run`)
    #[arg(value_name = "FILE")]
    pub file: Option<PathBuf>,
}

#[derive(Subcommand, Debug)]
pub enum CliCommand {
    /// Create a new Aevia project
    New {
        /// Project name
        name: String,
        /// Parent directory for the new project
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
    },
    /// Compile `.ae` sources to RSSN
    Build {
        /// Source files or directories
        #[arg(required = true)]
        paths: Vec<PathBuf>,
    },
    /// Build and execute an `.ae` file
    Run {
        /// Entry `.ae` file
        file: PathBuf,
        /// Arguments for the function
        args: Vec<f64>,
    },
    /// Type-check without compiling
    Check {
        paths: Vec<PathBuf>,
    },
    /// Format `.ae` sources
    Fmt {
        paths: Vec<PathBuf>,
    },
    /// Static analysis (dimensions, style)
    Lint {
        paths: Vec<PathBuf>,
    },
    /// Generate documentation from doc comments
    Doc {
        paths: Vec<PathBuf>,
        /// Output directory (optional)
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Generate HTML documentation instead of Markdown
        #[arg(long)]
        html: bool,
    },
    /// Interactive REPL
    Shell,
    /// Run project tests
    Test {
        /// Project root (defaults to current directory)
        #[arg(default_value = ".")]
        path: PathBuf,
    },
}

pub fn execute() -> AeviaResult<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(sub) => dispatch_subcommand(sub),
        None => match cli.file {
            Some(file) => commands::run(commands::RunTarget { file, args: vec![] }),
            None => {
                Cli::parse_from(["aevia", "--help"]);
                Ok(())
            }
        },
    }
}

fn dispatch_subcommand(sub: CliCommand) -> AeviaResult<()> {
    match sub {
        CliCommand::New { name, path } => {
            let dir = commands::new(&name, &path)?;
            println!(
                "{} Created Aevia project `{}` at {}",
                "✓".green(),
                name.bold(),
                dir.display()
            );
            println!("  Run: cd {} && aevia run src/main.ae", name);
            Ok(())
        }
        CliCommand::Build { paths } => commands::build(paths),
        CliCommand::Run { file, args } => commands::run(commands::RunTarget { file, args }),
        CliCommand::Check { paths } => commands::check(paths),
        CliCommand::Fmt { paths } => commands::fmt(paths),
        CliCommand::Lint { paths } => commands::lint(paths),
        CliCommand::Doc { paths, output, html } => commands::doc(paths, output, html),
        CliCommand::Shell => commands::shell(),
        CliCommand::Test { path } => commands::test(&path),
    }
}
