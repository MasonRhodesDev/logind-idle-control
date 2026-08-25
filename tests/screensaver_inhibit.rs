//! The idle inhibitor must be visible to hypridle.
//!
//! hypridle counts only its own `org.freedesktop.ScreenSaver` inhibits
//! (`m_iInhibitLocks` is touched solely by `onInhibit`); it never lists
//! logind inhibitors. A logind-only inhibitor is therefore invisible to it,
//! and the session idle-locks with the toggle on. These tests pin that we
//! take the ScreenSaver inhibit too, against a stub service standing in for
//! hypridle on a private bus.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use logind_idle_control::dbus::ScreenSaverInhibit;
use zbus::{connection, interface};

#[derive(Default)]
struct Calls {
    inhibited: Vec<(String, String)>,
    uninhibited: Vec<u32>,
}

struct StubScreenSaver {
    calls: Arc<Mutex<Calls>>,
}

#[interface(name = "org.freedesktop.ScreenSaver")]
impl StubScreenSaver {
    fn inhibit(&self, application_name: String, reason_for_inhibit: String) -> u32 {
        self.calls
            .lock()
            .unwrap()
            .inhibited
            .push((application_name, reason_for_inhibit));
        4242
    }

    fn un_inhibit(&self, cookie: u32) {
        self.calls.lock().unwrap().uninhibited.push(cookie);
    }
}

/// CI runs `cargo test` with no session bus, so provide one rather than
/// skipping: a test that silently does not run is worse than no test.
fn ensure_session_bus() -> bool {
    if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some() {
        return true;
    }
    let out = match std::process::Command::new("dbus-daemon")
        .args(["--session", "--print-address", "--fork"])
        .output()
    {
        Ok(out) if out.status.success() => out,
        _ => {
            eprintln!("no session bus and no dbus-daemon to start one; skipping");
            return false;
        }
    };
    let address = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if address.is_empty() {
        eprintln!("dbus-daemon printed no address; skipping");
        return false;
    }
    // SAFETY: single-threaded at this point in the test binary.
    unsafe { std::env::set_var("DBUS_SESSION_BUS_ADDRESS", address) };
    true
}

async fn stub_bus() -> (zbus::Connection, Arc<Mutex<Calls>>) {
    let calls = Arc::new(Mutex::new(Calls::default()));
    let conn = connection::Builder::session()
        .expect("session bus")
        .name("org.freedesktop.ScreenSaver")
        .expect("claim name")
        .serve_at(
            "/org/freedesktop/ScreenSaver",
            StubScreenSaver {
                calls: Arc::clone(&calls),
            },
        )
        .expect("serve")
        .build()
        .await
        .expect("stub bus");
    (conn, calls)
}

// One test, one bus name: `org.freedesktop.ScreenSaver` is a well-known
// name, so two stubs in parallel tests would queue on it and one test's
// calls would land on the other's stub.
#[tokio::test]
async fn the_inhibit_is_visible_to_a_screensaver_consumer_and_is_handed_back() {
    if !ensure_session_bus() {
        return;
    }
    let (_conn, calls) = stub_bus().await;

    let lock = ScreenSaverInhibit::acquire()
        .await
        .expect("screensaver inhibit");
    tokio::time::sleep(Duration::from_millis(50)).await;
    let taken = calls.lock().unwrap().inhibited.clone();
    assert_eq!(
        taken.len(),
        1,
        "expected exactly one Inhibit call: {taken:?}"
    );
    assert_eq!(taken[0].0, "logind-idle-control");

    lock.release().await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    let released = calls.lock().unwrap().uninhibited.clone();
    assert_eq!(released, vec![4242], "the cookie must be handed back");
}
