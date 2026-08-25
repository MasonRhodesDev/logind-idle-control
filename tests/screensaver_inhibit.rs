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

/// A private session bus for the duration of the test, killed on drop.
///
/// Never reuse an inherited `DBUS_SESSION_BUS_ADDRESS`: on the desktop this
/// daemon exists for, hypridle already owns `org.freedesktop.ScreenSaver`
/// on the real bus, so the stub below cannot claim the name and the test
/// fails with NameTaken. Trusting the ambient bus made `cargo test` fail on
/// exactly the target platform. A private bus also keeps the test from
/// poking the developer's live session.
struct PrivateBus {
    pid: i32,
}

impl PrivateBus {
    /// `None` means no `dbus-daemon` to run one; the caller skips rather
    /// than failing, since not every build environment ships dbus.
    fn start() -> Option<Self> {
        let out = std::process::Command::new("dbus-daemon")
            .args(["--session", "--print-address", "--print-pid", "--fork"])
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&out.stdout);
        let mut lines = text.lines();
        let address = lines.next()?.trim().to_string();
        let pid: i32 = lines.next()?.trim().parse().ok()?;
        if address.is_empty() {
            return None;
        }
        // SAFETY: called once, before any other thread exists in this test
        // binary. A second test in this file would race here -- see the
        // one-test-one-bus-name note below, which already forbids adding one.
        unsafe { std::env::set_var("DBUS_SESSION_BUS_ADDRESS", &address) };
        Some(Self { pid })
    }
}

impl Drop for PrivateBus {
    fn drop(&mut self) {
        // Otherwise every run leaves a daemon behind, which accumulates on
        // a persistent CI runner and in a local edit-test loop.
        let _ = std::process::Command::new("kill")
            .arg(self.pid.to_string())
            .status();
    }
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
    let Some(_bus) = PrivateBus::start() else {
        eprintln!("no dbus-daemon to provide a private session bus; skipping");
        return;
    };
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
