//! Fixed JNI commands for the Android process-lifecycle experiment only.
use crate::{policy, Guest, LIFECYCLE};
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    time::{Duration, Instant},
};
thread_local! { static INSTANCE: RefCell<Option<Guest>> = const { RefCell::new(None) }; }
fn jobs(guest: &Guest) {
    let mut count = 0;
    while guest.rt.is_job_pending() {
        guest.rt.execute_pending_job().unwrap();
        count += 1;
        assert!(count < 10000);
    }
}
fn step(action: i32) -> i32 {
    if action == 5 {
        std::process::abort();
    }
    if action == 6 {
        loop {
            std::thread::sleep(Duration::from_secs(60));
        }
    }
    if action == 7 {
        let report = crate::run();
        assert_eq!(report["cycles"], 20);
        assert_eq!(report["checks"].as_array().unwrap().len(), 14);
        assert_eq!(report["policy"]["passed"], true);
        return 20;
    }
    INSTANCE.with(|slot| {
        if action == 0 {
            *slot.borrow_mut() = Some(Guest::new());
        }
        let borrowed = slot.borrow();
        let guest = borrowed.as_ref().unwrap();
        let until = Instant::now() + Duration::from_secs(3);
        guest
            .rt
            .set_interrupt_handler(Some(Box::new(move || Instant::now() > until)));
        match action {
            0 => {
                assert_eq!(guest.read("[vm.state.value,vm.action()]"), json!([1, 2]));
                let script: Value = serde_json::from_str(LIFECYCLE).unwrap();
                guest.eval(script["start"].as_str().unwrap()).unwrap();
                jobs(guest);
                let raw = guest.out.borrow_mut().pop_front().unwrap();
                let request = policy::request(&raw).unwrap();
                assert_eq!(request, json!({"id":1,"op":"subscribe","value":"value"}));
                1 // Actual guest subscription request; parent records it before granting.
            }
            1 => {
                guest.deliver(json!({"id":1,"value":"s1"}));
                jobs(guest);
                let raw = guest.out.borrow_mut().pop_front().unwrap();
                let request = policy::request(&raw).unwrap();
                assert_eq!(request, json!({"id":2,"op":"stall","value":null}));
                2 // Actual guest pending call, retained by the parent generation.
            }
            2 => {
                assert_eq!(guest.read("1+1"), json!(2));
                2
            }
            3 => {
                guest.deliver(json!({"event":"s1","value":1}));
                jobs(guest);
                guest.read("events.length").as_i64().unwrap() as i32
            }
            4 => {
                guest.eval("vm.state.value=9;vm.action=()=>10;").unwrap();
                9
            }
            8 => {
                assert_eq!(guest.read("[vm.state.value,vm.action()]"), json!([1, 2]));
                assert_eq!(guest.read("[reply,events.length]"), json!([null, 0]));
                1
            }
            _ => panic!("unknown trusted service test command"),
        }
    })
}
#[no_mangle]
pub extern "system" fn Java_com_lelloman_talia_spike_RuntimeService_step(
    _env: *mut std::ffi::c_void,
    _class: *mut std::ffi::c_void,
    action: i32,
) -> i32 {
    std::panic::catch_unwind(|| step(action)).unwrap_or(-1)
}
