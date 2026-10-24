#[cfg(any(windows, test))]
#[path = "../src/identity_poc/mod.rs"]
mod identity_poc;

fn main() {
    #[cfg(windows)]
    if let Err(error) = identity_poc::windows::run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
    #[cfg(not(windows))]
    {
        eprintln!("Unsupported configuration: native identity experiment requires Windows");
        std::process::exit(1);
    }
}
