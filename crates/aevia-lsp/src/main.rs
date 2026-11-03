//! The `aevia-lsp` binary: a language server over standard input and output.

use std::process::ExitCode;

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if arguments
        .iter()
        .any(|argument| argument == "--version" || argument == "-V")
    {
        println!("aevia-lsp {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    if arguments
        .iter()
        .any(|argument| argument == "--help" || argument == "-h")
    {
        println!(
            "aevia-lsp {}\n\n\
             A language server for Aevia, speaking LSP 3.17 over standard input\n\
             and output.\n\n\
             Usage: aevia-lsp [--stdio]\n\n\
             `--stdio` is the only transport and is the default; it is accepted\n\
             because clients pass it.\n",
            env!("CARGO_PKG_VERSION")
        );
        return ExitCode::SUCCESS;
    }
    // Any other argument is a transport this server does not implement. Saying
    // so is better than starting and then hanging.
    if let Some(unknown) = arguments
        .iter()
        .find(|argument| argument.as_str() != "--stdio")
    {
        eprintln!("aevia-lsp: `{unknown}` is not an option; the only transport is --stdio");
        return ExitCode::from(2);
    }

    match aevia_lsp::serve_stdio() {
        aevia_lsp::Exit::Graceful => ExitCode::SUCCESS,
        aevia_lsp::Exit::Ungraceful => ExitCode::from(1),
    }
}
