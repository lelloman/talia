use super::*;
use crate::alerts::{delivery::Outcome, providers::Provider};
use lettre::{
    message::{MultiPart, SinglePart},
    transport::smtp::authentication::Credentials,
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
};
use std::time::Duration;
fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
pub fn render(content: &Content, run: &Run) -> Result<(String, String)> {
    if content.subject.trim().is_empty()
        || content.subject.len() > 200
        || content.subject.chars().any(char::is_control)
        || content.sections.len() > 32
        || serde_json::to_vec(content).map_err(err)?.len() > 32768
    {
        return Err("report composition bounds".into());
    }
    let mut text = format!("{}\n\n{}\n", content.subject, content.summary);
    let mut html=format!("<!doctype html><html><body style=\"font-family:Arial,sans-serif;color:#17243a;background:#f4f7fb;padding:24px\"><main style=\"max-width:760px;margin:auto;background:white;padding:28px;border-radius:12px\"><p style=\"color:#506780\">TALÌA · MONITORING REPORT</p><h1>{}</h1><p style=\"white-space:pre-wrap\">{}</p>",escape(&content.subject),escape(&content.summary));
    for section in &content.sections {
        text += &format!("\n{}\n{}\n", section.title, section.text);
        html+=&format!("<section style=\"border-top:1px solid #dbe3ee;margin-top:24px\"><h2>{}</h2><div style=\"white-space:pre-wrap;overflow-wrap:anywhere\">{}</div></section>",escape(&section.title),escape(&section.text));
    }
    for (id, output) in &run.outputs {
        if output.status == "failed" {
            let warning = format!(
                "Incomplete step {id}: {}",
                output.error.as_deref().unwrap_or("failed")
            );
            text += &format!("\n{warning}\n");
            html += &format!(
                "<p style=\"background:#fff0dc;padding:12px\">{}</p>",
                escape(&warning)
            );
        }
    }
    let date = |ms| {
        chrono::DateTime::from_timestamp_millis(ms)
            .map(|t| t.to_rfc3339())
            .unwrap_or_else(|| ms.to_string())
    };
    let period = format!(
        "Period: {} to {}. Run: {}",
        date(run.period_start),
        date(run.created),
        run.id
    );
    text += &format!("\n{period}\n");
    html += &format!(
        "<footer style=\"margin-top:24px;color:#506780\">{}</footer></main></body></html>",
        escape(&period)
    );
    Ok((text, html))
}
pub async fn send(provider: &Provider, to: &str, run: &Run) -> Outcome {
    let Provider::Smtp {
        host,
        port,
        tls,
        from,
        username,
        password,
    } = provider
    else {
        return Outcome::Failed("report requires SMTP provider".into());
    };
    let builder = match tls.as_str() {
        "tls" => AsyncSmtpTransport::<Tokio1Executor>::relay(host),
        "starttls" => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(host),
        "none_loopback"
            if host == "localhost"
                || host
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback()) =>
        {
            Ok(AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(
                host,
            ))
        }
        _ => return Outcome::Failed("invalid SMTP TLS configuration".into()),
    };
    let Ok(mut builder) = builder else {
        return Outcome::Failed("invalid SMTP provider".into());
    };
    builder = builder.port(*port).timeout(Some(Duration::from_secs(10)));
    match (username, password) {
        (Some(u), Some(p)) => builder = builder.credentials(Credentials::new(u.clone(), p.clone())),
        (None, None) => (),
        _ => return Outcome::Failed("incomplete SMTP credentials".into()),
    }
    let (Ok(from), Ok(to)) = (from.parse(), to.parse()) else {
        return Outcome::Failed("invalid email address".into());
    };
    let message = Message::builder()
        .from(from)
        .to(to)
        .message_id(Some(format!("{}@talia", run.id)))
        .subject(&run.content.as_ref().unwrap().subject)
        .multipart(
            MultiPart::alternative()
                .singlepart(SinglePart::plain(run.text.clone().unwrap()))
                .singlepart(SinglePart::html(run.html.clone().unwrap())),
        );
    let Ok(message) = message else {
        return Outcome::Failed("invalid report email".into());
    };
    match tokio::time::timeout(Duration::from_secs(15), builder.build().send(message)).await {
        Ok(Ok(_)) => Outcome::Sent,
        Ok(Err(e)) if e.is_permanent() => Outcome::Failed("SMTP rejected report".into()),
        Ok(Err(e)) if e.is_transient() => Outcome::Retry("SMTP temporarily rejected report".into()),
        _ => Outcome::Unknown("report email acceptance unknown; not automatically retried".into()),
    }
}
