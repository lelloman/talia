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
    guest: Guest,
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
        let (tx, rx) = mpsc::channel();
        Self {
            guest,
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
        let end = Instant::now() + Duration::from_millis(100);
        self.guest
            .rt
            .set_interrupt_handler(Some(Box::new(move || Instant::now() > end)));
    }
    fn sync_visibility(&mut self) {
        let visible = VISIBLE.load(Ordering::SeqCst);
        if visible == self.active {
            return;
        }
        self.epoch += 1;
        self.active = visible;
        self.guest.out.borrow_mut().clear();
        if visible {
            self.deadline();
            self.guest.eval("engine.cancelPending();").unwrap();
            self.send("read", json!({"tag":"resume"}), None, None);
            for id in self.outcomes.keys().cloned().collect::<Vec<_>>() {
                self.send("status", json!({"actionId":id}), None, Some(id));
            }
        }
    }
    fn tick(&mut self) {
        self.sync_visibility();
        while let Ok(reply) = self.rx.try_recv() {
            self.pending -= 1;
            if !self.active || reply.epoch != self.epoch {
                self.ignored += 1;
                continue;
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
                self.guest.deliver(if reply.value.get("error").is_some() {
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
        self.guest.eval("vm.state.ticks++;").unwrap();
        for _ in 0..1000 {
            if !self.guest.rt.is_job_pending() {
                break;
            }
            self.guest.rt.execute_pending_job().unwrap();
        }
        assert!(!self.guest.rt.is_job_pending(), "job budget");
        loop {
            let raw = self.guest.out.borrow_mut().pop_front();
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
                    let action =
                        format!("a-{}-{}-{}", std::process::id(), self.guest.generation, id);
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
        self.local = self.guest.read("vm.state");
        if self.pending == 0 {
            self.send("read", json!({"tag":"subscription"}), None, None);
        }
    }
    fn report(&self) -> Value {
        json!({"active":self.active,"epoch":self.epoch,"local":self.local,"snapshot":self.snapshot,"outcomes":self.outcomes,"pending":self.pending,"ignored":self.ignored,"subscriptions":if self.active {1}else{0}})
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
            let s = slot.as_mut().unwrap();
            s.sync_visibility();
            s.deadline();
            if s.active {
                match command {
                    1 => s
                        .guest
                        .eval("vm.state.dirty=true;vm.action=()=>10;")
                        .unwrap(),
                    2 => s.guest.eval("writeValue(42);").unwrap(),
                    _ => (),
                }
            }
            s.tick();
            s.report()
        })
    });
    match result {
        Ok(report) => std::fs::write(
            "/data/user_de/0/com.lelloman.talia.spike/files/lifecycle.json",
            report.to_string(),
        )
        .map(|_| 0)
        .unwrap_or(2),
        Err(_) => 1,
    }
}
