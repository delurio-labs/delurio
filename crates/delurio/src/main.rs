//! `delurio`: the command line (SPEC.md §10).

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(delurio::run(&args));
}
