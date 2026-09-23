use super::*;
use crate::{
    alerts::delivery::Outcome,
    runtime::Engine,
    sources::{Adapters, SourceRequest},
};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeSet,
    rc::Rc,
    time::Duration,
};
#[derive(Clone)]
pub struct Worker {
    pub engine: Engine,
    pub providers: Option<String>,
    active: Rc<RefCell<BTreeSet<String>>>,
    last: Rc<Cell<i64>>,
}
impl Worker {
    pub fn new(engine: Engine) -> Self {
        Self {
            engine,
            providers: std::env::var("TALIA_ALERT_PROVIDERS").ok(),
            active: Default::default(),
            last: Rc::new(Cell::new(i64::MIN)),
        }
    }
    pub fn tick(&self) -> Result<()> {
        let now = self.engine.now();
        if now.saturating_sub(self.last.get()) < 1000 {
            return Ok(());
        }
        self.last.set(now);
        let scheduled = self.engine.store.borrow_mut().report_scheduled(now);
        let ids = self.engine.store.borrow().report_active()?;
        for id in ids {
            if self.active.borrow().len() >= 4 {
                break;
            }
            let r = self.engine.store.borrow().report_run(&id)?;
            if now < r.deadline
                && (r.status == "delivering"
                    && r.deliveries.iter().any(|d| d.status == "pending")
                    && !r
                        .deliveries
                        .iter()
                        .any(|d| d.status == "pending" && d.next_at <= now))
            {
                continue;
            }
            if !self.active.borrow_mut().insert(id.clone()) {
                continue;
            }
            let this = self.clone();
            tokio::task::spawn_local(async move {
                if let Err(error) = this.advance(&id).await {
                    if let Ok(mut r) = this.engine.store.borrow().report_run(&id) {
                        r.status = "failed".into();
                        r.error = Some(error);
                        let _ = this.engine.store.borrow().report_put(&r);
                    }
                }
                this.active.borrow_mut().remove(&id);
            });
        }
        scheduled
    }
    pub async fn advance(&self, id: &str) -> Result<()> {
        let mut run = self.engine.store.borrow().report_run(id)?;
        let now = self.engine.now();
        if run.status == "delivering" {
            return self.deliver(&mut run).await;
        }
        if !["queued", "running"].contains(&run.status.as_str()) {
            return Ok(());
        }
        if now >= run.deadline {
            run.status = "failed".into();
            run.error = Some("report deadline exceeded".into());
            return self.engine.store.borrow().report_put(&run);
        }
        run.status = "running".into();
        self.engine.store.borrow().report_put(&run)?;
        if run.index >= run.definition.steps.len() {
            let content: Content =
                serde_json::from_value(evaluate(&run.definition.compose, &run.context())?)
                    .map_err(|_| "invalid composed report")?;
            let (text, html) = email::render(&content, &run)?;
            run.content = Some(content);
            run.text = Some(text);
            run.html = Some(html);
            self.finish(&mut run)?;
            return self.engine.store.borrow().report_put(&run);
        }
        let step = run.definition.steps[run.index].clone();
        let result: Result<Value> = match &step.action {
            Action::Read { variable } => self
                .engine
                .read(
                    variable,
                    Duration::from_millis((run.deadline - now).min(10000) as u64),
                )
                .await
                .and_then(|v| serde_json::to_value(v).map_err(err)),
            Action::Script { source } => evaluate(source, &run.context()),
            Action::Source { source, request } => self.source(source, request, &run).await,
            Action::Analysis {
                instructions,
                inputs,
            } => {
                let selected: BTreeMap<_, _> = inputs
                    .iter()
                    .map(|id| (id.clone(), run.outputs.get(id)))
                    .collect();
                crate::ai::execute(
                    &self.engine,
                    &format!("{}-{}", run.id, step.id),
                    crate::ai::Scope::Report {
                        run: run.id.clone(),
                    },
                    run.deadline,
                    instructions,
                    json!({"period":{"start":run.period_start,"end":run.created},"steps":selected}),
                    false,
                )
                .await
            }
            Action::Unavailable { reason, .. } => Err(reason.clone()),
        };
        let result = result.and_then(|v| {
            if serde_json::to_vec(&v).map_err(err)?.len() > 32768 {
                Err("report step output size limit".into())
            } else {
                Ok(v)
            }
        });
        let output = match result {
            Ok(value) => Output {
                status: "succeeded".into(),
                value,
                error: None,
            },
            Err(error) => Output {
                status: "failed".into(),
                value: Value::Null,
                error: Some(error),
            },
        };
        if output.status == "failed" && !step.optional {
            run.status = "failed".into();
            run.error = Some(format!("required step {} failed", step.id));
        }
        run.outputs.insert(step.id, output);
        run.index += 1;
        self.engine.store.borrow().report_put(&run)
    }
    async fn source(&self, id: &str, script: &str, run: &Run) -> Result<Value> {
        let request: SourceRequest = serde_json::from_value(evaluate(script, &run.context())?)
            .map_err(|_| "invalid report source request")?;
        if request.has_effect() {
            return Err("report checks allow only read-only source requests".into());
        }
        let source = self
            .engine
            .store
            .borrow()
            .monitoring_config()?
            .source(id)?
            .clone();
        let wire = Adapters::new()?.fetch(&source, &request).await?;
        // Explicit wire value preserves undefined/NaN/etc. ctx.decode(value) is provided to scripts.
        Ok(json!({"wire":wire}))
    }
    fn finish(&self, run: &mut Run) -> Result<()> {
        if run.send {
            self.enqueue(run)?;
        } else {
            run.status = if run.outputs.values().any(|s| s.status == "failed") {
                "partial"
            } else {
                "complete"
            }
            .into();
        }
        Ok(())
    }
    pub fn enqueue(&self, run: &mut Run) -> Result<()> {
        if run.content.is_none() {
            return Err("report is not composed".into());
        }
        let destinations = self.engine.store.borrow().alert_destinations()?;
        for id in &run.definition.destinations {
            let d = destinations
                .iter()
                .find(|d| {
                    &d.id == id && d.enabled && ["email", "telegram"].contains(&d.channel.as_str())
                })
                .ok_or("report email or Telegram destination unavailable")?;
            run.deliveries.push(Delivery {
                destination: id.clone(),
                version: d.version,
                status: "pending".into(),
                attempts: 0,
                next_at: 0,
                error: None,
            });
        }
        run.status = "delivering".into();
        run.send = true;
        Ok(())
    }
    async fn deliver(&self, run: &mut Run) -> Result<()> {
        let now = self.engine.now();
        if let Some(index) = run
            .deliveries
            .iter()
            .position(|d| d.status == "pending" && d.next_at <= now)
        {
            let dest = self
                .engine
                .store
                .borrow()
                .alert_destinations()?
                .into_iter()
                .find(|d| {
                    d.id == run.deliveries[index].destination
                        && d.version == run.deliveries[index].version
                        && ["email", "telegram"].contains(&d.channel.as_str())
                        && d.enabled
                });
            let outcome = if now >= run.deadline {
                Outcome::Failed("report delivery deadline exceeded".into())
            } else if let Some(d) = dest {
                if d.channel == "telegram" {
                    if d.provider != crate::telegram::PROVIDER {
                        return Err("Reports require the web-managed Telegram bot".into());
                    }
                    let delivery_id = format!("{}-{}", run.id, d.id);
                    let status = self.engine.store.borrow_mut().telegram_report(
                        &delivery_id,
                        d.target.parse().map_err(|_| "invalid Telegram chat")?,
                        run,
                    )?;
                    if status == "pending" {
                        return Ok(());
                    }
                    run.deliveries[index].status = status;
                    run.deliveries[index].error = if run.deliveries[index].status == "sent" {
                        None
                    } else {
                        Some("Telegram delivery was not confirmed; not automatically resent".into())
                    };
                    self.engine.store.borrow().report_put(run)?;
                    return Ok(());
                }
                let provider = match &self.providers {
                    Some(path) => {
                        crate::alerts::providers::configuration(std::path::Path::new(path))
                            .await
                            .and_then(|mut c| {
                                c.remove(&d.provider)
                                    .ok_or("report SMTP provider missing".into())
                            })
                    }
                    None => Err("TALIA_ALERT_PROVIDERS is not configured".into()),
                };
                match provider {
                    Ok(p) => {
                        run.deliveries[index].status = "sending".into();
                        run.deliveries[index].attempts += 1;
                        self.engine.store.borrow().report_put(run)?;
                        email::send(&p, &d.target, run).await
                    }
                    Err(e) => Outcome::Failed(e),
                }
            } else {
                Outcome::Failed("email or Telegram destination disabled or changed".into())
            };
            let delivery = &mut run.deliveries[index];
            match outcome {
                Outcome::Sent => {
                    delivery.status = "sent".into();
                    delivery.error = None;
                }
                Outcome::Retry(e) | Outcome::RetryAfter(e, _) if delivery.attempts < 3 => {
                    delivery.status = "pending".into();
                    delivery.next_at = now + 30000;
                    delivery.error = Some(e);
                }
                Outcome::Unknown(e) => {
                    delivery.status = "unknown".into();
                    delivery.error = Some(e);
                }
                Outcome::Failed(e) | Outcome::Retry(e) | Outcome::RetryAfter(e, _) => {
                    delivery.status = "failed".into();
                    delivery.error = Some(e);
                }
                _ => {
                    delivery.status = "failed".into();
                    delivery.error = Some("report email was not accepted".into());
                }
            }
        }
        if run
            .deliveries
            .iter()
            .all(|d| d.status != "pending" && d.status != "sending")
        {
            run.status = if run.deliveries.iter().any(|d| d.status != "sent")
                || run.outputs.values().any(|s| s.status == "failed")
            {
                "partial"
            } else {
                "complete"
            }
            .into();
        }
        self.engine.store.borrow().report_put(run)
    }
}
