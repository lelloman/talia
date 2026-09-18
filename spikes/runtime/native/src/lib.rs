use rquickjs::{Context, Function, Runtime};
use serde_json::{json, Value};
use std::{
    cell::RefCell,
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
    value: i64,
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
        ctx.with(|c| {
            let queue = out.clone();
            c.globals()
                .set(
                    "__send",
                    Function::new(c.clone(), move |s: String| {
                        queue.borrow_mut().push_back(s);
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
        }
    }
    fn eval(&self, s: &str) -> Result<(), String> {
        self.ctx.with(|c| {
            c.eval::<(), _>(s)
                .map_err(|e| format!("{e}: {:?}", c.catch()))
        })
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
        let until = Instant::now() + Duration::from_secs(4);
        loop {
            assert!(Instant::now() < until, "host deadline exceeded");
            let mut jobs = 0;
            while self.rt.is_job_pending() {
                self.rt.execute_pending_job().expect("pending job failed");
                jobs += 1;
                assert!(jobs < 10000, "job budget exceeded");
            }
            let request = self.out.borrow_mut().pop_front();
            if let Some(raw) = request {
                let req: Value = serde_json::from_str(&raw).unwrap();
                let id = req["id"].clone();
                let op = req["op"].as_str().unwrap();
                let val = req["value"].clone();
                let result = match op {
                    "echo" | "delay" => val,
                    "read" => json!(host.value),
                    "write" => {
                        host.value = val.as_i64().unwrap();
                        json!(host.value)
                    }
                    "fail" => {
                        self.deliver(json!({"id":id,"error":"host failure"}));
                        continue;
                    }
                    "subscribe" => {
                        host.next += 1;
                        let sub = format!("s{}", host.next);
                        host.subscriptions.push((self.generation, sub.clone()));
                        json!(sub)
                    }
                    "unsubscribe" => {
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
                        host.stalled.push((self.generation, id));
                        continue;
                    }
                    "late" => {
                        for (generation, old) in host.stalled.drain(..) {
                            self.deliver_for(generation, json!({"id":old,"value":999}));
                        }
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
                    return report;
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
pub fn run() -> Value {
    let start = Instant::now();
    let lifecycle = check_lifecycle();
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
    assert_eq!(host.value, 7);
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
    json!({"host":std::env::consts::OS,"arch":std::env::consts::ARCH,"cycles":runs.len(),"lifecycle":lifecycle,"checks":runs[0]["checks"],"fresh_context":true,"engine_effect_survives":true,"interruption_ms":interruption_ms,"heap_limit":true,"elapsed_ms":start.elapsed().as_millis()})
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
