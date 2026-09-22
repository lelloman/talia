mod http_mcp;
mod browser;
mod deployment;
mod oidc;
use axum::{
    extract::{DefaultBodyLimit, State},
    routing::post,
    http::HeaderMap, Json, Router,
};
use serde_json::{json, Value};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
    time::Duration,
};
use talia_engine::{
    definitions::identifier,
    monitoring::MonitoringConfig,
    pipelines::Pipelines,
    runtime::Engine,
    scheduling::Scheduler,
    store::{Definition, Instance, Result, Store},
    value,
    watches::Watches,
};
use tokio::sync::{mpsc, oneshot};
struct Request {
    browser_subject: Option<String>,
    body: Value,
    credential: Option<String>,
    agent: bool,
    http_mcp: bool,
    connection: String,
    call: String,
    reply: oneshot::Sender<Value>,
}
#[derive(Clone)]
struct Service {
    engine: Engine,
    alert_policies: talia_engine::alerts::policy::Policies,
    alert_sender: talia_engine::alerts::providers::Sender,
    reports: talia_engine::reports::worker::Worker,
    pipelines: Pipelines,
    watches: Watches,
    live: talia_engine::mcp_live::Live,
    agent_engine: talia_engine::mcp_engine::AgentEngine,
    monitoring_error: Rc<RefCell<Option<String>>>,
    incarnation: String,
    clients: Rc<RefCell<HashMap<String, u64>>>,
    leases: Rc<RefCell<HashMap<String, (String, i64)>>>,
}
impl Service {
    fn snapshot(&self) -> Result<Value> {
        let s = self.engine.store.borrow();
        let all = s.instances()?;
        let mut values:Vec<Value>=all.into_iter().map(|i|{let meta=self.engine.evaluation.borrow().get(&i.id).cloned().unwrap_or_default();json!({"id":i.id,"value":i.value,"hasValue":i.has_value,"timestamp":i.timestamp,"quality":i.quality,"revision":i.revision,"generation":i.generation,"evaluation":if meta.running>0{"refreshing"}else if meta.error.is_some(){"error"}else if meta.invalidated{"invalidated"}else{"idle"},"error":meta.error})}).collect();
        values.extend(s.monitoring_values()?);
        Ok(
            json!({"revision":s.revision()?,"values":values,"monitoringVersion":s.monitoring_config()?.version,"monitoringError":self.monitoring_error.borrow().clone()}),
        )
    }
    fn status(&self, id: &str) -> Result<Value> {
        identifier(id)?;
        let store = self.engine.store.borrow();
        if let Some(a) = store.run_admission(id)? {
            let run = a.run_id.as_ref().map(|id| store.run(id)).transpose()?;
            return Ok(
                json!({"actionId":id,"status":run.as_ref().map(|r|r.status.as_str()).unwrap_or("skipped"),"runId":a.run_id,"admission":a.disposition,"outcome":run}),
            );
        }
        Ok(match store.action(id)? {
            Some((_, status, outcome)) => {
                json!({"actionId":id,"status":status,"outcome":outcome.and_then(|x|serde_json::from_str::<Value>(&x).ok())})
            }
            None => json!({"actionId":id,"status":"unknown"}),
        })
    }
    async fn agent_execute(&self, credential:&str, connection:&str, call:&str, body:Value, http_mcp:bool)->talia_engine::authority::Result<Value> {
        use talia_engine::authority::ErrorCode;
        if http_mcp && body["name"]=="_key_auth" {let store=self.engine.store.borrow();let mut info=store.user_key_info(credential)?;info["admin"]=json!(store.user_admin(info["subject"].as_str().unwrap())?);return Ok(info)}
        let session=if http_mcp {self.engine.store.borrow().user_agent_authenticate(credential)?}else{self.engine.store.borrow().agent_authenticate(credential)?};
        if http_mcp {self.engine.store.borrow().agent_require_dashboard_admin(&session)?;}
        let r: talia_engine::mcp::Request=serde_json::from_value(body).map_err(|_|ErrorCode::InvalidInput)?;
        if let Some(op)=r.name.strip_prefix("reports_") {return Ok(self.engine.store.borrow_mut().report_api(&session,op,r.arguments,self.engine.now()).unwrap_or_else(|error|json!({"error":error})));}
        if let Some(op)=r.name.strip_prefix("alerts_") {return Ok(self.engine.store.borrow_mut().alert_api(&session,op,r.arguments,self.engine.now()).unwrap_or_else(|error|json!({"error":error})));}
        if r.name=="_cancel_call"||r.name=="_session_close" {self.live.cancel(&session,connection,if r.name=="_cancel_call" {r.arguments["callId"].as_str()}else{None})?;}
        if r.name=="clients_list"||r.name.starts_with("live_"){return self.live.execute(&session,connection,call,r).await;}
        if r.name=="operation_status" {if r.arguments.as_object().is_none_or(|o|o.len()!=1||!o.contains_key("requestId")){return Err(ErrorCode::InvalidInput)}let result=self.live.outcome(&session,r.arguments["requestId"].as_str().unwrap_or(""))?;if result["outcome"].is_object(){let mut result=result;result.as_object_mut().unwrap().remove("error");return Ok(result)}}
        if r.name.starts_with("engine_") || r.name.starts_with('_') || r.name=="operation_status" {
            return self.agent_engine.execute(&session,connection,call,r).await;
        }
        let before=if r.name=="definitions_save" {self.engine.store.borrow().instances().map_err(|_|ErrorCode::StorageError)?}else{vec![]};
        let value=talia_engine::mcp::dispatch(&mut self.engine.store.borrow_mut(),&session,r,self.engine.now())?;
        if !value["receipt"].is_null() {
            let after=self.engine.store.borrow().instances().map_err(|_|ErrorCode::StorageError)?;
            for i in &after {if !before.iter().any(|b|b.id==i.id && b.revision==i.revision && b.generation==i.generation){self.engine.catalog_changed(&i.id);}}
            for i in &before {if !after.iter().any(|a|a.id==i.id){self.engine.changed(&i.id);}}
        }
        Ok(value)
    }
    async fn execute(&self, r: &Value) -> Result<Value> {
        if r["version"] != 1 {
            return Err("protocol version".into());
        }
        let client = r["client"].as_str().ok_or("client identity")?;
        identifier(client)?;
        let epoch = r["epoch"]
            .as_u64()
            .filter(|x| *x > 0 && *x < 9_007_199_254_740_991)
            .ok_or("client epoch")?;
        {
            let mut clients = self.clients.borrow_mut();
            if !clients.contains_key(client) && clients.len() >= 256 {
                return Err("client capacity".into());
            }
            let active = clients.entry(client.into()).or_default();
            if epoch < *active {
                return Err("stale client epoch".into());
            }
            *active = epoch;
        }
        let op = r["op"].as_str().ok_or("operation")?;
        let a = &r["args"];
        if op == "hello" {
            return self.snapshot();
        }
        if r["incarnation"] != self.incarnation {
            return Err("server incarnation changed".into());
        }
        let id = a["id"].as_str().unwrap_or("value");
        match op {
            "snapshot" => self.snapshot(),
            "monitoringConfig" => {
                Ok(serde_json::to_value(self.engine.store.borrow().monitoring_config()?).unwrap())
            }
            "configureMonitoring" => {
                let config: MonitoringConfig =
                    serde_json::from_value(a["config"].clone()).map_err(|e| e.to_string())?;
                self.engine.store.borrow_mut().configure_monitoring(
                    &config,
                    a["expected"]
                        .as_u64()
                        .ok_or("expected monitoring version")?,
                )?;
                self.snapshot()
            }
            "run" => {
                let action = a["actionId"].as_str().ok_or("action id")?;
                if self.engine.store.borrow().action(action)?.is_some() {
                    return Err("action identity belongs to a value mutation".into());
                }
                self.pipelines.request(id, action, "manual", 0)?;
                self.status(action)
            }
            "runs" => Ok(serde_json::to_value(self.engine.store.borrow().runs()?).unwrap()),
            "runStatus" => Ok(serde_json::to_value(self.engine.store.borrow().run(id)?).unwrap()),
            "cancelRun" => Ok(serde_json::to_value(self.pipelines.cancel(id)?).unwrap()),
            "resumeWatch" => {
                self.watches.resume(id)?;
                self.snapshot()
            }
            "read" => {
                if id.starts_with("monitor.") {
                    return self
                        .engine
                        .store
                        .borrow()
                        .monitoring_values()?
                        .into_iter()
                        .find(|v| v["id"] == id)
                        .ok_or_else(|| "monitoring resource missing".into());
                }
                let i = self.engine.read(id, Duration::from_secs(5)).await?;
                Ok(serde_json::to_value(i).unwrap())
            }
            "subscribe" => {
                let key = format!("{client}:{id}");
                let mut leases = self.leases.borrow_mut();
                if !leases.contains_key(&key) {
                    if leases.len() >= 1024 {
                        return Err("subscription capacity".into());
                    }
                    if id.starts_with("monitor.") {
                        self.engine.store.borrow().monitor_state(&id[8..])?;
                    } else {
                        self.engine.subscribe(id)?;
                    }
                }
                leases.insert(key, (id.into(), self.engine.now()));
                drop(leases);
                if !id.starts_with("monitor.") {
                    self.engine.invalidate(id)?;
                }
                self.snapshot()
            }
            "unsubscribe" => {
                if let Some((id, _)) = self.leases.borrow_mut().remove(&format!("{client}:{id}")) {
                    self.engine.unsubscribe(&id);
                }
                Ok(Value::Null)
            }
            "poll" => {
                for (_, (_, seen)) in self
                    .leases
                    .borrow_mut()
                    .iter_mut()
                    .filter(|(k, _)| k.starts_with(&format!("{client}:")))
                {
                    *seen = self.engine.now();
                }
                self.snapshot()
            }
            "status" => self.status(a["actionId"].as_str().ok_or("action id")?),
            "write" | "set" => {
                let action = a["actionId"].as_str().ok_or("action id")?;
                identifier(action)?;
                if self.engine.store.borrow().run_admission(action)?.is_some() {
                    return Err("action identity belongs to a Pipeline run".into());
                }
                value::validate(&a["value"])?;
                let request=json!({"client":client,"op":op,"id":id,"value":a["value"],"expected":a["expected"]}).to_string();
                if !self.engine.store.borrow().accept_action(action, &request)? {
                    return self.status(action);
                }
                // This task is detached from the HTTP wait; accepted work finishes independently.
                let outcome = if op == "write" {
                    let expected = a["expected"].as_u64().ok_or("expected revision");
                    match expected {
                        Ok(n) => self.engine.write(id, n, a["value"].clone()),
                        Err(e) => Err(e.into()),
                    }
                } else {
                    self.engine.set(id, a["value"].clone()).await
                };
                let (status, body) = match outcome {
                    Ok(i) => ("complete", serde_json::to_value(i).unwrap()),
                    Err(e) => ("failed", json!({"error":e})),
                };
                self.engine
                    .store
                    .borrow()
                    .finish_action(action, status, &body.to_string())?;
                self.status(action)
            }
            "define" => {
                let d: Definition =
                    serde_json::from_value(a["definition"].clone()).map_err(|e| e.to_string())?;
                let expected = a["expected"].as_u64().ok_or("expected version")?;
                self.engine
                    .store
                    .borrow_mut()
                    .define(&d, expected, a["migration"].as_str())?;
                let all = self.engine.store.borrow().instances()?;
                for i in all.iter().filter(|i| i.definition == d.id) {
                    self.engine.invalidate(&i.id)?;
                    self.engine.changed(&i.id);
                }
                self.snapshot()
            }
            "create" => {
                let i: Instance =
                    serde_json::from_value(a["instance"].clone()).map_err(|e| e.to_string())?;
                self.engine.store.borrow_mut().add_instance(&i)?;
                self.snapshot()
            }
            "parameters" => {
                self.engine.store.borrow_mut().reconfigure(
                    id,
                    a["expected"].as_u64().ok_or("expected revision")?,
                    a["value"].clone(),
                )?;
                self.engine.invalidate(id)?;
                self.engine.changed(id);
                self.snapshot()
            }
            "remove" => {
                self.engine.store.borrow_mut().remove_instance(id)?;
                self.snapshot()
            }
            "removeDefinition" => {
                self.engine.store.borrow_mut().remove_definition(id)?;
                self.snapshot()
            }
            "invalidate" => {
                self.engine.invalidate(id)?;
                self.snapshot()
            }
            "history" => Ok(serde_json::to_value(
                self.engine.store.borrow().history(id, self.engine.now())?,
            )
            .unwrap()),
            _ => Err("unknown operation".into()),
        }
    }
}
async fn health(State(tx): State<mpsc::Sender<Request>>) -> axum::http::StatusCode {
    let (reply, rx)=oneshot::channel();
    if tx.try_send(Request { http_mcp:false,browser_subject:None,body:json!({"version":1,"client":"healthcheck","epoch":1,"op":"hello"}),reply,credential:None,agent:false,connection:String::new(),call:String::new()}).is_err(){return axum::http::StatusCode::SERVICE_UNAVAILABLE;}
    match tokio::time::timeout(Duration::from_secs(3),rx).await {
        Ok(Ok(v)) if v.get("error").is_none() => axum::http::StatusCode::OK,
        _=>axum::http::StatusCode::SERVICE_UNAVAILABLE
    }
}
async fn rpc(State(tx): State<mpsc::Sender<Request>>, browser:Option<axum::Extension<deployment::Identity>>, Json(body): Json<Value>) -> Json<Value> {
    let (reply, rx) = oneshot::channel();
    if tx.try_send(Request { http_mcp:false, browser_subject:browser.map(|axum::Extension(i)|i.subject), body, reply, credential: None, agent: false, connection:String::new(), call:String::new() }).is_err() {
        return Json(json!({"error":"server busy"}));
    }
    Json(
        rx.await
            .unwrap_or_else(|_| json!({"error":"server stopped"})),
    )
}
async fn client_rpc(State(tx): State<mpsc::Sender<Request>>, headers: HeaderMap, browser:Option<axum::Extension<deployment::Identity>>, Json(body): Json<Value>) -> Json<Value> {
    let credential = headers.get("authorization").and_then(|s|s.to_str().ok()).and_then(|s|s.strip_prefix("Bearer ")).unwrap_or("").to_string();
    let (reply, rx) = oneshot::channel();
    if tx.try_send(Request { http_mcp:false, browser_subject:browser.map(|axum::Extension(i)|i.subject), body, reply, credential: Some(credential), agent: false, connection:String::new(), call:String::new() }).is_err() { return Json(json!({"error":"limit_exceeded"})); }
    Json(rx.await.unwrap_or_else(|_|json!({"error":"internal_error"})))
}
async fn agent_rpc(State(tx): State<mpsc::Sender<Request>>, headers: HeaderMap, Json(body): Json<Value>) -> Json<Value> {
    if headers.contains_key("origin") { return Json(json!({"error":"forbidden"})); }
    let credential = headers.get("authorization").and_then(|s|s.to_str().ok()).and_then(|s|s.strip_prefix("Bearer ")).unwrap_or("").to_string();
    let (reply, rx) = oneshot::channel();
    if tx.try_send(Request { http_mcp:false, browser_subject:None, body, reply, credential: Some(credential), agent: true, connection:headers.get("x-talia-session").and_then(|v|v.to_str().ok()).unwrap_or("").into(), call:headers.get("x-talia-call").and_then(|v|v.to_str().ok()).unwrap_or("").into() }).is_err() { return Json(json!({"error":"limit_exceeded"})); }
    Json(rx.await.unwrap_or_else(|_|json!({"error":"internal_error"})))
}
async fn alerts_rpc(State(tx):State<mpsc::Sender<Request>>,headers:HeaderMap,browser:Option<axum::Extension<deployment::Identity>>,Json(body):Json<Value>)->Json<Value>{
    let credential=headers.get("authorization").and_then(|s|s.to_str().ok()).and_then(|s|s.strip_prefix("Bearer ")).unwrap_or("").to_string();
    let (reply,rx)=oneshot::channel();
    if body.as_object().is_none_or(|o|o.keys().any(|k|k!="op"&&k!="args"&&k!="dashboard")){return Json(json!({"error":"invalid_input"}));}
    let context=body["dashboard"].clone();let mut body=json!({"name":format!("alerts_{}",body["op"].as_str().unwrap_or("")),"arguments":body.get("args").cloned().unwrap_or_else(||json!({}))});if browser.is_some(){body["dashboard"]=context;}
    if tx.try_send(Request{http_mcp:false,browser_subject:browser.map(|axum::Extension(i)|i.subject),body,credential:Some(credential),agent:true,connection:String::new(),call:String::new(),reply}).is_err(){return Json(json!({"error":"limit_exceeded"}));}
    Json(rx.await.unwrap_or_else(|_|json!({"error":"internal_error"})))
}
async fn account_rpc(State(tx):State<mpsc::Sender<Request>>, browser:Option<axum::Extension<deployment::Identity>>, Json(mut body):Json<Value>)->Json<Value>{
 let Some(axum::Extension(identity))=browser else{return Json(json!({"error":"unauthenticated"}))};
 // Internal envelope keeps trusted identity separate from untrusted operation arguments.
 body=json!({"name":identity.name,"authSession":identity.session_id,"request":body});
 let (reply,rx)=oneshot::channel();
 if tx.try_send(Request{http_mcp:false,browser_subject:Some(identity.subject),body,reply,credential:None,agent:false,connection:"account".into(),call:String::new()}).is_err(){return Json(json!({"error":"limit_exceeded"}))}
 Json(rx.await.unwrap_or_else(|_|json!({"error":"internal_error"})))
}
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    run(std::env::args().collect()).await
}
async fn run(args:Vec<String>)->Result<()> {
    let path = args
        .get(1)
        .ok_or("usage: talia-engine DATABASE [PORT] [--seed]")?;
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(format!("{path}.lock"))
        .map_err(|e| e.to_string())?;
    lock.try_lock().map_err(|_| "database already owned")?;
    let mut store = Store::open(path)?;
    store.recover_actions()?;
    store.clients_recover().map_err(|_|"client recovery failed".to_string())?;
    if args.iter().any(|s| s == "--seed") && store.definitions()?.is_empty() {
        let d = Definition {
            id: "stored".into(),
            version: 1,
            source: "".into(),
            kind: "stored".into(),
            value_schema: "any".into(),
            state_schema: "any".into(),
            dependencies: vec![],
            read_policy: "shared".into(),
        };
        store.define(&d, 0, None)?;
        store.add_instance(&Instance {
            id: "value".into(),
            definition: d.id,
            params: value::undefined(),
            state: value::undefined(),
            value: value::number(62.0),
            has_value: true,
            timestamp: 0,
            quality: "seed".into(),
            revision: 1,
            generation: 1,
            history_count: 100,
            history_age_ms: 86400000,
        })?;
    }
    if args.iter().any(|s|s == "--seed") { store.seed_dashboards()?; }
    let incarnation = std::fs::read_to_string("/proc/sys/kernel/random/uuid")
        .map_err(|e| e.to_string())?
        .trim()
        .to_string();
    if let Ok(subjects)=std::env::var("TALIA_BOOTSTRAP_ADMINS") {for subject in subjects.split(',').map(str::trim).filter(|s|!s.is_empty()){store.user_bootstrap(subject).map_err(|e|format!("admin bootstrap: {e:?}"))?;}}
    let engine = Engine::new(store);
    engine.store.borrow_mut().recover_runs(engine.now())?;
    engine.store.borrow_mut().agent_recover(engine.now()).map_err(|_| "agent recovery failed".to_string())?;
    engine.store.borrow_mut().alert_recover_policies(engine.now())?;
    engine.store.borrow_mut().alert_recover_deliveries(engine.now())?;
    engine.store.borrow().report_recover()?;
    let pipelines = Pipelines::new(engine.clone())?;
    let watches = Watches::new(pipelines.clone());
    let agent_engine=talia_engine::mcp_engine::AgentEngine::new(engine.clone(),pipelines.clone(),watches.clone());
    let service = Service {
        live: talia_engine::mcp_live::Live::new(engine.clone(),agent_engine.clone()),
        agent_engine,
        alert_policies: talia_engine::alerts::policy::Policies::new(engine.clone()),
        alert_sender: talia_engine::alerts::providers::Sender::new(engine.clone()),
        reports: talia_engine::reports::worker::Worker::new(engine.clone()),
        engine,
        pipelines,
        watches,
        monitoring_error: Default::default(),
        incarnation: incarnation.clone(),
        clients: Default::default(),
        leases: Default::default(),
    };
    let (tx, mut rx) = mpsc::channel::<Request>(128);
    let deployment = deployment::Deployment::load().await?;
    let mut router = Router::new()
        .route("/engine", post(rpc))
        .route("/clients", post(client_rpc))
        .route("/account", post(account_rpc))
        .route("/agent", post(agent_rpc))
        .route("/alerts", post(alerts_rpc))
        .layer(DefaultBodyLimit::max(2_359_296))
        .route("/healthz", axum::routing::get(health))
        .with_state(tx.clone());
    if let Some(d)=deployment {
        router=router.merge(http_mcp::routes(tx.clone(),d.clone())).merge(d.routes()).layer(axum::middleware::from_fn_with_state(d, deployment::Deployment::gate));
    }
    let listener = tokio::net::TcpListener::bind((
        std::env::var("TALIA_LISTEN").unwrap_or_else(|_|"127.0.0.1".into()),
        args.get(2)
            .and_then(|p| p.parse::<u16>().ok())
            .unwrap_or(18745),
    ))
    .await
    .map_err(|e| e.to_string())?;
    println!(
        "{}",
        json!({"port":listener.local_addr().unwrap().port(),"incarnation":incarnation})
    );
    let local = tokio::task::LocalSet::new();
    local.run_until(async move{
  let monitoring=service.clone();tokio::task::spawn_local(async move{
    let scheduler=Scheduler{pipelines:monitoring.pipelines.clone()};
    loop {
      let mut errors=vec![];
      if let Err(e)=scheduler.tick(){errors.push(e);}
      if let Err(e)=monitoring.watches.tick(){errors.push(e);}
      if let Err(e)=monitoring.alert_policies.tick(){errors.push(e);}
      if let Err(e)=monitoring.engine.store.borrow_mut().alert_schedule(monitoring.engine.now()){errors.push(e);}
      if let Err(e)=monitoring.alert_sender.tick(){errors.push(e);}
      if let Err(e)=monitoring.reports.tick(){errors.push(e);}
      *monitoring.monitoring_error.borrow_mut()=if errors.is_empty(){None}else{Some(errors.join("; "))};
      tokio::time::sleep(Duration::from_millis(50)).await;
    }
  });
  let agent_leases=service.agent_engine.clone();tokio::task::spawn_local(async move{loop{tokio::time::sleep(Duration::from_secs(1)).await;agent_leases.sweep();}});
  let leases=service.clone();tokio::task::spawn_local(async move{loop{tokio::time::sleep(Duration::from_secs(10)).await;let expired:Vec<_>=leases.leases.borrow().iter().filter(|(_,(_,t))|leases.engine.now()-*t>60000).map(|(k,_)|k.clone()).collect();for key in expired{if let Some((id,_))=leases.leases.borrow_mut().remove(&key){leases.engine.unsubscribe(&id);}}}});
  tokio::task::spawn_local(async move{let active=Rc::new(Cell::new(0usize));while let Some(request)=rx.recv().await{if active.get()>=128{let _=request.reply.send(json!({"error":if request.agent {"limit_exceeded"}else{"server busy"}}));continue;}let active=active.clone();active.set(active.get()+1);let s=service.clone();tokio::task::spawn_local(async move{
  if let Some(subject)=request.browser_subject.as_deref(){
    let result=s.browser_execute(subject,&request).await;
    let response=if request.connection=="account"||request.agent {result.unwrap_or_else(|e|json!({"error":e}))}
      else if request.credential.is_some(){match result{Ok(v)=>json!({"value":v,"incarnation":s.incarnation}),Err(e)=>json!({"error":e,"incarnation":s.incarnation})}}
      else{match result{Ok(v)=>json!({"version":1,"incarnation":s.incarnation,"epoch":request.body["epoch"],"value":v}),Err(e)=>json!({"version":1,"incarnation":s.incarnation,"epoch":request.body["epoch"],"error":e})}};
    let _=request.reply.send(response);active.set(active.get()-1);return;
  }
  if request.agent {
    if request.reply.is_closed() {active.set(active.get()-1);return;}
    let result=s.agent_execute(request.credential.as_deref().unwrap_or(""),&request.connection,&request.call,request.body,request.http_mcp).await;
    let response=result.unwrap_or_else(|error|json!({"error":error}));
    let _=request.reply.send(response);active.set(active.get()-1);return;
  }if let Some(credential)=request.credential {
    let result=if request.body["op"].as_str().is_some_and(|op|op.starts_with("live")){s.live.host(&credential,request.body).await}else{serde_json::from_value::<talia_engine::clients::Request>(request.body).map_err(|_|talia_engine::authority::ErrorCode::InvalidInput).and_then(|r|s.engine.store.borrow_mut().client_request(&credential,r,s.engine.now()))};
    let response=match result {Ok(value)=>json!({"value":value,"incarnation":s.incarnation}),Err(error)=>json!({"error":error,"incarnation":s.incarnation})};let _=request.reply.send(response);active.set(active.get()-1);return;
  }let result=s.execute(&request.body).await;let response=match result{Ok(value)=>json!({"version":1,"incarnation":s.incarnation,"epoch":request.body["epoch"],"value":value}),Err(error)=>json!({"version":1,"incarnation":s.incarnation,"epoch":request.body["epoch"],"error":error})};let _=request.reply.send(response);active.set(active.get()-1);});}});
  axum::serve(listener,router).await.map_err(|e|e.to_string())
 }).await
}
