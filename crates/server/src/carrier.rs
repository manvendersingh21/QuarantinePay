//! Simulated carrier. Its signing key is separate from the gateway key and from any seller, so a
//! seller cannot fabricate proof of delivery. In production this would be a carrier's webhook or
//! signed tracking API; here it is clearly labelled as simulated.
use anyhow::Result;
use ed25519_dalek::SigningKey;
use std::path::PathBuf;

pub fn key() -> Result<SigningKey> {
    let path = PathBuf::from(".local").join("carrier-key");
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            use rand::RngCore;
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            let mut b = vec![0u8; 32];
            rand::thread_rng().fill_bytes(&mut b);
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)?;
            f.write_all(&b)?;
            b
        }
        Err(e) => return Err(e.into()),
    };
    let arr: [u8; 32] = bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("invalid carrier key length"))?;
    Ok(SigningKey::from_bytes(&arr))
}
