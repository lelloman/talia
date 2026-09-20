use rquickjs::{Context, Runtime};
use std::time::{Duration, Instant};
/// Each invocation owns a bounded context; no platform/network globals are installed.
pub struct Script {
    pub ctx: Context,
    pub rt: Runtime,
    pub deadline: Option<Instant>,
}
impl Script {
    pub fn new() -> Result<Self, String> {
        let rt = Runtime::new().map_err(|e| e.to_string())?;
        rt.set_memory_limit(16 * 1024 * 1024);
        rt.set_max_stack_size(512 * 1024);
        let ctx = Context::full(&rt).map_err(|e| e.to_string())?;
        let s = Self {
            ctx,
            rt,
            deadline: None,
        };
        s.eval(include_str!("../shared/value.js"))?;
        Ok(s)
    }
    pub fn budget(&self) {
        let end = (Instant::now() + Duration::from_millis(100)).min(
            self.deadline
                .unwrap_or_else(|| Instant::now() + Duration::from_secs(5)),
        );
        self.rt
            .set_interrupt_handler(Some(Box::new(move || Instant::now() > end)));
    }
    pub fn eval(&self, src: &str) -> Result<(), String> {
        if src.len() > 262144 {
            return Err("script size limit".into());
        }
        self.budget();
        self.ctx.with(|c| {
            c.eval::<(), _>(src).map_err(|e| {
                let ex = c.catch();
                ex.as_object()
                    .and_then(|o| o.get::<_, String>("message").ok())
                    .unwrap_or_else(|| e.to_string())
            })
        })
    }
    pub fn string(&self, src: &str) -> Result<String, String> {
        self.budget();
        self.ctx
            .with(|c| c.eval::<String, _>(src).map_err(|e| e.to_string()))
    }
    pub fn drain(&self) -> Result<(), String> {
        self.budget();
        for _ in 0..10000 {
            if !self.rt.is_job_pending() {
                return Ok(());
            }
            self.rt
                .execute_pending_job()
                .map_err(|_| "script job failed")?;
        }
        Err("job budget".into())
    }
}
