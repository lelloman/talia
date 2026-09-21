use super::{policy::*, *};
use ring::digest;
fn yes() -> bool {
    true
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Destination {
    pub id: String,
    pub version: u64,
    pub channel: String,
    pub provider: String,
    pub target: String,
    #[serde(default = "yes")]
    pub enabled: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Device {
    pub id: String,
    pub version: u64,
    #[serde(default)]
    pub owner: String,
    pub token: String,
    #[serde(default)]
    pub groups: Vec<String>,
    #[serde(default = "yes")]
    pub enabled: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Delivery {
    pub id: String,
    pub slot: String,
    pub binding: String,
    pub binding_version: u64,
    pub policy_version: u64,
    pub key: String,
    pub occurrence: u64,
    pub stage: String,
    pub stage_at: i64,
    pub active: bool,
    pub action: ResponseAction,
    pub destination: String,
    pub destination_version: u64,
    pub device: Option<String>,
    pub status: String,
    pub attempts: u32,
    pub due: i64,
    pub expires: i64,
    pub updated_at: i64,
    pub error: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Slot {
    next: i64,
    sequence: u64,
}
#[derive(Clone, Debug)]
pub struct Dispatch {
    pub delivery: Delivery,
    pub destination: Destination,
    pub address: String,
    pub alert: Alert,
}
#[derive(Clone, Debug)]
pub enum Outcome {
    Sent,
    Retry(String),
    RetryAfter(String, u64),
    Failed(String),
    Unknown(String),
}
fn digest_id(s: &str) -> String {
    digest::digest(&digest::SHA256, s.as_bytes())
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
impl Store {
    pub fn alert_destinations(&self) -> Result<Vec<Destination>> {
        self.alert_list("destination")
    }
    pub fn alert_destination_save(
        &mut self,
        d: &Destination,
        expected: u64,
        actor: &str,
        now: i64,
    ) -> Result<()> {
        key(&d.id)?;
        key(&d.provider)?;
        key(&d.target)?;
        if d.version != expected + 1 || !["email", "telegram", "push"].contains(&d.channel.as_str())
        {
            return Err("invalid destination".into());
        }
        if d.channel == "push"
            && !d.target.starts_with("device:")
            && !d.target.starts_with("user:")
            && !d.target.starts_with("group:")
        {
            return Err("push target must select device, user or group".into());
        }
        self.alert_atomic(|s| {
            if s.alert_get::<Destination>("destination", &d.id)?
                .map_or(0, |v| v.version)
                != expected
            {
                return Err("conflict".into());
            }
            s.alert_put("destination", &d.id, d)?;
            s.alert_audit(
                actor,
                "destination_saved",
                &d.id,
                now,
                json!({"version":d.version}),
            )
        })
    }
    /// Caller provides an authenticated owner, never a payload-derived identity.
    pub fn alert_device_register(
        &mut self,
        d: &Device,
        expected: u64,
        owner: &str,
        now: i64,
    ) -> Result<()> {
        key(&d.id)?;
        key(owner)?;
        if d.version != expected + 1
            || d.token.is_empty()
            || d.token.len() > 4096
            || d.groups.len() > 32
        {
            return Err("invalid device".into());
        }
        self.alert_atomic(|s| {
            let old = s.alert_get::<Device>("device", &d.id)?;
            if old.as_ref().is_some_and(|v| v.owner != owner) {
                return Err("forbidden".into());
            }
            if old.as_ref().map_or(0, |v| v.version) != expected {
                return Err("conflict".into());
            }
            let mut d = d.clone();
            d.groups = old.map_or_else(Vec::new, |v| v.groups);
            d.owner = owner.into();
            s.alert_put("device", &d.id, &d)?;
            s.alert_audit(
                owner,
                "device_registered",
                &d.id,
                now,
                json!({"version":d.version,"enabled":d.enabled}),
            )
        })
    }
    pub fn alert_device_groups(
        &mut self,
        id: &str,
        expected: u64,
        groups: Vec<String>,
        actor: &str,
        now: i64,
    ) -> Result<()> {
        if groups.len() > 32 {
            return Err("group capacity".into());
        }
        for g in &groups {
            key(g)?;
        }
        self.alert_atomic(|s| {
            let mut d = s
                .alert_get::<Device>("device", id)?
                .ok_or("device missing")?;
            if d.version != expected {
                return Err("conflict".into());
            }
            d.groups = groups;
            d.version += 1;
            s.alert_put("device", id, &d)?;
            s.alert_audit(
                actor,
                "device_groups",
                id,
                now,
                json!({"version":d.version}),
            )
        })
    }
    pub fn alert_devices_public(&self) -> Result<Vec<Value>> {
        Ok(self.alert_list::<Device>("device")?.into_iter().map(|d|json!({"id":d.id,"version":d.version,"owner":d.owner,"groups":d.groups,"enabled":d.enabled})).collect())
    }
    pub fn alert_deliveries(&self) -> Result<Vec<Delivery>> {
        self.alert_list("delivery")
    }
    fn destination_targets(&self, d: &Destination) -> Result<Vec<Option<String>>> {
        if d.channel != "push" {
            return Ok(vec![None]);
        }
        Ok(self
            .alert_list::<Device>("device")?
            .into_iter()
            .filter(|v| {
                v.enabled
                    && (d.target == format!("device:{}", v.id)
                        || d.target == format!("user:{}", v.owner)
                        || d.target
                            .strip_prefix("group:")
                            .is_some_and(|g| v.groups.iter().any(|x| x == g)))
            })
            .map(|d| Some(d.id))
            .collect())
    }
    fn delivery_waiting(&self, j: &Delivery, now: i64) -> Result<bool> {
        if now >= j.expires {
            return Ok(false);
        }
        let Some(b) = self.alert_get::<Binding>("binding", &j.binding)? else {
            return Ok(false);
        };
        let Some(p) = self.alert_get::<Policy>("policy", &b.policy)? else {
            return Ok(false);
        };
        let e = self
            .alert_get::<Evaluation>("evaluation", &b.id)?
            .unwrap_or_default();
        Ok(b.version == j.binding_version
            && p.version == j.policy_version
            && (e.error.is_some()
                || e.last_at.is_none()
                || e.binding_version != b.version
                || e.policy_version != p.version
                || self.alert_silenced(&self.alert(&j.key)?, now)?))
    }
    fn delivery_valid(&self, j: &Delivery, now: i64) -> Result<bool> {
        let Some(b) = self.alert_get::<Binding>("binding", &j.binding)? else {
            return Ok(false);
        };
        let Some(p) = self.alert_get::<Policy>("policy", &b.policy)? else {
            return Ok(false);
        };
        let Some(a) = self.alert_get::<Alert>("alert", &j.key)? else {
            return Ok(false);
        };
        let Some(d) = self.alert_get::<Destination>("destination", &j.destination)? else {
            return Ok(false);
        };
        let e = self
            .alert_get::<Evaluation>("evaluation", &b.id)?
            .unwrap_or_default();
        let actions = if a.active {
            &p.stages.get(&a.stage).ok_or("stage missing")?.actions
        } else {
            &p.recovery
        };
        Ok(b.enabled
            && b.version == j.binding_version
            && p.version == j.policy_version
            && d.enabled
            && d.version == j.destination_version
            && a.occurrence == j.occurrence
            && a.active == j.active
            && a.stage == j.stage
            && a.stage_at == j.stage_at
            && (!a.active || !j.action.until_ack || a.acknowledgement.is_none())
            && e.error.is_none()
            && e.last_at.is_some()
            && e.binding_version == b.version
            && e.policy_version == p.version
            && now < j.expires
            && !self.alert_silenced(&a, now)?
            && actions.iter().any(|v| v.id == j.action.id)
            && e.actions.as_ref().is_none_or(|v| v.contains(&j.action.id))
            && self.destination_targets(&d)?.contains(&j.device))
    }
    pub fn alert_schedule(&mut self, now: i64) -> Result<()> {
        self.alert_atomic(|s| {
            // Retain in-flight outcomes even if superseded. External effects cannot be undone.
            for mut job in s
                .alert_deliveries()?
                .into_iter()
                .filter(|d| d.status == "pending")
            {
                if s.delivery_waiting(&job, now)? {
                    continue;
                }
                if !s.delivery_valid(&job, now)? {
                    job.status = "skipped".into();
                    job.error = Some("no longer applicable".into());
                    job.updated_at = now;
                    s.alert_put("delivery", &job.id, &job)?;
                }
            }
            for b in s.alert_bindings()?.into_iter().filter(|b| b.enabled) {
                let e = s
                    .alert_get::<Evaluation>("evaluation", &b.id)?
                    .unwrap_or_default();
                let p = s
                    .alert_get::<Policy>("policy", &b.policy)?
                    .ok_or("policy missing")?;
                if e.error.is_some()
                    || e.last_at.is_none()
                    || e.binding_version != b.version
                    || e.policy_version != p.version
                {
                    continue;
                }
                let Some(a) = s.alert_get::<Alert>("alert", &b.key)? else {
                    continue;
                };
                let actions = if a.active {
                    &p.stages.get(&a.stage).ok_or("stage missing")?.actions
                } else {
                    &p.recovery
                };
                for action in actions {
                    if a.active && action.until_ack && a.acknowledgement.is_some()
                        || s.alert_silenced(&a, now)?
                        || e.actions.as_ref().is_some_and(|v| !v.contains(&action.id))
                    {
                        continue;
                    }
                    for destination in &action.destinations {
                        let Some(d) = s.alert_get::<Destination>("destination", destination)?
                        else {
                            s.alert_put("delivery_error",&format!("{}:{destination}",b.id),&json!({"id":b.id,"error":format!("destination missing: {destination}")}))?;
                            continue;
                        };
                        s.conn.execute("DELETE FROM alert_entities WHERE kind='delivery_error' AND id=?",[format!("{}:{destination}",b.id)]).map_err(err)?;
                        if !d.enabled {
                            continue;
                        }
                        for device in s.destination_targets(&d)? {
                            let id = digest_id(
                                &json!([
                                    b.id,
                                    b.version,
                                    p.version,
                                    a.occurrence,
                                    a.stage,
                                    a.stage_at,
                                    a.active,
                                    action.id,
                                    d.id,
                                    d.version,
                                    device
                                ])
                                .to_string(),
                            );
                            let mut slot =
                                s.alert_get::<Slot>("delivery_slot", &id)?.unwrap_or(Slot {
                                    next: a
                                        .resolved_at
                                        .filter(|_| !a.active)
                                        .unwrap_or(a.stage_at)
                                        .saturating_add(action.delay_ms as i64),
                                    sequence: 0,
                                });
                            if slot.next > now {
                                continue;
                            }
                            if s.alert_deliveries()?.iter().any(|j| {
                                j.slot == id && (j.status == "pending" || j.status == "sending")
                            }) {
                                continue;
                            }
                            slot.sequence += 1;
                            slot.next = if a.active {
                                action
                                    .repeat_ms
                                    .map_or(i64::MAX, |n| now.saturating_add(n as i64))
                            } else {
                                i64::MAX
                            };
                            let job = Delivery {
                                id: format!("{}-{}", id, slot.sequence),
                                slot: id.clone(),
                                binding: b.id.clone(),
                                binding_version: b.version,
                                policy_version: p.version,
                                key: a.key.clone(),
                                occurrence: a.occurrence,
                                stage: a.stage.clone(),
                                stage_at: a.stage_at,
                                active: a.active,
                                action: action.clone(),
                                destination: d.id.clone(),
                                destination_version: d.version,
                                device,
                                status: "pending".into(),
                                attempts: 0,
                                due: now,
                                expires: now.saturating_add(action.expiry_ms as i64),
                                updated_at: now,
                                error: None,
                            };
                            s.alert_put("delivery_slot", &id, &slot)?;
                            s.alert_put("delivery", &job.id, &job)?;
                        }
                    }
                }
            }
            Ok(())
        })
    }
    pub fn alert_claim_delivery(&mut self, id: &str, now: i64) -> Result<Option<Dispatch>> {
        self.alert_atomic(|s| {
            let Some(mut j) = s.alert_get::<Delivery>("delivery", id)? else {
                return Ok(None);
            };
            if j.status != "pending" || j.due > now {
                return Ok(None);
            }
            if s.delivery_waiting(&j, now)? {
                return Ok(None);
            }
            if !s.delivery_valid(&j, now)? {
                j.status = "skipped".into();
                j.updated_at = now;
                s.alert_put("delivery", id, &j)?;
                return Ok(None);
            }
            let d = s
                .alert_get::<Destination>("destination", &j.destination)?
                .ok_or("destination missing")?;
            let address = if let Some(device) = &j.device {
                s.alert_get::<Device>("device", device)?
                    .ok_or("device missing")?
                    .token
            } else {
                d.target.clone()
            };
            if let Some(repeat) = j.action.repeat_ms.filter(|_| j.active) {
                if let Some(mut slot) = s.alert_get::<Slot>("delivery_slot", &j.slot)? {
                    slot.next = slot.next.max(now.saturating_add(repeat as i64));
                    s.alert_put("delivery_slot", &j.slot, &slot)?;
                }
            }
            j.status = "sending".into();
            j.attempts += 1;
            j.updated_at = now;
            s.alert_put("delivery", id, &j)?;
            Ok(Some(Dispatch {
                alert: s.alert(&j.key)?,
                delivery: j,
                destination: d,
                address,
            }))
        })
    }
    pub fn alert_finish_delivery(&mut self, id: &str, outcome: Outcome, now: i64) -> Result<()> {
        self.alert_atomic(|s| {
            let mut j = s
                .alert_get::<Delivery>("delivery", id)?
                .ok_or("delivery missing")?;
            if j.status != "sending" {
                return Err("delivery not in flight".into());
            }
            let retry = match &outcome {
                Outcome::Sent => {
                    j.status = "sent".into();
                    j.error = None;
                    false
                }
                Outcome::Failed(e) => {
                    j.status = "failed".into();
                    j.error = Some(e.clone());
                    false
                }
                Outcome::Retry(e) | Outcome::RetryAfter(e, _) | Outcome::Unknown(e) => {
                    j.error = Some(e.clone());
                    true
                }
            };
            if retry {
                let delay = match outcome {
                    Outcome::RetryAfter(_, ms) => ms.max(j.action.retry_ms),
                    _ => j.action.retry_ms,
                }
                .min(i64::MAX as u64) as i64;
                j.status = if j.attempts < j.action.max_attempts
                    && now.saturating_add(delay) < j.expires
                {
                    "pending"
                } else {
                    "failed"
                }
                .into();
                j.due = now.saturating_add(delay);
            }
            j.updated_at = now;
            j.error = j.error.map(|s| s.chars().take(256).collect());
            s.alert_put("delivery", id, &j)?;
            s.alert_audit(
                "delivery",
                &j.status,
                &j.key,
                now,
                json!({"delivery":j.id,"attempts":j.attempts}),
            )
        })
    }
    pub fn alert_recover_deliveries(&mut self, now: i64) -> Result<()> {
        for mut j in self
            .alert_deliveries()?
            .into_iter()
            .filter(|j| j.status == "sending")
        {
            j.status = if j.attempts < j.action.max_attempts {
                "pending"
            } else {
                "failed"
            }
            .into();
            j.error = Some("delivery outcome unknown after restart".into());
            j.due = now.saturating_add(j.action.retry_ms as i64);
            j.updated_at = now;
            self.alert_put("delivery", &j.id, &j)?;
        }
        Ok(())
    }
}
#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    pub fn fixture() -> Store {
        let mut s = Store::open(":memory:").unwrap();
        let action = ResponseAction {
            id: "notify".into(),
            destinations: vec!["mail".into(), "chat".into()],
            delay_ms: 0,
            repeat_ms: Some(1000),
            until_ack: true,
            max_attempts: 3,
            retry_ms: 100,
            expiry_ms: 10000,
        };
        let p = Policy {
            id: "policy".into(),
            version: 1,
            source: "{evaluate(){return {active:true,stage:'warning'}}}".into(),
            stages: [(
                "warning".into(),
                Stage {
                    reset_ack: false,
                    actions: vec![action],
                },
            )]
            .into_iter()
            .collect(),
            recovery: vec![],
        };
        s.alert_policy_save(&p, 0, &BTreeMap::new(), "admin", 0)
            .unwrap();
        let b = Binding {
            id: "binding".into(),
            version: 1,
            policy: p.id,
            key: "disk".into(),
            params: Value::Null,
            inputs: BTreeMap::new(),
            labels: BTreeMap::new(),
            every_ms: 1000,
            enabled: true,
        };
        s.alert_binding_save(&b, 0, "admin", 0).unwrap();
        s.alert_observe(
            &Observation {
                key: b.key,
                active: true,
                stage: "warning".into(),
                severity: "warning".into(),
                message: "disk low".into(),
                labels: BTreeMap::new(),
                reset_ack: false,
            },
            0,
            "watch",
            0,
        )
        .unwrap();
        s.alert_put(
            "evaluation",
            &b.id,
            &Evaluation {
                last_at: Some(0),
                binding_version: 1,
                policy_version: 1,
                ..Default::default()
            },
        )
        .unwrap();
        for id in ["mail", "chat"] {
            s.alert_destination_save(
                &Destination {
                    id: id.into(),
                    version: 1,
                    channel: "email".into(),
                    provider: "smtp".into(),
                    target: format!("{id}@example.test"),
                    enabled: true,
                },
                0,
                "admin",
                0,
            )
            .unwrap();
        }
        s
    }
    #[test]
    fn independent_retry_ack_silence_and_coalescing() {
        let mut s = fixture();
        s.alert_schedule(0).unwrap();
        let jobs = s.alert_deliveries().unwrap();
        assert_eq!(jobs.len(), 2);
        for j in &jobs {
            s.alert_claim_delivery(&j.id, 0).unwrap().unwrap();
            s.alert_finish_delivery(
                &j.id,
                if j.destination == "mail" {
                    Outcome::Sent
                } else {
                    Outcome::Retry("temporary".into())
                },
                1,
            )
            .unwrap();
        }
        s.alert_schedule(100).unwrap();
        assert!(s
            .alert_claim_delivery(
                &jobs.iter().find(|j| j.destination == "mail").unwrap().id,
                200
            )
            .unwrap()
            .is_none());
        let a = s.alert("disk").unwrap();
        s.alert_acknowledge("disk", 1, a.revision, "human", 50)
            .unwrap();
        s.alert_schedule(200).unwrap();
        assert!(s
            .alert_deliveries()
            .unwrap()
            .iter()
            .any(|j| j.status == "skipped"));
        s.alert_schedule(100000).unwrap();
        assert_eq!(s.alert_deliveries().unwrap().len(), 2);
    }
    #[test]
    fn restart_revalidates_and_unknown_is_not_success() {
        let mut s = fixture();
        s.alert_schedule(0).unwrap();
        let j = s.alert_deliveries().unwrap().remove(0);
        s.alert_claim_delivery(&j.id, 0).unwrap().unwrap();
        s.alert_recover_policies(2000).unwrap();
        s.alert_recover_deliveries(2000).unwrap();
        assert!(s.alert_claim_delivery(&j.id, 3000).unwrap().is_none());
        assert_ne!(
            s.alert_get::<Delivery>("delivery", &j.id)
                .unwrap()
                .unwrap()
                .status,
            "sent"
        );
        s.alert_put(
            "evaluation",
            "binding",
            &Evaluation {
                last_at: Some(3000),
                binding_version: 1,
                policy_version: 1,
                ..Default::default()
            },
        )
        .unwrap();
        s.alert_schedule(3000).unwrap();
        let jobs = s.alert_deliveries().unwrap();
        assert!(jobs.len() <= 4);
        s.alert_schedule(3000).unwrap();
        assert_eq!(s.alert_deliveries().unwrap().len(), jobs.len());
    }
    #[test]
    fn devices_have_owners_and_rotating_tokens() {
        let mut s = fixture();
        let mut d = Device {
            id: "installation".into(),
            version: 1,
            owner: "forged".into(),
            token: "one".into(),
            groups: vec!["home".into()],
            enabled: true,
        };
        s.alert_device_register(&d, 0, "user", 0).unwrap();
        d.version = 2;
        d.token = "two".into();
        assert!(s.alert_device_register(&d, 1, "intruder", 1).is_err());
        s.alert_device_register(&d, 1, "user", 1).unwrap();
        assert_eq!(s.alert_devices_public().unwrap().len(), 1);
        assert!(!s.alert_devices_public().unwrap()[0]
            .to_string()
            .contains("token"));
        let dst = Destination {
            id: "push".into(),
            version: 1,
            channel: "push".into(),
            provider: "provider".into(),
            target: "user:user".into(),
            enabled: true,
        };
        assert_eq!(
            s.destination_targets(&dst).unwrap(),
            vec![Some("installation".into())]
        );
    }
}

#[cfg(test)]
mod recovery_tests {
    use super::*;
    #[test]
    fn silence_pauses_pending_without_replaying_missed_intervals() {
        let mut s = super::tests::fixture();
        s.alert_schedule(0).unwrap();
        s.alert_silence_save(
            &Silence {
                id: "maintenance".into(),
                version: 1,
                key: Some("disk".into()),
                labels: BTreeMap::new(),
                until: 5000,
                reason: "maintenance".into(),
                actor: String::new(),
            },
            0,
            "human",
            1,
        )
        .unwrap();
        s.alert_schedule(4000).unwrap();
        let jobs = s.alert_deliveries().unwrap();
        assert_eq!(jobs.len(), 2);
        for j in &jobs {
            assert!(s.alert_claim_delivery(&j.id, 4000).unwrap().is_none());
        }
        s.alert_schedule(5000).unwrap();
        assert_eq!(s.alert_deliveries().unwrap().len(), 2);
        for j in &jobs {
            s.alert_claim_delivery(&j.id, 5000).unwrap().unwrap();
            s.alert_finish_delivery(&j.id, Outcome::Sent, 5001).unwrap();
        }
        s.alert_schedule(9000).unwrap();
        assert_eq!(s.alert_deliveries().unwrap().len(), 4);
        s.alert_schedule(9000).unwrap();
        assert_eq!(s.alert_deliveries().unwrap().len(), 4);
    }
    #[test]
    fn acknowledgement_does_not_undo_inflight_and_resolution_skips_retry() {
        let mut s = super::tests::fixture();
        s.alert_schedule(0).unwrap();
        let jobs = s.alert_deliveries().unwrap();
        for j in &jobs {
            s.alert_claim_delivery(&j.id, 0).unwrap().unwrap();
        }
        s.alert_acknowledge("disk", 1, 1, "human", 1).unwrap();
        s.alert_finish_delivery(&jobs[0].id, Outcome::Sent, 2)
            .unwrap();
        s.alert_finish_delivery(&jobs[1].id, Outcome::Unknown("response lost".into()), 2)
            .unwrap();
        s.alert_schedule(3).unwrap();
        assert_eq!(
            s.alert_get::<Delivery>("delivery", &jobs[0].id)
                .unwrap()
                .unwrap()
                .status,
            "sent"
        );
        assert_eq!(
            s.alert_get::<Delivery>("delivery", &jobs[1].id)
                .unwrap()
                .unwrap()
                .status,
            "skipped"
        );
    }
}

#[cfg(test)]
mod qualification_tests {
    use super::*;
    #[test]
    fn late_pending_delivery_resumes_from_dispatch_time_without_burst() {
        let mut s = super::tests::fixture();
        s.alert_schedule(0).unwrap();
        let jobs = s.alert_deliveries().unwrap();
        for j in jobs {
            s.alert_claim_delivery(&j.id, 5000).unwrap().unwrap();
            s.alert_finish_delivery(&j.id, Outcome::Sent, 5001).unwrap();
        }
        s.alert_schedule(5002).unwrap();
        assert_eq!(s.alert_deliveries().unwrap().len(), 2);
        s.alert_schedule(6000).unwrap();
        assert_eq!(s.alert_deliveries().unwrap().len(), 4);
    }
    #[test]
    fn missing_destination_does_not_block_other_destinations() {
        let mut s = super::tests::fixture();
        s.conn
            .execute(
                "DELETE FROM alert_entities WHERE kind='destination' AND id='mail'",
                [],
            )
            .unwrap();
        s.alert_schedule(0).unwrap();
        assert_eq!(s.alert_deliveries().unwrap().len(), 1);
        assert_eq!(s.alert_list::<Value>("delivery_error").unwrap().len(), 1);
    }
}
