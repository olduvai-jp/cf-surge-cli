fn main() {
    if let Err(message) = cfsurge::run() {
        eprintln!("{message}");
        std::process::exit(1);
    }
}
