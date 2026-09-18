use rquickjs::{Context, Function, Runtime};
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    collections::VecDeque,
    rc::Rc,
    time::{Duration, Instant},
};

const BRIDGE: &str = include_str!("../../shared/bridge.js");
const SUITE: &str = include_str!("../../shared/suite.js");

#[derive(Default)]
struct Host {
    value: i64,
    subscriptions: Vec<String>,
    stalled: Vec<Value>,
    next: u64,
}
struct Guest {
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
        Self { rt, ctx, out }
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
                        host.subscriptions.push(sub.clone());
                        json!(sub)
                    }
                    "unsubscribe" => {
                        host.subscriptions
                            .retain(|s| Some(s.as_str()) != val.as_str());
                        Value::Null
                    }
                    "publish" => {
                        for sub in &host.subscriptions {
                            self.deliver(json!({"event":sub,"value":val}));
                        }
                        Value::Null
                    }
                    "stall" => {
                        host.stalled.push(id);
                        continue;
                    }
                    "late" => {
                        for old in host.stalled.drain(..) {
                            self.deliver(json!({"id":old,"value":999}));
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
pub fn run() -> Value {
    let start = Instant::now();
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
        host.subscriptions.clear();
        host.stalled.clear();
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
    json!({"host":std::env::consts::OS,"arch":std::env::consts::ARCH,"cycles":runs.len(),"checks":runs[0]["checks"],"fresh_context":true,"engine_effect_survives":true,"interruption_ms":interruption_ms,"heap_limit":true,"elapsed_ms":start.elapsed().as_millis()})
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
