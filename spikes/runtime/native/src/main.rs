fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("--process-worker") => talia_runtime_spike::process::worker(),
        Some("--process-check") => println!("{}", talia_runtime_spike::process::check()),
        _ => println!("{}", talia_runtime_spike::run()),
    }
}
