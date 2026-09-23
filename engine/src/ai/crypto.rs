use crate::store::Result;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use ring::{
    aead,
    rand::{SecureRandom, SystemRandom},
};
use std::{io::Write, os::unix::fs::OpenOptionsExt};
pub async fn key(path: String, create: bool) -> Result<aead::LessSafeKey> {
    let bytes = tokio::task::spawn_blocking(move || -> Result<Vec<u8>> {
        if create && !std::path::Path::new(&path).exists() {
            let mut bytes = [0; 32];
            SystemRandom::new()
                .fill(&mut bytes)
                .map_err(|_| "randomness unavailable")?;
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)
            {
                Ok(mut f) => {
                    f.write_all(&bytes)
                        .and_then(|_| f.sync_all())
                        .map_err(|_| "cannot persist AI encryption key")?;
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(_) => return Err("cannot create AI encryption key".into()),
            }
        }
        use std::io::Read;
        let mut bytes = vec![];
        std::fs::File::open(path)
            .map_err(|_| "AI encryption key unavailable")?
            .take(33)
            .read_to_end(&mut bytes)
            .map_err(|_| "AI key unreadable")?;
        Ok(bytes)
    })
    .await
    .map_err(|_| "AI key task failed")??;
    Ok(aead::LessSafeKey::new(
        aead::UnboundKey::new(&aead::AES_256_GCM, &bytes).map_err(|_| "invalid AI key")?,
    ))
}
pub fn seal(key: &aead::LessSafeKey, token: &str) -> Result<String> {
    let mut nonce = [0; 12];
    SystemRandom::new()
        .fill(&mut nonce)
        .map_err(|_| "randomness unavailable")?;
    let mut bytes = token.as_bytes().to_vec();
    key.seal_in_place_append_tag(
        aead::Nonce::assume_unique_for_key(nonce),
        aead::Aad::from(b"talia-ai-account-v1"),
        &mut bytes,
    )
    .map_err(|_| "cannot encrypt token")?;
    Ok(STANDARD.encode([nonce.as_slice(), bytes.as_slice()].concat()))
}
pub fn unseal(key: &aead::LessSafeKey, encrypted: &str) -> Result<String> {
    let mut bytes = STANDARD
        .decode(encrypted)
        .map_err(|_| "invalid encrypted token")?;
    if bytes.len() < 28 {
        return Err("invalid encrypted token".into());
    }
    let nonce = bytes[..12].try_into().unwrap();
    let plain = key
        .open_in_place(
            aead::Nonce::assume_unique_for_key(nonce),
            aead::Aad::from(b"talia-ai-account-v1"),
            &mut bytes[12..],
        )
        .map_err(|_| "cannot decrypt AI token")?;
    String::from_utf8(plain.to_vec()).map_err(|_| "invalid token".into())
}
