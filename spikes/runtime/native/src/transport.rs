//! Real loopback HTTP adapter used unchanged by Linux and the Android JNI shell.
use rquickjs::{Context, Function, Runtime};
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    collections::VecDeque,
    io::{Read, Write},
    net::TcpStream,
    rc::Rc,
    sync::mpsc,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
const CLIENT: &str = include_str!("../../../transport/shared/client.js");
const SUITE: &str = include_str!("../../../transport/shared/suite.js");
pub(crate) fn http(port: u16, request: &Value) -> Result<Value, String> {
    let mut socket = TcpStream::connect_timeout(
        &format!("127.0.0.1:{port}").parse().unwrap(),
        Duration::from_secs(2),
    )
    .map_err(|e| e.to_string())?;
    socket
        .set_read_timeout(Some(Duration::from_secs(8)))
        .unwrap();
    socket
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let body = request.to_string();
    write!(socket,"POST /rpc HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",body.len(),body).map_err(|e|e.to_string())?;
    let mut response = Vec::new();
    socket
        .take(65537)
        .read_to_end(&mut response)
        .map_err(|e| e.to_string())?;
    if response.len() > 65536 {
        return Err("response budget".into());
    }
    let response = String::from_utf8(response).map_err(|e| e.to_string())?;
    let (headers, body) = response
        .split_once("\r\n\r\n")
        .ok_or("incomplete HTTP response")?;
    if !headers.starts_with("HTTP/1.1 200 ") {
        return Err("HTTP failure".into());
    }
    // This deliberately narrow test adapter supports the server's bounded JSON responses,
    // not general HTTP (TLS, redirects, streaming chunked responses or pooled connections).
    let length = headers.lines().find_map(|line| {
        line.to_ascii_lowercase()
            .strip_prefix("content-length: ")
            .and_then(|v| v.parse::<usize>().ok())
    });
    if length != Some(body.len()) {
        return Err("incomplete HTTP body".into());
    }
    let reply: Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
    for field in ["channel", "epoch", "id"] {
        if reply[field] != request[field] {
            return Err("response route mismatch".into());
        }
    }
    Ok(reply)
}
fn eval(ctx: &Context, script: &str) -> Result<(), String> {
    ctx.with(|c| {
        c.eval::<(), _>(script)
            .map_err(|e| format!("{e}: {:?}", c.catch()))
    })
}
fn report(ctx: &Context) -> Value {
    ctx.with(|c| {
        let raw: String = c.eval("JSON.stringify(report)").unwrap();
        serde_json::from_str(&raw).unwrap()
    })
}
fn deadline(rt: &Runtime) {
    let end = Instant::now() + Duration::from_secs(1);
    rt.set_interrupt_handler(Some(Box::new(move || Instant::now() > end)));
}
pub fn run(port: u16) -> Value {
    let begin = Instant::now();
    let rt = Runtime::new().unwrap();
    rt.set_memory_limit(16 * 1024 * 1024);
    rt.set_max_stack_size(512 * 1024);
    let ctx = Context::full(&rt).unwrap();
    let queue = Rc::new(RefCell::new(VecDeque::<String>::new()));
    ctx.with(|c| {
        let q = queue.clone();
        c.globals()
            .set(
                "__send",
                Function::new(c.clone(), move |raw: String| {
                    assert!(
                        raw.len() <= 32768 && q.borrow().len() < 32,
                        "wire/queue budget"
                    );
                    q.borrow_mut().push_back(raw);
                })
                .unwrap(),
            )
            .unwrap();
    });
    deadline(&rt);
    eval(&ctx, CLIENT).unwrap();
    eval(&ctx, SUITE).unwrap();
    let session = format!(
        "native-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let (tx, rx) = mpsc::channel();
    let mut active = 0;
    let mut peak = 0;
    let mut workers = Vec::new();
    let mut dispatched = 0;
    loop {
        assert!(
            begin.elapsed() < Duration::from_secs(90),
            "transport suite deadline: {}",
            report(&ctx)
        );
        deadline(&rt);
        for _ in 0..10000 {
            if !rt.is_job_pending() {
                break;
            }
            rt.execute_pending_job().unwrap();
        }
        assert!(!rt.is_job_pending(), "job budget");
        while let Some(raw) = queue.borrow_mut().pop_front() {
            assert!(active < 32, "HTTP concurrency budget");
            dispatched += 1;
            assert!(dispatched <= 2000, "total request budget");
            let mut request: Value = serde_json::from_str(&raw).unwrap();
            assert_eq!(request.as_object().unwrap().len(), 5);
            for field in ["channel", "epoch", "id", "op", "args"] {
                assert!(request.get(field).is_some());
            }
            assert!(matches!(
                request["op"].as_str(),
                Some("read" | "watch" | "action" | "status" | "cancel" | "test" | "write-anything")
            ));
            request["session"] = json!(session);
            let tx = tx.clone();
            active += 1;
            peak = peak.max(active);
            workers.push(thread::spawn(move||{
                let reply=http(port,&request).unwrap_or_else(|_|json!({"channel":request["channel"],"epoch":request["epoch"],"id":request["id"],"transportError":true}));
                tx.send(reply).ok();
            }));
        }
        while let Ok(reply) = rx.try_recv() {
            active -= 1;
            deadline(&rt);
            eval(&ctx, &format!("__receive({})", json!(reply.to_string()))).unwrap();
        }
        let current = report(&ctx);
        if current["done"] == true
            && active == 0
            && queue.borrow().is_empty()
            && !rt.is_job_pending()
        {
            break;
        }
        thread::sleep(Duration::from_millis(1));
    }
    for worker in workers {
        worker.join().unwrap();
    }
    let mut result = report(&ctx);
    result["host"] = json!(std::env::consts::OS);
    result["arch"] = json!(std::env::consts::ARCH);
    result["elapsed_ms"] = json!(begin.elapsed().as_millis());
    result["peak_http_requests"] = json!(peak);
    result["http_requests_remaining"] = json!(active);
    result
}
#[no_mangle]
pub extern "system" fn Java_com_lelloman_talia_spike_TransportActivity_runTransport(
    _env: *mut std::ffi::c_void,
    _class: *mut std::ffi::c_void,
    port: i32,
) -> i32 {
    match std::panic::catch_unwind(|| run(u16::try_from(port).unwrap())) {
        Ok(report) => std::fs::write(
            "/data/user_de/0/com.lelloman.talia.spike/files/transport.json",
            report.to_string(),
        )
        .map(|_| 0)
        .unwrap_or(2),
        Err(_) => 1,
    }
}
