//! Linux-only crash-containment experiment. Not a production IPC protocol.
use crate::{policy, Guest, Host, BRIDGE, LIFECYCLE};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

pub fn worker() {
    let guest = Guest::new();
    let stdin = std::io::stdin();
    let mut reader = stdin.lock();
    loop {
        let mut bytes = Vec::new();
        reader
            .by_ref()
            .take((policy::COMMAND_BYTES + 1) as u64)
            .read_until(b'\n', &mut bytes)
            .unwrap();
        if bytes.is_empty() {
            return;
        }
        assert!(bytes.len() <= policy::COMMAND_BYTES && bytes.last() == Some(&b'\n'));
        let command: Value = serde_json::from_slice(&bytes).unwrap();
        let value = match command["op"].as_str().unwrap() {
            "eval" => {
                guest.eval(command["source"].as_str().unwrap()).unwrap();
                Value::Null
            }
            "deliver" => {
                guest.deliver(command["message"].clone());
                Value::Null
            }
            "read" => guest.read(command["source"].as_str().unwrap()),
            "suite" => crate::run(),
            "abort" => std::process::abort(), // Trusted harness injection, never a guest capability.
            "hang" => loop {
                std::thread::sleep(Duration::from_secs(60));
            },
            _ => panic!("invalid harness command"),
        };
        let mut jobs = 0;
        while guest.rt.is_job_pending() {
            guest.rt.execute_pending_job().unwrap();
            jobs += 1;
            assert!(jobs < 10000);
        }
        let requests: Vec<String> = guest.out.borrow_mut().drain(..).collect();
        let response = json!({"value":value,"requests":requests}).to_string();
        assert!(response.len() < policy::COMMAND_BYTES);
        println!("{response}");
        std::io::stdout().flush().unwrap();
    }
}
struct Process {
    child: Child,
    input: ChildStdin,
    output: Receiver<Option<Value>>,
    reader: Option<std::thread::JoinHandle<()>>,
}
impl Process {
    fn spawn() -> Self {
        // Disable core dumps only for the deliberately crashing test child.
        let mut child = Command::new("sh")
            .args(["-c", "ulimit -c 0; exec \"$@\"", "talia-spike"])
            .arg(std::env::current_exe().unwrap())
            .arg("--process-worker")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, output) = mpsc::sync_channel(1);
        let reader = std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut bytes = Vec::new();
                let value = match reader
                    .by_ref()
                    .take((policy::COMMAND_BYTES + 1) as u64)
                    .read_until(b'\n', &mut bytes)
                {
                    Ok(n)
                        if n > 0 && n <= policy::COMMAND_BYTES && bytes.last() == Some(&b'\n') =>
                    {
                        serde_json::from_slice(&bytes).ok()
                    }
                    _ => None,
                };
                let done = value.is_none();
                if tx.send(value).is_err() || done {
                    break;
                }
            }
        });
        Self {
            child,
            input,
            output,
            reader: Some(reader),
        }
    }
    fn send(&mut self, value: Value) {
        let text = value.to_string();
        assert!(text.len() < policy::COMMAND_BYTES);
        writeln!(self.input, "{text}").unwrap();
        self.input.flush().unwrap();
    }
    fn receive(&self, timeout: Duration) -> Result<Value, String> {
        self.output
            .recv_timeout(timeout)
            .map_err(|_| "deadline/disconnect".to_string())?
            .ok_or("child exit/invalid response".into())
    }
    fn call(&mut self, value: Value) -> Value {
        self.send(value);
        self.receive(Duration::from_secs(3)).unwrap()
    }
    fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        // No unsolicited messages in this sequential protocol: at most one result plus EOF.
        while self.output.try_recv().is_ok() {}
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        self.stop();
        if let Some(reader) = self.reader.take() {
            reader.join().unwrap();
        }
    }
}
fn subscribe(process: &mut Process, host: &mut Host, generation: u64) {
    let scripts: Value = serde_json::from_str(LIFECYCLE).unwrap();
    let result = process.call(json!({"op":"eval","source":scripts["start"]}));
    let request = policy::request(result["requests"][0].as_str().unwrap()).unwrap();
    assert_eq!(request["op"], "subscribe");
    let sub = format!("s{generation}");
    host.subscriptions.push((generation, sub.clone()));
    let result = process.call(json!({"op":"deliver","message":{"id":request["id"],"value":sub}}));
    let request = policy::request(result["requests"][0].as_str().unwrap()).unwrap();
    assert_eq!(request["op"], "stall");
    host.stalled.push((generation, request["id"].clone()));
}
pub fn check() -> Value {
    assert_eq!(
        std::env::consts::OS,
        "linux",
        "process experiment requires Linux"
    );
    let mut host = Host::default();
    let mut survivor = Process::spawn();
    subscribe(&mut survivor, &mut host, 1);
    let mut outcomes = Vec::new();
    for (generation, fault) in [(2, "abort"), (3, "hang")] {
        let mut victim = Process::spawn();
        subscribe(&mut victim, &mut host, generation);
        victim.send(json!({"op":fault}));
        assert_eq!(
            survivor.call(json!({"op":"read","source":"1+1"}))["value"],
            2
        );
        assert!(victim.receive(Duration::from_millis(200)).is_err());
        if fault == "abort" {
            use std::os::unix::process::ExitStatusExt;
            assert_eq!(victim.child.wait().unwrap().signal(), Some(6));
        }
        victim.stop();
        host.retire(generation);
        assert_eq!(host.subscriptions.len(), 1);
        assert_eq!(host.stalled.len(), 1);
        survivor.call(json!({"op":"deliver","message":{"event":"s1","value":generation}}));
        assert_eq!(
            survivor.call(json!({"op":"read","source":"events.length"}))["value"],
            generation - 1
        );
        let mut replacement = Process::spawn();
        assert_eq!(
            replacement.call(json!({"op":"read","source":"[vm.state.value,vm.action()]"}))["value"],
            json!([1, 2])
        );
        replacement.call(json!({"op":"eval","source":BRIDGE}));
        let recovered = replacement.call(json!({"op":"suite"}));
        assert_eq!(recovered["value"]["cycles"], 20);
        assert!(recovered["value"]["policy"]["passed"].as_bool().unwrap());
        outcomes.push(json!({"fault":fault,"supervisor_survived":true,"survivor_progress":true,"resources_retired":true,"replacement_works":true,"replacement_full_suite":true}));
    }
    host.retire(1);
    json!({"host":"linux","isolation":"child process","faults":outcomes,"resources_empty":host.subscriptions.is_empty() && host.stalled.is_empty()})
}
