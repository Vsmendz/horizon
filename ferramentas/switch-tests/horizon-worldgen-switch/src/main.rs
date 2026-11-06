use std::path::PathBuf;

fn main() {
    let root = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .expect("prepared resource directory"),
    );
    if let Err(error) = horizon_switch_worldgen::run(&root, &|line| println!("{line}")) {
        eprintln!("FAIL {error:#}");
        std::process::exit(1);
    }
}
