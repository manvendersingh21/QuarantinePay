use std::io::{Read, Write};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut bytes = Vec::new();
    std::io::stdin().take(65537).read_to_end(&mut bytes)?;
    let verdict = quarantinepay_core::verify_invoice(&bytes);
    std::io::stdout().write_all(&serde_json::to_vec(&verdict)?)?;
    Ok(())
}
