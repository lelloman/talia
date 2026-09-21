//! OIDC browser dispatch. Dashboard data grants are enforced on the server.
use crate::*;
impl Service {
    pub(crate) async fn browser_execute(
        &self,
        subject: &str,
        r: &Request,
    ) -> std::result::Result<Value, String> {
        use talia_engine::authority::ErrorCode as E;
        let err = |e: E| {
            serde_json::to_value(e)
                .unwrap()
                .as_str()
                .unwrap()
                .to_string()
        };
        if r.connection == "account" {
            let mut store = self.engine.store.borrow_mut();
            store
                .user_seen(subject, r.body["name"].as_str().unwrap_or(subject))
                .map_err(err)?;
            return store
                .user_request(subject, r.body["request"].clone())
                .map_err(err);
        }
        let admin = self
            .engine
            .store
            .borrow()
            .user_admin(subject)
            .map_err(err)?;
        if let Some(credential) = &r.credential {
            if !r.agent {
                if r.body["op"]
                    .as_str()
                    .is_some_and(|op| op.starts_with("live"))
                {
                    if !admin {
                        return Err("forbidden".into());
                    }
                    return self
                        .live
                        .host(credential, r.body.clone())
                        .await
                        .map_err(err);
                }
                return self
                    .engine
                    .store
                    .borrow_mut()
                    .user_client_request(subject, credential, r.body.clone(), self.engine.now())
                    .map_err(err);
            }
        }
        if r.agent {
            let op = r.body["name"]
                .as_str()
                .unwrap_or("")
                .strip_prefix("alerts_")
                .unwrap_or("");
            if !admin {
                let pkg = self.browser_package(subject, &r.body)?;
                if !["snapshot", "history"].contains(&op)
                    || !pkg["grants"]["reads"]
                        .as_array()
                        .is_some_and(|a| a.iter().any(|v| v == "alerts"))
                {
                    return Err("forbidden".into());
                }
            }
            let mut store = self.engine.store.borrow_mut();
            let session = store.browser_alert_session(subject).map_err(err)?;
            return store.alert_api(&session, op, r.body["arguments"].clone(), self.engine.now());
        }
        let mut body = r.body.clone();
        // Isolate epochs and subscription leases between authenticated users and dashboards.
        let namespace = format!("{}:{}:{}", subject, body["client"], body["dashboard"]);
        body["client"] = json!(format!(
            "b{}",
            ring::digest::digest(&ring::digest::SHA256, namespace.as_bytes()).as_ref()[..24]
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        ));
        if admin {
            return self.execute(&body).await;
        }
        let pkg = match self.browser_package(subject, &body) {
            Ok(pkg) => pkg,
            Err(error) => {
                self.browser_drop_leases(body["client"].as_str().unwrap(), &[]);
                return Err(error);
            }
        };
        let op = body["op"].as_str().unwrap_or("");
        if ![
            "hello",
            "snapshot",
            "read",
            "subscribe",
            "unsubscribe",
            "poll",
            "history",
        ]
        .contains(&op)
        {
            return Err("forbidden".into());
        }
        let reads = pkg["grants"]["reads"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        self.browser_drop_leases(body["client"].as_str().unwrap(), &reads);
        if ["read", "subscribe", "unsubscribe", "history"].contains(&op)
            && !reads.contains(&body["args"]["id"])
        {
            return Err("forbidden".into());
        }
        let mut result = self
            .execute(&body)
            .await
            .map_err(|_| "resource_unavailable".to_string())?;
        // Awaiting a getter must not allow a revoked share or changed package to return data.
        self.browser_package(subject, &body)?;
        if let Some(values) = result.get_mut("values").and_then(Value::as_array_mut) {
            values.retain(|v| reads.contains(&v["id"]));
            for v in values {
                if !v["error"].is_null() {
                    v["error"] = json!("resource_unavailable")
                }
            }
            result.as_object_mut().unwrap().remove("monitoringError");
            result.as_object_mut().unwrap().remove("monitoringVersion");
        }
        if op == "read" {
            let allowed = [
                "id",
                "value",
                "has_value",
                "hasValue",
                "timestamp",
                "quality",
                "revision",
                "generation",
                "evaluation",
            ];
            if let Some(o) = result.as_object_mut() {
                o.retain(|k, _| allowed.contains(&k.as_str()));
            }
        }
        Ok(result)
    }
    fn browser_drop_leases(&self, client: &str, reads: &[Value]) {
        let prefix = format!("{client}:");
        let remove: Vec<_> = self
            .leases
            .borrow()
            .iter()
            .filter(|(key, (id, _))| key.starts_with(&prefix) && !reads.contains(&json!(id)))
            .map(|(k, _)| k.clone())
            .collect();
        for key in remove {
            if let Some((id, _)) = self.leases.borrow_mut().remove(&key) {
                self.engine.unsubscribe(&id);
            }
        }
    }
    fn browser_package(&self, subject: &str, body: &Value) -> Result<Value> {
        let context = &body["dashboard"];
        let pkg = self
            .engine
            .store
            .borrow()
            .user_package(subject, context["id"].as_str().ok_or("forbidden")?)
            .map_err(|_| "forbidden")?;
        if pkg["revision"] != context["revision"] {
            return Err("dashboard_changed".into());
        }
        Ok(pkg)
    }
}
#[cfg(test)]
mod tests;
