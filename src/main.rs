fn main() {
    if let Err(err) = aevia::run() {
        aevia::diagnostics::emit(err);
        std::process::exit(1);
    }
}
