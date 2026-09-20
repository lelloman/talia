//! Bounded QuickJS host. UI resolution and untrusted VM use separate contexts.
use rquickjs::{Context, Function, Runtime};
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};
struct Guest {
    ctx: Context,
    rt: Runtime,
    out: Rc<RefCell<Vec<String>>>,
    poison: Rc<RefCell<bool>>,
}
impl Guest {
    fn new() -> Result<Self, String> {
        let rt = Runtime::new().map_err(|e| e.to_string())?;
        rt.set_memory_limit(16 * 1024 * 1024);
        rt.set_max_stack_size(512 * 1024);
        let ctx = Context::full(&rt).map_err(|e| e.to_string())?;
        let out = Rc::new(RefCell::new(Vec::new()));
        let poison = Rc::new(RefCell::new(false));
        let q = out.clone();
        let p = poison.clone();
        ctx.with(|c| {
            c.globals().set(
                "__send",
                Function::new(c.clone(), move |raw: String| {
                    let valid = raw.len() <= 32768
                        && q.borrow().len() < 64
                        && serde_json::from_str::<Value>(&raw).ok().is_some_and(|v| {
                            v.as_object().is_some_and(|o| o.len() == 3)
                                && v["id"]
                                    .as_u64()
                                    .is_some_and(|i| i > 0 && i <= 9_007_199_254_740_991)
                                && ["read", "write", "subscribe", "unsubscribe", "run"]
                                    .contains(&v["op"].as_str().unwrap_or(""))
                        });
                    if valid && !*p.borrow() {
                        q.borrow_mut().push(raw)
                    } else {
                        *p.borrow_mut() = true;
                    }
                })
                .unwrap(),
            )
        })
        .map_err(|e| e.to_string())?;
        Ok(Self {
            ctx,
            rt,
            out,
            poison,
        })
    }
    fn eval(&mut self, source: &str) -> Result<Value, String> {
        if source.len() > 262144 || *self.poison.borrow() {
            return Err("source limit or retired guest".into());
        }
        let until = Instant::now() + Duration::from_millis(500);
        self.rt
            .set_interrupt_handler(Some(Box::new(move || Instant::now() > until)));
        let value = self.ctx.with(|c| {
            c.eval::<String, _>(source).map_err(|error| {
                let exception = c.catch();
                let detail = exception
                    .as_object()
                    .and_then(|o| o.get::<_, String>("message").ok())
                    .or_else(|| exception.as_string().and_then(|s| s.to_string().ok()));
                format!("script: {}", detail.unwrap_or_else(|| error.to_string()))
            })
        })?;
        for _ in 0..10000 {
            if !self.rt.is_job_pending() {
                break;
            }
            self.rt
                .execute_pending_job()
                .map_err(|_| "pending job failure")?;
        }
        if self.rt.is_job_pending() || *self.poison.borrow() {
            return Err("guest budget/bridge violation".into());
        }
        if value.len() > 262144 {
            return Err("response size limit".into());
        }
        let messages: Vec<Value> = self
            .out
            .borrow_mut()
            .drain(..)
            .map(|s| serde_json::from_str(&s).unwrap())
            .collect();
        Ok(json!({"value":value,"out":messages}))
    }
}
thread_local! {static VM:RefCell<Option<Guest>>=const{RefCell::new(None)};static UI:RefCell<Option<Guest>>=const{RefCell::new(None)};}
pub fn command(source: &str, ui: bool, reset: bool) -> String {
    let slot = if ui { &UI } else { &VM };
    slot.with(|s| {
        let mut slot = s.borrow_mut();
        if reset || slot.is_none() {
            match Guest::new() {
                Ok(g) => *slot = Some(g),
                Err(e) => return json!({"error":e}).to_string(),
            }
        }
        match slot.as_mut().unwrap().eval(source) {
            Ok(v) => v.to_string(),
            Err(e) => {
                *slot = None;
                json!({"error":e}).to_string()
            }
        }
    })
}
#[no_mangle]
pub extern "system" fn Java_com_lelloman_talia_dashboard_MainActivity_evaluate(
    mut env: jni::JNIEnv,
    _class: jni::objects::JClass,
    source: jni::objects::JString,
    ui: jni::sys::jboolean,
    reset: jni::sys::jboolean,
) -> jni::sys::jstring {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let text: String = env.get_string(&source).map_err(|e| e.to_string())?.into();
        Ok::<_, String>(command(&text, ui != 0, reset != 0))
    }));
    let text = match result {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => json!({"error":e}).to_string(),
        Err(_) => "{\"error\":\"native runtime failure\"}".into(),
    };
    env.new_string(text)
        .map(|s| s.into_raw())
        .unwrap_or(std::ptr::null_mut())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn portable_vm() {
        let lib = include_str!("../../shared/vm.js");
        let example = include_str!("../../examples/monitor.vm.js");
        let result: Value = serde_json::from_str(&command(
            &format!("{lib}\n{example}\nTaliaVM.start({{}});'ok';"),
            false,
            true,
        ))
        .unwrap();
        assert_eq!(result["out"][0]["op"], "subscribe");
        let result: Value = serde_json::from_str(&command(
            "TaliaVM.dispatch('controls',{target:'x',value:null});'ok';",
            false,
            false,
        ))
        .unwrap();
        assert!(result.get("error").is_none());
        let result: Value =
            serde_json::from_str(&command("JSON.stringify(TaliaVM.snapshot())", false, false))
                .unwrap();
        assert!(result["value"].as_str().unwrap().contains("controlsScreen"));
    }
    #[test]
    fn bounded_and_isolated() {
        assert!(command("while(true){};''", false, true).contains("error"));
        assert!(command(
            "__send(JSON.stringify({id:1,op:'file',value:null}));''",
            false,
            true
        )
        .contains("error"));
        assert!(command("globalThis.secret=42;'ok'", false, true).contains("ok"));
        assert!(command("String(typeof secret)", true, true).contains("undefined"));
    }
}

#[no_mangle]
pub extern "system" fn Java_com_lelloman_talia_dashboard_MainActivity_retire(
    _env: jni::JNIEnv,
    _class: jni::objects::JClass,
) {
    VM.with(|s| *s.borrow_mut() = None);
    UI.with(|s| *s.borrow_mut() = None);
}
