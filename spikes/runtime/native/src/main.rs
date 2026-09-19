fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("--transport") => {
            let result = talia_runtime_spike::transport::run(
                std::env::args().nth(2).unwrap().parse().unwrap(),
            );
            println!("{}", result);
            if result.get("error").is_some() {
                std::process::exit(1);
            }
        }
        Some("--process-worker") => talia_runtime_spike::process::worker(),
        Some("--process-check") => println!("{}", talia_runtime_spike::process::check()),
        _ => println!("{}", talia_runtime_spike::run()),
    }
}
