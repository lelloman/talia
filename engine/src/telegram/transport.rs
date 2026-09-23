use super::*;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use ring::{
    aead,
    rand::{SecureRandom, SystemRandom},
};
use std::{io::Write, os::unix::fs::OpenOptionsExt};

pub fn random() -> Result<String> {
    let mut bytes = [0; 32];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| "randomness unavailable")?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
pub fn hash(s: &str) -> String {
    ring::digest::digest(&ring::digest::SHA256, s.as_bytes())
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
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
                        .map_err(|_| "cannot persist Telegram encryption key")?;
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(_) => return Err("cannot create Telegram encryption key".into()),
            }
        }
        use std::io::Read;
        let mut bytes = vec![];
        std::fs::File::open(path)
            .map_err(|_| "Telegram encryption key unavailable")?
            .take(33)
            .read_to_end(&mut bytes)
            .map_err(|_| "Telegram key unreadable")?;
        Ok(bytes)
    })
    .await
    .map_err(|_| "Telegram key task failed")??;
    Ok(aead::LessSafeKey::new(
        aead::UnboundKey::new(&aead::AES_256_GCM, &bytes).map_err(|_| "invalid Telegram key")?,
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
        aead::Aad::from(b"talia-telegram-v1"),
        &mut bytes,
    )
    .map_err(|_| "cannot encrypt token")?;
    Ok(STANDARD.encode([nonce.as_slice(), bytes.as_slice()].concat()))
}
fn unseal(key: &aead::LessSafeKey, encrypted: &str) -> Result<String> {
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
            aead::Aad::from(b"talia-telegram-v1"),
            &mut bytes[12..],
        )
        .map_err(|_| "cannot decrypt Telegram token")?;
    String::from_utf8(plain.to_vec()).map_err(|_| "invalid token".into())
}
#[derive(Clone)]
pub struct Bot {
    pub token: String,
    pub base: String,
}
impl Bot {
    pub async fn load(engine: &Engine) -> Result<(Config, Self)> {
        let (config, path) = {
            let s = engine.store.borrow();
            (s.telegram_config()?, s.telegram_key_path()?)
        };
        if !config.enabled {
            return Err("Telegram is disabled".into());
        }
        let token = unseal(&key(path, false).await?, &config.token)?;
        Ok((
            config,
            Self {
                token,
                base: base(),
            },
        ))
    }
    pub async fn call(&self, method: &str, args: Value) -> Result<Value> {
        if self.token.is_empty()
            || self.token.len() > 256
            || !self
                .token
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b":_-".contains(&c))
        {
            return Err("invalid bot token".into());
        }
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(12))
            .build()
            .map_err(|_| "Telegram client unavailable")?;
        let mut response = client
            .post(format!("{}/bot{}/{method}", self.base, self.token))
            .json(&args)
            .send()
            .await
            .map_err(|_| "Telegram connection failed; outcome may be unknown")?;
        let status = response.status();
        let mut bytes = vec![];
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| "Telegram response interrupted")?
        {
            if bytes.len() + chunk.len() > 262144 {
                return Err("Telegram response too large".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        let body: Value =
            serde_json::from_slice(&bytes).map_err(|_| "invalid Telegram response")?;
        if !status.is_success() || body["ok"] != true {
            return Err(format!(
                "Telegram rejected request (HTTP {})",
                status.as_u16()
            ));
        }
        Ok(body["result"].clone())
    }
}
pub fn base() -> String {
    // Production endpoints cannot be redirected by web configuration.
    #[cfg(test)]
    if let Some(url) = TEST_BASE.with(|v| v.borrow().clone()) {
        return url;
    }
    "https://api.telegram.org".into()
}
#[cfg(test)]
thread_local! {pub static TEST_BASE:std::cell::RefCell<Option<String>>=const {std::cell::RefCell::new(None)};}
