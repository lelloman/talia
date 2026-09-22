fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = std::env::args()
        .nth(1)
        .ok_or("supply an output directory")?;
    std::fs::create_dir_all(&dir)?;
    for (name, schema) in simple_agents_protocol::schemas() {
        std::fs::write(
            std::path::Path::new(&dir).join(format!("{name}.schema.json")),
            format!("{}\n", serde_json::to_string_pretty(&schema)?),
        )?;
    }
    Ok(())
}
