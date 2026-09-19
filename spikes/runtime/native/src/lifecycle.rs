//! Persistent native dashboard fixture. UI lifecycle owns admission; HTTP outlives pause.
use super::{policy, Guest};
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};
static SIGNALS: AtomicBool = AtomicBool::new(false);
static VISIBLE: AtomicBool = AtomicBool::new(false);
thread_local! {static SESSION:RefCell<Option<Session>>=const {RefCell::new(None)};}
const SAVED: &str = include_str!("../../../lifecycle/dashboard.js");
struct Reply {
    epoch: u64,
    guest_id: Option<Value>,
    action: Option<String>,
    value: Value,
}
struct Session {
    guest: Option<Guest>,
    survivor: Guest,
    survivor_ticks: u64,
    failure: Option<String>,
    external_error: Option<String>,
    signals: Vec<Value>,
    port: u16,
    active: bool,
    epoch: u64,
    next: u64,
    pending: usize,
    ignored: usize,
    last_request: u64,
    snapshot: Value,
    local: Value,
    outcomes: BTreeMap<String, Value>,
    tx: mpsc::Sender<Reply>,
    rx: mpsc::Receiver<Reply>,
}
impl Session {
    fn new(port: u16) -> Self {
        let guest = Guest::with_grants(&["read", "write"]);
        guest.eval(SAVED).unwrap();
        let survivor = Guest::with_grants(&[]);
        survivor.eval("globalThis.survivorTicks=0;").unwrap();
        let (tx, rx) = mpsc::channel();
        Self {
            guest: Some(guest),
            survivor,
            survivor_ticks: 0,
            failure: None,
            external_error: None,
            signals: Vec::new(),
            port,
            active: false,
            epoch: 0,
            next: 0,
            pending: 0,
            ignored: 0,
            last_request: 0,
            snapshot: Value::Null,
            local: json!({"dirty":false,"ticks":0}),
            outcomes: BTreeMap::new(),
            tx,
            rx,
        }
    }
    fn guest(&self) -> &Guest {
        self.guest.as_ref().expect("stopped dashboard")
    }
    fn fail(&mut self, diagnostic: String) {
        if self.failure.is_some() {
            return;
        }
        self.active = false;
        self.epoch += 1;
        if SIGNALS.load(Ordering::SeqCst) {
            self.signals
                .push(json!({"type":"dashboard-failure","diagnostic":diagnostic}));
        }
        self.failure = Some(diagnostic);
        self.guest.take();
    }
    fn send(&mut self, op: &str, args: Value, guest_id: Option<Value>, action: Option<String>) {
        if self.pending >= 32 {
            return;
        }
        self.next += 1;
        self.pending += 1;
        let request = json!({"session":"lifecycle-android","channel":"dashboard","epoch":self.epoch,"id":self.next,"op":op,"args":args});
        let (tx, port, epoch) = (self.tx.clone(), self.port, self.epoch);
        thread::spawn(move || {
            let value =
                super::transport::http(port, &request).unwrap_or_else(|e| json!({"error":e}));
            tx.send(Reply {
                epoch,
                guest_id,
                action,
                value,
            })
            .ok();
        });
    }
    fn deadline(&self) {
        if self.failure.is_some() {
            return;
        }
        let end = Instant::now() + Duration::from_millis(100);
        self.guest()
            .rt
            .set_interrupt_handler(Some(Box::new(move || Instant::now() > end)));
    }
    fn sync_visibility(&mut self) {
        if self.failure.is_some() {
            return;
        }
        let visible = VISIBLE.load(Ordering::SeqCst);
        if visible == self.active {
            return;
        }
        self.epoch += 1;
        self.active = visible;
        self.guest().out.borrow_mut().clear();
        if visible {
            self.deadline();
            self.guest().eval("engine.cancelPending();").unwrap();
            self.send("read", json!({"tag":"resume"}), None, None);
            for id in self.outcomes.keys().cloned().collect::<Vec<_>>() {
                self.send("status", json!({"actionId":id}), None, Some(id));
            }
        }
    }
    fn tick(&mut self) {
        if VISIBLE.load(Ordering::SeqCst) {
            let end = Instant::now() + Duration::from_millis(100);
            self.survivor
                .rt
                .set_interrupt_handler(Some(Box::new(move || Instant::now() > end)));
            self.survivor.eval("survivorTicks++;").unwrap();
            self.survivor_ticks = self.survivor.read("survivorTicks").as_u64().unwrap();
        }
        self.sync_visibility();
        while let Ok(reply) = self.rx.try_recv() {
            self.pending -= 1;
            if !self.active || reply.epoch != self.epoch {
                self.ignored += 1;
                continue;
            }
            if let Some(error) = reply.value.get("error") {
                self.external_error = Some(error.to_string());
            }
            if let Some(action) = reply.action {
                self.outcomes.insert(action, reply.value["value"].clone());
            }
            let data = &reply.value["value"];
            let snapshot = if data.get("revision").is_some() {
                data
            } else {
                &data["result"]
            };
            if snapshot.get("revision").is_some()
                && (self.snapshot.is_null()
                    || snapshot["revision"].as_u64() >= self.snapshot["revision"].as_u64())
            {
                self.snapshot = snapshot.clone();
            }
            if let Some(id) = reply.guest_id {
                self.deadline();
                self.guest().deliver(if reply.value.get("error").is_some() {
                    json!({"id":id,"error":reply.value["error"]})
                } else {
                    json!({"id":id,"value":data})
                });
            }
        }
        if !self.active {
            return;
        }
        self.deadline();
        self.guest().eval("vm.state.ticks++;").unwrap();
        for _ in 0..1000 {
            if !self.guest().rt.is_job_pending() {
                break;
            }
            self.guest().rt.execute_pending_job().unwrap();
        }
        assert!(!self.guest().rt.is_job_pending(), "job budget");
        loop {
            let raw = self.guest().out.borrow_mut().pop_front();
            let Some(raw) = raw else { break };
            let req = policy::request(&raw).unwrap();
            let id = req["id"].as_u64().unwrap();
            assert!(id > self.last_request);
            self.last_request = id;
            match req["op"].as_str().unwrap() {
                "read" => self.send(
                    "read",
                    json!({"tag":"guest"}),
                    Some(req["id"].clone()),
                    None,
                ),
                "write" => {
                    let action = format!(
                        "a-{}-{}-{}",
                        std::process::id(),
                        self.guest().generation,
                        id
                    );
                    self.outcomes
                        .insert(action.clone(), json!({"status":"unknown"}));
                    self.send(
                        "action",
                        json!({"actionId":action,"value":req["value"]}),
                        Some(req["id"].clone()),
                        Some(action),
                    );
                }
                _ => panic!("ungranted operation"),
            }
        }
        self.local = self.guest().read("vm.state");
        if self.pending == 0 {
            self.send("read", json!({"tag":"subscription"}), None, None);
        }
    }
    fn report(&self) -> Value {
        json!({"survivor_ticks":self.survivor_ticks,"active":self.active,"failure":self.failure,"external_error":self.external_error,"signals":self.signals,"pending_waits":if self.guest.is_some(){self.pending}else{0},"epoch":self.epoch,"local":self.local,"snapshot":self.snapshot,"outcomes":self.outcomes,"pending":self.pending,"ignored":self.ignored,"subscriptions":if self.active {1}else{0}})
    }
}
#[no_mangle]
pub extern "system" fn Java_com_lelloman_talia_spike_LifecycleActivity_setDashboardVisible(
    _: *mut std::ffi::c_void,
    _: *mut std::ffi::c_void,
    visible: u8,
) {
    VISIBLE.store(visible != 0, Ordering::SeqCst);
}
#[no_mangle]
pub extern "system" fn Java_com_lelloman_talia_spike_LifecycleActivity_step(
    _: *mut std::ffi::c_void,
    _: *mut std::ffi::c_void,
    command: i32,
    port: i32,
) -> i32 {
    let result = std::panic::catch_unwind(|| {
        SESSION.with(|slot| {
            let mut slot = slot.borrow_mut();
            if slot.is_none() {
                *slot = Some(Session::new(port as u16));
            }
            if command==3 {
                let old=slot.take().unwrap();let mut fresh=Session::new(port as u16);
                fresh.outcomes=old.outcomes;fresh.signals=old.signals;*slot=Some(fresh);
            }
            let s = slot.as_mut().unwrap();
            s.sync_visibility();
            s.deadline();
            if s.active {
                match command {
                    1 => s
                        .guest()
                        .eval("vm.state.dirty=true;vm.action=()=>10;")
                        .unwrap(),
                    2 => s.guest().eval("writeValue(42);").unwrap(),
                    4 => s.guest().eval("throw Error('fixture internal error')").unwrap(),
                    5 => s.guest().eval("while(true){}").unwrap(),
                    6 => s.guest().eval("globalThis.buffers=[];for(let i=0;i<1000;i++)buffers.push(new ArrayBuffer(1024*1024));").unwrap(),
                    7 => {s.send("unsupported-probe",json!({}),None,None);},
                    _ => (),
                }
            }
            s.tick();
            s.report()
        })
    });
    let report = match result {
        Ok(report) => report,
        Err(error) => SESSION.with(|slot| {
            let mut slot = slot.borrow_mut();
            let s = slot.as_mut().unwrap();
            let diagnostic = error
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| error.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "internal runtime failure".into());
            s.fail(diagnostic.chars().take(512).collect());
            s.report()
        }),
    };
    std::fs::write(
        "/data/user_de/0/com.lelloman.talia.spike/files/lifecycle.json",
        report.to_string(),
    )
    .map(|_| 0)
    .unwrap_or(2)
}
#[no_mangle]
pub extern "system" fn Java_com_lelloman_talia_spike_LifecycleActivity_setFailureSignals(
    _: *mut std::ffi::c_void,
    _: *mut std::ffi::c_void,
    enabled: u8,
) {
    SIGNALS.store(enabled != 0, Ordering::SeqCst);
}
