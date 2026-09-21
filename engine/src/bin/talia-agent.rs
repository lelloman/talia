//! Offline operator credential/policy setup. Never exposed through MCP.
use serde::Deserialize;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use talia_engine::{authority::Grant, store::Store};
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Policy {
    principal: String,
    expected_version: u64,
    enabled: bool,
    grants: Vec<Grant>,
    ceiling: Option<Vec<Grant>>,
}
fn main() -> Result<(), String> {
    let a: Vec<_> = std::env::args().collect();
    if a.len() != 4 {
        return Err("usage: talia-agent DATABASE POLICY_JSON CREDENTIAL_OUTPUT | talia-agent DATABASE POLICY_JSON --policy-only".into());
    }
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(format!("{}.lock", a[1]))
        .map_err(|_| "cannot open database lock")?;
    lock.try_lock()
        .map_err(|_| "stop the engine before provisioning credentials")?;
    let policy: Policy =
        serde_json::from_slice(&std::fs::read(&a[2]).map_err(|_| "cannot read policy")?)
            .map_err(|_| "invalid policy JSON")?;
    let mut output = if a[3] == "--policy-only" {
        None
    } else {
        Some(
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&a[3])
                .map_err(|_| "credential output must be a new writable file")?,
        )
    };
    let mut store = Store::open(&a[1]).map_err(|_| "cannot open database")?;
    let token = store
        .agent_provision(
            &policy.principal,
            policy.expected_version,
            policy.enabled,
            &policy.grants,
            policy.ceiling.as_deref(),
            output.is_some(),
        )
        .map_err(|e| format!("provisioning failed: {e:?}"))?;
    if let (Some(file), Some(token)) = (&mut output, token) {
        if file
            .write_all(token.as_bytes())
            .and_then(|_| file.sync_all())
            .is_err()
        {
            store.agent_credential_revoke(&token).map_err(|_|"credential write and revocation failed; disable this principal before starting engine")?;
            return Err("credential write failed; token revoked, policy was saved".into());
        }
    }
    println!("policy saved at version {}", policy.expected_version + 1);
    Ok(())
}
