mod android_service;
mod policy;
pub mod process;
use rquickjs::{Context, Function, Runtime};
use serde_json::{json, Value};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

const BRIDGE: &str = include_str!("../../shared/bridge.js");
static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);
const LIFECYCLE: &str = include_str!("../../shared/lifecycle.json");
const SUITE: &str = include_str!("../../shared/suite.js");

#[derive(Default)]
struct Host {
    value: f64,
    subscriptions: Vec<(u64, String)>,
    stalled: Vec<(u64, Value)>,
    next: u64,
}
impl Host {
    fn retire(&mut self, generation: u64) {
        self.subscriptions.retain(|(g, _)| *g != generation);
        self.stalled.retain(|(g, _)| *g != generation);
    }
}
struct Guest {
    generation: u64,
    rt: Runtime,
    ctx: Context,
    out: Rc<RefCell<VecDeque<String>>>,
    violation: Rc<RefCell<Option<String>>>,
    last_request: Cell<u64>,
}
impl Guest {
    fn new() -> Self {
        let rt = Runtime::new().unwrap();
        rt.set_memory_limit(16 * 1024 * 1024);
        rt.set_max_stack_size(512 * 1024);
        let until = Instant::now() + Duration::from_secs(3);
        rt.set_interrupt_handler(Some(Box::new(move || Instant::now() > until)));
        let ctx = Context::full(&rt).unwrap();
        let out = Rc::new(RefCell::new(VecDeque::new()));
        let violation = Rc::new(RefCell::new(None));
        ctx.with(|c| {
            let queue = out.clone();
            let failed = violation.clone();
            c.globals()
                .set(
                    "__send",
                    Function::new(c.clone(), move |raw: rquickjs::Value| {
                        if failed.borrow().is_some() {
                            return;
                        }
                        let s = match raw.as_string().and_then(|s| s.to_string().ok()) {
                            Some(s) => s,
                            None => {
                                *failed.borrow_mut() = Some("wire type".into());
                                return;
                            }
                        };
                        let error = if queue.borrow().len() >= policy::QUEUED {
                            Some("request queue limit".to_string())
                        } else {
                            policy::request(&s).err()
                        };
                        if let Some(error) = error {
                            *failed.borrow_mut() = Some(error);
                        } else {
                            queue.borrow_mut().push_back(s);
                        }
                    })
                    .unwrap(),
                )
                .unwrap();
            c.eval::<(), _>(BRIDGE).unwrap();
        });
        Self {
            generation: NEXT_GENERATION.fetch_add(1, Ordering::Relaxed),
            rt,
            ctx,
            out,
            violation,
            last_request: Cell::new(0),
        }
    }
    fn eval(&self, s: &str) -> Result<(), String> {
        if s.len() > policy::COMMAND_BYTES {
            *self.violation.borrow_mut() = Some("command size".into());
            return Err("command size".into());
        }
        if let Some(error) = self.violation.borrow().clone() {
            return Err(error);
        }
        let result = self.ctx.with(|c| {
            c.eval::<(), _>(s)
                .map_err(|e| format!("{e}: {:?}", c.catch()))
        });
        if let Some(error) = self.violation.borrow().clone() {
            Err(error)
        } else {
            result
        }
    }
    fn read(&self, expr: &str) -> Value {
        let s = self.ctx.with(|c| {
            c.eval::<String, _>(format!("JSON.stringify({expr})"))
                .unwrap()
        });
        serde_json::from_str(&s).unwrap()
    }
    fn deliver_for(&self, generation: u64, msg: Value) -> bool {
        if generation != self.generation {
            return false;
        }
        self.deliver(msg);
        true
    }
    fn deliver(&self, msg: Value) {
        let raw = serde_json::to_string(&msg.to_string()).unwrap();
        self.eval(&format!("__receive({raw})")).unwrap();
    }
    fn drive(&self, host: &mut Host) -> Value {
        self.try_drive(host).unwrap()
    }
    fn try_drive(&self, host: &mut Host) -> Result<Value, String> {
        let result = self.drive_inner(host);
        if let Err(error) = &result {
            *self.violation.borrow_mut() = Some(error.clone());
            self.out.borrow_mut().clear();
            host.retire(self.generation);
        }
        result
    }
    fn drive_inner(&self, host: &mut Host) -> Result<Value, String> {
        let until = Instant::now() + Duration::from_secs(4);
        loop {
            if Instant::now() >= until {
                return Err("host deadline exceeded".into());
            }
            if let Some(error) = self.violation.borrow().clone() {
                return Err(error);
            }
            let mut jobs = 0;
            while self.rt.is_job_pending() {
                self.rt
                    .execute_pending_job()
                    .map_err(|_| "pending job failed")?;
                jobs += 1;
                if jobs >= 10000 {
                    return Err("job budget exceeded".into());
                }
            }
            let request = self.out.borrow_mut().pop_front();
            if let Some(raw) = request {
                if let Some(error) = self.violation.borrow().clone() {
                    return Err(error);
                }
                let req = policy::request(&raw)?;
                let request_id = req["id"].as_u64().unwrap();
                if request_id <= self.last_request.get() {
                    return Err("request ID replay/order".into());
                }
                self.last_request.set(request_id);
                let id = req["id"].clone();
                let op = req["op"].as_str().unwrap();
                let val = req["value"].clone();
                let result =
                    match op {
                        "echo" | "delay" => val,
                        "read" => json!(host.value),
                        "write" => {
                            host.value = val.as_f64().unwrap();
                            json!(host.value)
                        }
                        "fail" => {
                            self.deliver(json!({"id":id,"error":"host failure"}));
                            continue;
                        }
                        "subscribe" => {
                            if host
                                .subscriptions
                                .iter()
                                .filter(|(g, _)| *g == self.generation)
                                .count()
                                >= policy::RESOURCES
                            {
                                return Err("subscription limit".into());
                            }
                            host.next += 1;
                            let sub = format!("s{}", host.next);
                            host.subscriptions.push((self.generation, sub.clone()));
                            json!(sub)
                        }
                        "unsubscribe" => {
                            if !host.subscriptions.iter().any(|(g, s)| {
                                *g == self.generation && Some(s.as_str()) == val.as_str()
                            }) {
                                return Err("subscription ownership".into());
                            }
                            host.subscriptions.retain(|(g, s)| {
                                *g != self.generation || Some(s.as_str()) != val.as_str()
                            });
                            Value::Null
                        }
                        "publish" => {
                            for (generation, sub) in &host.subscriptions {
                                self.deliver_for(*generation, json!({"event":sub,"value":val}));
                            }
                            Value::Null
                        }
                        "stall" => {
                            if host
                                .stalled
                                .iter()
                                .filter(|(g, _)| *g == self.generation)
                                .count()
                                >= policy::RESOURCES
                            {
                                return Err("stalled call limit".into());
                            }
                            host.stalled.push((self.generation, id));
                            continue;
                        }
                        "late" => {
                            let mut retained = Vec::new();
                            for (generation, old) in host.stalled.drain(..) {
                                if generation == self.generation {
                                    self.deliver_for(generation, json!({"id":old,"value":999}));
                                } else {
                                    retained.push((generation, old));
                                }
                            }
                            host.stalled = retained;
                            Value::Null
                        }
                        _ => panic!("unknown host capability"),
                    };
                let response = json!({"id":id,"value":result});
                self.deliver(response.clone());
                if op == "late" {
                    self.deliver(response);
                }
            } else {
                let report = self.read("report");
                if report["done"] == true {
                    assert!(report["error"].is_null(), "{report}");
                    return Ok(report);
                }
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    }
}
fn check_lifecycle() -> Value {
    let scripts: Value = serde_json::from_str(LIFECYCLE).unwrap();
    let mut host = Host::default();
    let survivor = Guest::new();
    survivor.eval(scripts["start"].as_str().unwrap()).unwrap();
    survivor.drive(&mut host);
    let old = Guest::new();
    old.eval(scripts["start"].as_str().unwrap()).unwrap();
    old.drive(&mut host);
    let old_generation = old.generation;
    let old_response = host
        .stalled
        .iter()
        .find(|(g, _)| *g == old_generation)
        .unwrap()
        .1
        .clone();
    let old_subscription = host
        .subscriptions
        .iter()
        .find(|(g, _)| *g == old_generation)
        .unwrap()
        .1
        .clone();
    assert_eq!(host.subscriptions.len(), 2);
    host.retire(old_generation);
    drop(old); // No guest cleanup/unsubscribe: host must retire resources.
    assert_eq!(host.subscriptions.len(), 1);
    assert!(host.stalled.iter().all(|(g, _)| *g != old_generation));
    let new = Guest::new();
    new.eval(scripts["start"].as_str().unwrap()).unwrap();
    new.drive(&mut host);
    let new_response = host
        .stalled
        .iter()
        .find(|(g, _)| *g == new.generation)
        .unwrap()
        .1
        .clone();
    assert_eq!(
        old_response, new_response,
        "IDs deliberately collide across reload"
    );
    let new_subscription = host
        .subscriptions
        .iter()
        .find(|(g, _)| *g == new.generation)
        .unwrap()
        .1
        .clone();
    assert!(!new.deliver_for(old_generation, json!({"id":old_response,"value":"stale"})));
    assert!(!new.deliver_for(old_generation, json!({"event":new_subscription,"value":99})));
    assert!(!new.deliver_for(old_generation, json!({"event":old_subscription,"value":99})));
    new.drive(&mut host);
    assert_eq!(new.read("[reply,events.length]"), json!([null, 0]));
    new.deliver_for(new.generation, json!({"id":new_response,"value":"new"}));
    new.deliver_for(new.generation, json!({"event":new_subscription,"value":3}));
    new.drive(&mut host);
    new.eval(scripts["verify"].as_str().unwrap()).unwrap();
    let survivor_subscription = host
        .subscriptions
        .iter()
        .find(|(g, _)| *g == survivor.generation)
        .unwrap()
        .1
        .clone();
    survivor.deliver_for(
        survivor.generation,
        json!({"event":survivor_subscription,"value":8}),
    );
    assert_eq!(survivor.read("events"), json!([8]));
    host.retire(new.generation);
    host.retire(survivor.generation);
    assert!(host.subscriptions.is_empty() && host.stalled.is_empty());
    json!({"forced_reload_cleanup":true,"stale_response_rejected":true,"stale_event_rejected":true,"colliding_request_ids":true,"other_instance_survives":true})
}
fn check_policy() -> Value {
    let scripts: Value = serde_json::from_str(LIFECYCLE).unwrap();
    let mut host = Host::default();
    let survivor = Guest::new();
    survivor.eval(scripts["start"].as_str().unwrap()).unwrap();
    survivor.drive(&mut host);
    let survivor_sub = host.subscriptions[0].1.clone();
    let cases = [
        ("malformed JSON", "__send('{')".to_string()),
        ("non-string wire", "try{__send({})}catch{}".into()),
        ("oversized command", "x".repeat(policy::COMMAND_BYTES + 1)),
        ("wrong envelope", "__send('[]')".into()),
        ("invalid write", "__send(JSON.stringify({id:1,op:'write',value:'99'}))".into()),
        ("extra routing field", "__send(JSON.stringify({id:1,op:'write',value:99,generation:1}))".into()),
        ("unsafe ID", "__send(JSON.stringify({id:9007199254740992,op:'write',value:99}))".into()),
        ("unknown capability", "engine.call('fetch')".into()),
        ("oversized UTF8", "engine.call('echo','é'.repeat(17000))".into()),
        ("deep JSON", "let x=null;for(let i=0;i<20;i++)x=[x];engine.call('echo',x)".into()),
        ("wide JSON", "engine.call('echo',Array(2100).fill(0))".into()),
        ("caught flood", "for(let i=1;i<1000;i++){try{__send(JSON.stringify({id:i,op:'echo',value:null}))}catch{}}".into()),
        ("replayed ID", "__send(JSON.stringify({id:1,op:'echo',value:null}));__send(JSON.stringify({id:1,op:'write',value:99}))".into()),
        ("subscription budget", "for(let i=0;i<17;i++)engine.call('subscribe','value')".into()),
        ("stalled budget", "for(let i=0;i<17;i++)engine.call('stall')".into()),
        ("subscription ownership", format!("engine.call('unsubscribe',{})", json!(survivor_sub))),
    ];
    let mut checked = Vec::new();
    for (name, source) in cases {
        let guest = Guest::new();
        let _ = guest.eval(&source);
        assert!(guest.try_drive(&mut host).is_err(), "{name}");
        assert!(guest.eval("1+1").is_err(), "poisoned guest reused");
        assert_eq!(host.value, 0.0);
        assert_eq!(host.subscriptions.len(), 1);
        assert_eq!(host.stalled.len(), 1);
        assert!(guest.out.borrow().is_empty());
        survivor.deliver(json!({"event":survivor_sub,"value":1}));
        checked.push(name);
        assert_eq!(survivor.read("events.length"), json!(checked.len()));
    }
    // Host validation still applies when bypassing the adapter queue.
    let guest = Guest::new();
    guest.eval(scripts["start"].as_str().unwrap()).unwrap();
    guest.drive(&mut host);
    guest.out.borrow_mut().push_back("null".into());
    assert!(guest.try_drive(&mut host).is_err());
    assert_eq!(host.subscriptions.len(), 1);
    assert_eq!(host.stalled.len(), 1);
    let guest = Guest::new();
    let prefix = json!({"id":1,"op":"echo","value":""}).to_string().len();
    let exact =
        json!({"id":1,"op":"echo","value":"x".repeat(policy::WIRE_BYTES-prefix)}).to_string();
    assert_eq!(exact.len(), policy::WIRE_BYTES);
    assert!(policy::request(&exact).is_ok());
    assert!(policy::request(&(exact + " ")).is_err());
    guest
        .eval("for(let i=0;i<16;i++)engine.call('subscribe','value');report.done=true;")
        .unwrap();
    guest.drive(&mut host);
    assert_eq!(host.subscriptions.len(), 17);
    guest
        .eval("for(let i=0;i<16;i++)engine.call('stall');")
        .unwrap();
    guest.drive(&mut host);
    assert_eq!(host.stalled.len(), 17);
    host.retire(guest.generation);
    host.retire(survivor.generation);
    json!({"passed":true,"cases":checked,"parent_validation":true,"exact_boundaries":true,"resources_empty":host.subscriptions.is_empty() && host.stalled.is_empty(),"wire_bytes":policy::WIRE_BYTES,"queued_requests":policy::QUEUED,"subscriptions":policy::RESOURCES,"stalled_calls":policy::RESOURCES})
}
pub fn run() -> Value {
    let start = Instant::now();
    let lifecycle = check_lifecycle();
    let policy = check_policy();
    let mut host = Host::default();
    let mut runs = Vec::new();
    for _ in 0..20 {
        let g = Guest::new();
        g.eval(SUITE).unwrap();
        let report = g.drive(&mut host);
        assert!(host.subscriptions.is_empty());
        g.eval("vm.state.value=9; vm.action=()=>10;").unwrap();
        assert_eq!(g.read("[vm.state.value,vm.action()]"), json!([9, 10]));
        runs.push(report);
        // Host owns subscription teardown even if guest cleanup is skipped.
        host.retire(g.generation);
    }
    let clean = Guest::new();
    assert_eq!(clean.read("[vm.state.value,vm.action()]"), json!([1, 2]));
    assert_eq!(host.value, 7.0);
    let runaway = Guest::new();
    let deadline = Instant::now() + Duration::from_millis(50);
    runaway
        .rt
        .set_interrupt_handler(Some(Box::new(move || Instant::now() > deadline)));
    let t = Instant::now();
    assert!(runaway.eval("while(true){}").is_err());
    let interruption_ms = t.elapsed().as_millis();
    let allocation = Guest::new();
    assert!(allocation.eval("globalThis.buffers=[]; for(let i=0;i<1000;i++) buffers.push(new ArrayBuffer(1024*1024));").is_err());
    let after = Guest::new();
    assert_eq!(after.read("1+1"), json!(2));
    json!({"host":std::env::consts::OS,"arch":std::env::consts::ARCH,"cycles":runs.len(),"lifecycle":lifecycle,"policy":policy,"checks":runs[0]["checks"],"execution":runs[0]["execution"],"fresh_context":true,"engine_effect_survives":true,"interruption_ms":interruption_ms,"heap_limit":true,"elapsed_ms":start.elapsed().as_millis()})
}

// Minimal JNI entry: P0 host embedding only, no Java object crosses the boundary.
#[no_mangle]
pub extern "system" fn Java_com_lelloman_talia_spike_MainActivity_runNative(
    _env: *mut std::ffi::c_void,
    _class: *mut std::ffi::c_void,
) -> i32 {
    match std::panic::catch_unwind(run) {
        Ok(report) => {
            match std::fs::write(
                "/data/user_de/0/com.lelloman.talia.spike/files/result.json",
                report.to_string(),
            ) {
                Ok(()) => 0,
                Err(_) => 2,
            }
        }
        Err(_) => 1,
    }
}
