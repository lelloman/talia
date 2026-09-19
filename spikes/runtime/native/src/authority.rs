use rquickjs::{Context, Runtime};
use serde_json::{json, Value};
// A separate trusted context; authored code never receives this context or its objects.
pub struct Authority {
    ctx: Context,
    _rt: Runtime,
}
impl Authority {
    pub fn new() -> Self {
        let rt = Runtime::new().unwrap();
        rt.set_memory_limit(4 * 1024 * 1024);
        let ctx = Context::full(&rt).unwrap();
        ctx.with(|c| {
            c.eval::<(), _>(include_str!("../../host/execution.js"))
                .unwrap();
            c.eval::<(), _>("globalThis.authority=new ExecutionAuthority();")
                .unwrap();
        });
        Self { ctx, _rt: rt }
    }
    pub fn call(&self, method: &str, args: Value) -> Result<Value, String> {
        self.ctx.with(|c| {
            let source=format!("JSON.stringify((()=>{{try{{return {{value:authority[{}](...{})??null}};}}catch(e){{return {{error:e.message}};}}}})())",json!(method),args);
            let raw=c.eval::<String,_>(source).map_err(|e|e.to_string())?;
            let result:Value=serde_json::from_str(&raw).map_err(|e|e.to_string())?;
            if let Some(error)=result["error"].as_str(){Err(error.into())}else{Ok(result["value"].clone())}
        })
    }
    pub fn test(&self) -> Value {
        self.ctx.with(|c| {
            c.eval::<(), _>(include_str!("../../host/execution-suite.js"))
                .unwrap();
            serde_json::from_str(
                &c.eval::<String, _>("JSON.stringify(testAuthority())")
                    .unwrap(),
            )
            .unwrap()
        })
    }
}
