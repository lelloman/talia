//! SSH transport for the host-enforced diagnostic shell. Never invokes a local shell.
use super::*;
use std::{collections::BTreeMap, process::Stdio};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Host {
    address: String,
    user: String,
    key_file: String,
    known_hosts: String,
    #[serde(default = "default_port")]
    port: u16,
}
fn default_port() -> u16 {
    22
}
#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Hosts {
    hosts: BTreeMap<String, Host>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    host: String,
    command: String,
}

fn valid_word(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 253
        && !s.starts_with('-')
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-:".contains(&b))
}
fn validate(host: &Host) -> Result<()> {
    if !valid_word(&host.address)
        || !valid_word(&host.user)
        || host.port == 0
        || [&host.key_file, &host.known_hosts]
            .iter()
            .any(|s| !s.starts_with('/') || s.contains(['\n', '\r', '\0']))
    {
        return Err("invalid host-shell configuration".into());
    }
    Ok(())
}
async fn configuration() -> Result<Hosts> {
    let path = std::env::var("TALIA_HOST_SHELL_CONFIG")
        .unwrap_or_else(|_| "/run/talia/host-shell.json".into());
    let config: Hosts = serde_json::from_str(
        &file(path, 16384)
            .await
            .map_err(|_| "Host diagnostics are not configured")?,
    )
    .map_err(|_| "invalid host-shell configuration")?;
    if config.hosts.len() > 8 || config.hosts.keys().any(|id| !valid_word(id)) {
        return Err("invalid host-shell inventory".into());
    }
    for host in config.hosts.values() {
        validate(host)?;
    }
    Ok(config)
}
fn arguments(host: &Host) -> Vec<String> {
    vec![
        "-F".into(),
        "/dev/null".into(),
        "-T".into(),
        "-oBatchMode=yes".into(),
        "-oIdentitiesOnly=yes".into(),
        "-oIdentityAgent=none".into(),
        "-oStrictHostKeyChecking=yes".into(),
        "-oGlobalKnownHostsFile=/dev/null".into(),
        format!("-oUserKnownHostsFile={}", host.known_hosts),
        "-oClearAllForwardings=yes".into(),
        "-oPermitLocalCommand=no".into(),
        "-oProxyCommand=none".into(),
        "-oProxyJump=none".into(),
        "-oConnectTimeout=5".into(),
        "-oConnectionAttempts=1".into(),
        "-oServerAliveInterval=5".into(),
        "-oServerAliveCountMax=1".into(),
        "-i".into(),
        host.key_file.clone(),
        "-p".into(),
        host.port.to_string(),
        "-l".into(),
        host.user.clone(),
        "--".into(),
        host.address.clone(),
        "talia-diagnostics".into(),
    ]
}
pub async fn execute(args: Value) -> Result<Value> {
    let request: Request =
        serde_json::from_value(args).map_err(|_| "host_exec requires host and command")?;
    if request.command.is_empty() || request.command.len() > 4096 || request.command.contains('\0')
    {
        return Err("host_exec command must contain 1–4096 UTF-8 bytes".into());
    }
    let config = configuration().await?;
    let host = config.hosts.get(&request.host).ok_or_else(|| {
        format!(
            "Host is not enabled. Available hosts: {}",
            config.hosts.keys().cloned().collect::<Vec<_>>().join(", ")
        )
    })?;
    let id = crate::mcp_engine::random_id().map_err(|_| "request identity unavailable")?;
    let body = json!({"version":1,"request_id":id,"command":request.command}).to_string();
    let mut child = tokio::process::Command::new("/usr/bin/ssh")
        .args(arguments(host))
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LANG", "C.UTF-8")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| "host diagnostic transport unavailable")?;
    let work = async {
        let mut input = child.stdin.take().ok_or("diagnostic stdin unavailable")?;
        input
            .write_all(body.as_bytes())
            .await
            .map_err(|_| "diagnostic request failed")?;
        input
            .shutdown()
            .await
            .map_err(|_| "diagnostic request failed")?;
        drop(input);
        let mut output = child
            .stdout
            .take()
            .ok_or("diagnostic stdout unavailable")?
            .take(131073);
        let mut errors = child
            .stderr
            .take()
            .ok_or("diagnostic stderr unavailable")?
            .take(8193);
        let (mut out, mut err) = (Vec::new(), Vec::new());
        tokio::try_join!(output.read_to_end(&mut out), errors.read_to_end(&mut err))
            .map_err(|_| "diagnostic response failed")?;
        if out.len() > 131072 || err.len() > 8192 {
            return Err("diagnostic response exceeded limit".into());
        }
        let status = child
            .wait()
            .await
            .map_err(|_| "diagnostic transport failed")?;
        // SSH diagnostics can contain operator paths; do not expose stderr.
        if !status.success() {
            return Err("Host diagnostics unavailable: SSH authentication, host verification or connection failed".into());
        }
        let result: Value =
            serde_json::from_slice(&out).map_err(|_| "invalid diagnostic response")?;
        if result["version"] != 1
            || result["request_id"] != id
            || result["host"] != request.host
            || !result["exit_code"].is_i64()
            || !result["stdout"].is_string()
            || !result["stderr"].is_string()
            || !result["truncated"].is_boolean()
            || serde_json::to_vec(&result).map_err(super::err)?.len() > 32768
        {
            return Err("invalid or oversized diagnostic response".into());
        }
        Ok(result)
    };
    let result = match tokio::time::timeout(Duration::from_secs(20), work).await {
        Ok(result) => result,
        Err(_) => Err("Host diagnostic timed out; remote execution is also time limited".into()),
    };
    if result.is_err() {
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
    // Changes/revocation while SSH was in flight invalidate the returned evidence.
    if configuration().await? != config {
        return Err("Host diagnostic permissions changed during the request".into());
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn host() -> Host {
        Host {
            address: "192.0.2.1".into(),
            user: "talia-diagnostics".into(),
            key_file: "/private/key".into(),
            known_hosts: "/private/known_hosts".into(),
            port: 22,
        }
    }
    #[test]
    fn transport_never_accepts_command_text_in_ssh_arguments() {
        let h = host();
        validate(&h).unwrap();
        let args = arguments(&h);
        assert_eq!(args.last().unwrap(), "talia-diagnostics");
        assert!(args.iter().any(|a| a == "-oStrictHostKeyChecking=yes"));
        assert!(args.iter().any(|a| a == "-oClearAllForwardings=yes"));
        let mut bad = h.clone();
        bad.address = "-oProxyCommand=sh".into();
        assert!(validate(&bad).is_err());
        bad = h;
        bad.user = "root; id".into();
        assert!(validate(&bad).is_err());
    }
    #[tokio::test]
    #[ignore = "requires provisioned hosts and TALIA_HOST_SHELL_CONFIG"]
    async fn provisioned_hosts_enforce_read_only_diagnostics() {
        for host in ["homelab", "vps-eu", "vps-us"] {
            let result = execute(json!({"host":host,"command":"hostname"}))
                .await
                .unwrap();
            assert_eq!(result["exit_code"], 0, "{host}: {result}");
            assert!(!result["stdout"].as_str().unwrap().is_empty());
            for command in [
                "cat /etc/shadow",
                "hostname; id",
                "find /var/log -exec id ;",
            ] {
                let result = execute(json!({"host":host,"command":command}))
                    .await
                    .unwrap();
                assert_eq!(result["exit_code"], 126, "{host}: {result}");
                assert_eq!(result["stdout"], "");
            }
        }
    }
    #[tokio::test]
    async fn reject_invalid_requests_before_any_connection() {
        assert!(execute(json!({"host":"x","command":"id","extra":true}))
            .await
            .is_err());
        assert!(execute(json!({"host":"x","command":"x".repeat(4097)}))
            .await
            .is_err());
        assert!(execute(json!({"host":"x","command":"\u{0000}"}))
            .await
            .is_err());
    }
}
