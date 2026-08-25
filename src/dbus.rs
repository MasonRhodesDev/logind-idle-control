use crate::session::SessionInfo;
use crate::State;
use anyhow::{bail, Context, Result};
use std::sync::Arc;
use tokio::sync::Mutex;
use zbus::object_server::SignalEmitter;
use zbus::{interface, proxy, Connection};

/// An `org.freedesktop.ScreenSaver` inhibit, held on its own connection.
///
/// The logind inhibitor alone is invisible to hypridle: it counts only the
/// inhibits taken through the ScreenSaver interface it serves
/// (`m_iInhibitLocks` is touched solely by its `onInhibit`), and consults
/// login1 only to resolve the session. Without this the toggle does not
/// stop the idle lock, which is the whole point of holding it.
///
/// The connection is private to the inhibit so that dropping it releases
/// the count even if `release` is never reached: hypridle watches
/// NameOwnerChanged and drops an owner's cookies when its name goes away.
pub struct ScreenSaverInhibit {
    connection: Connection,
    cookie: u32,
}

impl ScreenSaverInhibit {
    pub async fn acquire() -> Result<Self> {
        let connection = Connection::session()
            .await
            .context("Failed to connect to session D-Bus")?;
        let proxy = ScreenSaverProxy::new(&connection)
            .await
            .context("Failed to create ScreenSaver proxy")?;
        let cookie = proxy
            .inhibit("logind-idle-control", "User requested idle inhibition")
            .await
            .context("Failed to inhibit via org.freedesktop.ScreenSaver")?;
        tracing::info!("Acquired ScreenSaver inhibit (cookie {cookie})");
        Ok(Self { connection, cookie })
    }

    /// Hand the cookie back. Dropping without this still releases the
    /// count when the connection closes, but only once the bus notices.
    pub async fn release(self) {
        match ScreenSaverProxy::new(&self.connection).await {
            Ok(proxy) => {
                if let Err(e) = proxy.un_inhibit(self.cookie).await {
                    tracing::warn!("Failed to release ScreenSaver inhibit: {e}");
                } else {
                    tracing::info!("Released ScreenSaver inhibit (cookie {})", self.cookie);
                }
            }
            Err(e) => tracing::warn!("Failed to create ScreenSaver proxy for release: {e}"),
        }
    }
}

#[proxy(
    interface = "org.freedesktop.ScreenSaver",
    default_service = "org.freedesktop.ScreenSaver",
    default_path = "/org/freedesktop/ScreenSaver"
)]
trait ScreenSaver {
    fn inhibit(&self, application_name: &str, reason_for_inhibit: &str) -> zbus::Result<u32>;
    fn un_inhibit(&self, cookie: u32) -> zbus::Result<()>;
}

pub struct InhibitorLock {
    _inhibit: logind_session::Inhibitor,
    /// Best effort: without hypridle (or any ScreenSaver implementation)
    /// on the bus there is nothing to inform, and the logind inhibitor --
    /// which hyprstate and systemd do honour -- still stands.
    screensaver: Option<ScreenSaverInhibit>,
}

impl InhibitorLock {
    pub async fn acquire() -> Result<Self> {
        let connection = Connection::system()
            .await
            .context("Failed to connect to system D-Bus")?;

        let proxy = logind_session::LogindManagerProxy::new(&connection)
            .await
            .context("Failed to create logind proxy")?;

        let inhibit = logind_session::Inhibitor::acquire(
            &proxy,
            "idle",
            "logind-idle-control",
            "User requested idle inhibition",
            "block",
        )
        .await
        .context("Failed to acquire inhibitor lock from logind")?;

        tracing::info!("Acquired idle inhibitor lock");

        let screensaver = match ScreenSaverInhibit::acquire().await {
            Ok(inhibit) => Some(inhibit),
            Err(e) => {
                tracing::warn!(
                    "No ScreenSaver inhibit ({e:#}); idle daemons that only \
                     count those will not see this inhibitor"
                );
                None
            }
        };

        Ok(Self {
            _inhibit: inhibit,
            screensaver,
        })
    }
}

impl InhibitorLock {
    /// Release both halves. The daemon drops the lock rather than calling
    /// this in some paths; the ScreenSaver count still goes away then,
    /// because its connection closes with it.
    pub async fn release(mut self) {
        if let Some(screensaver) = self.screensaver.take() {
            screensaver.release().await;
        }
    }
}

impl Drop for InhibitorLock {
    fn drop(&mut self) {
        tracing::info!("Released idle inhibitor lock");
    }
}

#[proxy(
    interface = "com.logind.IdleControl",
    default_service = "com.logind.IdleControl",
    default_path = "/com/logind/IdleControl"
)]
trait IdleControl {
    fn enable(&self) -> zbus::Result<bool>;
    fn disable(&self) -> zbus::Result<bool>;
    fn toggle(&self) -> zbus::Result<bool>;

    #[zbus(property)]
    fn enabled(&self) -> zbus::Result<bool>;
}

struct ControlService {
    state: Arc<Mutex<State>>,
    inhibitor: Arc<Mutex<Option<InhibitorLock>>>,
    session: SessionInfo,
}

#[interface(name = "com.logind.IdleControl")]
impl ControlService {
    async fn enable(&self) -> zbus::fdo::Result<bool> {
        self.dispatch("Enable").await
    }

    async fn disable(&self) -> zbus::fdo::Result<bool> {
        self.dispatch("Disable").await
    }

    async fn toggle(&self) -> zbus::fdo::Result<bool> {
        self.dispatch("Toggle").await
    }

    #[zbus(property)]
    async fn enabled(&self) -> bool {
        self.inhibitor.lock().await.is_some()
    }

    #[zbus(signal)]
    async fn state_changed(signal_emitter: &SignalEmitter<'_>, enabled: bool) -> zbus::Result<()>;
}

impl ControlService {
    async fn dispatch(&self, signal_name: &str) -> zbus::fdo::Result<bool> {
        handle_signal(
            signal_name,
            Arc::clone(&self.state),
            Arc::clone(&self.inhibitor),
            Arc::new(self.session.clone()),
        )
        .await
        .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }
}

pub async fn serve_control(
    session: &SessionInfo,
    state: Arc<Mutex<State>>,
    inhibitor: Arc<Mutex<Option<InhibitorLock>>>,
) -> Result<Connection> {
    let object_path = get_object_path_for_session(session);
    let iface = ControlService {
        state,
        inhibitor,
        session: session.clone(),
    };

    zbus::connection::Builder::session()?
        .name("com.logind.IdleControl")?
        .serve_at(object_path.as_str(), iface)?
        .build()
        .await
        .context("Failed to request D-Bus name com.logind.IdleControl")
}

pub async fn name_is_owned(connection: &Connection) -> Result<bool> {
    let proxy = zbus::fdo::DBusProxy::new(connection).await?;
    let name = zbus::names::BusName::try_from("com.logind.IdleControl")
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    proxy
        .name_has_owner(name)
        .await
        .context("Failed to query D-Bus name ownership")
}

async fn ensure_name_owner(connection: &Connection) -> Result<()> {
    if !name_is_owned(connection).await? {
        bail!("Idle control daemon is not running");
    }
    Ok(())
}

pub async fn enable() -> Result<bool> {
    request("Enable").await
}

pub async fn disable() -> Result<bool> {
    request("Disable").await
}

pub async fn toggle() -> Result<bool> {
    request("Toggle").await
}

pub async fn query_enabled() -> Result<bool> {
    request("Status").await
}

async fn request(command: &str) -> Result<bool> {
    let session = crate::session::get_current_session().await?;
    let object_path = get_object_path_for_session(&session);
    let connection = Connection::session()
        .await
        .context("Failed to connect to session D-Bus")?;

    ensure_name_owner(&connection).await?;

    let proxy = IdleControlProxy::builder(&connection)
        .path(object_path.as_str())?
        .build()
        .await
        .context("Idle control daemon is not running")?;

    match command {
        "Enable" => proxy
            .enable()
            .await
            .context("Failed to enable idle inhibitor"),
        "Disable" => proxy
            .disable()
            .await
            .context("Failed to disable idle inhibitor"),
        "Toggle" => proxy
            .toggle()
            .await
            .context("Failed to toggle idle inhibitor"),
        "Status" => proxy
            .enabled()
            .await
            .context("Failed to query idle inhibitor status"),
        _ => bail!("Unknown control command"),
    }
}

pub async fn handle_signal(
    signal_name: &str,
    state: Arc<Mutex<State>>,
    inhibitor: Arc<Mutex<Option<InhibitorLock>>>,
    session: Arc<SessionInfo>,
) -> Result<bool> {
    tracing::info!("Received D-Bus signal: {}", signal_name);

    let mut current_state = state.lock().await;

    let new_state = match signal_name {
        "Enable" => State::Enabled,
        "Disable" => State::Disabled,
        "Toggle" => current_state.toggle(),
        _ => {
            drop(current_state);
            return Ok(inhibitor.lock().await.is_some());
        }
    };

    let mut lock_guard = inhibitor.lock().await;
    if new_state.is_enabled() {
        if lock_guard.is_none() {
            match InhibitorLock::acquire().await {
                Ok(lock) => {
                    *lock_guard = Some(lock);
                }
                Err(e) => {
                    tracing::error!("Failed to acquire inhibitor lock: {}", e);
                    *current_state = State::Disabled;
                    current_state.save()?;
                    drop(lock_guard);
                    drop(current_state);
                    if let Err(emit_err) = emit_state_changed(&session, false).await {
                        tracing::error!("Failed to emit StateChanged signal: {}", emit_err);
                    }
                    return Err(e).context("Failed to acquire inhibitor lock");
                }
            }
        }
        *current_state = State::Enabled;
        current_state.save()?;
    } else {
        // Hand the ScreenSaver cookie back rather than waiting for the bus
        // to notice the connection close: a consumer that still counts it
        // keeps suppressing the idle timers the user just re-enabled.
        if let Some(lock) = lock_guard.take() {
            lock.release().await;
        }
        *current_state = State::Disabled;
        current_state.save()?;
    }

    let enabled = lock_guard.is_some();
    drop(lock_guard);
    drop(current_state);

    if let Err(e) = emit_state_changed(&session, enabled).await {
        tracing::error!("Failed to emit StateChanged signal: {}", e);
    }

    tracing::info!("State changed to: {}", if enabled { "1" } else { "0" });

    Ok(enabled)
}

fn get_object_path_for_session(session: &SessionInfo) -> String {
    format!(
        "/com/logind/IdleControl/session_{}",
        session.id.replace('-', "_")
    )
}

pub async fn emit_signal(signal_name: &str) -> Result<()> {
    let session = crate::session::get_current_session().await?;
    let object_path = get_object_path_for_session(&session);

    let connection = Connection::session()
        .await
        .context("Failed to connect to session D-Bus")?;

    connection
        .emit_signal(
            None::<()>,
            object_path.as_str(),
            "com.logind.IdleControl",
            signal_name,
            &(),
        )
        .await
        .context("Failed to emit D-Bus signal")?;

    Ok(())
}

pub async fn emit_state_changed(session: &SessionInfo, enabled: bool) -> Result<()> {
    let object_path = get_object_path_for_session(session);

    let connection = Connection::session()
        .await
        .context("Failed to connect to session D-Bus")?;

    connection
        .emit_signal(
            None::<()>,
            object_path.as_str(),
            "com.logind.IdleControl",
            "StateChanged",
            &(enabled,),
        )
        .await
        .context("Failed to emit StateChanged signal")?;

    tracing::debug!("Emitted StateChanged({}) on {}", enabled, object_path);

    Ok(())
}

pub async fn listen_unlock_signals<F>(session: &SessionInfo, mut callback: F) -> Result<()>
where
    F: FnMut() + Send + 'static,
{
    use futures_util::StreamExt;
    use zbus::MatchRule;

    let connection = Connection::system()
        .await
        .context("Failed to connect to system D-Bus")?;

    let session_path = session.path.to_string();

    let match_rule = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .path(session_path.clone())?
        .interface("org.freedesktop.login1.Session")?
        .member("Unlock")?
        .build();

    let proxy = zbus::fdo::DBusProxy::new(&connection).await?;
    proxy.add_match_rule(match_rule).await?;

    let mut stream = zbus::MessageStream::from(&connection);

    tracing::info!(
        "Listening for Unlock signals on {} (session {})",
        session_path,
        session.id
    );

    while let Some(msg) = stream.next().await {
        if let Ok(msg) = msg {
            if let Some(path) = msg.header().path() {
                if path.as_str() == session_path {
                    if let Some(member) = msg.header().member() {
                        if member.as_str() == "Unlock" {
                            tracing::info!("Unlock signal detected for session {}", session.id);
                            callback();
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

pub async fn monitor_state_changes() -> Result<()> {
    use std::io::Write;

    let session = crate::session::get_current_session().await?;

    let enabled = query_enabled().await?;
    if enabled {
        println!("1");
    } else {
        println!("0");
    }
    std::io::stdout().flush()?;

    let (tx_state, mut rx_state) = tokio::sync::mpsc::channel::<bool>(10);
    let (tx_event, mut rx_event) = tokio::sync::mpsc::channel::<()>(10);

    let session_state = session.clone();
    let object_path = get_object_path_for_session(&session);
    tokio::spawn(async move {
        if let Err(e) = monitor_state_changed_signals(&session_state, &object_path, tx_state).await
        {
            tracing::warn!("StateChanged monitor exited: {}", e);
        }
    });

    let tx_lock = tx_event.clone();
    let session_lock = session.clone();
    tokio::spawn(async move {
        if let Err(e) = monitor_lock_signal(&session_lock, tx_lock).await {
            tracing::warn!("Lock monitor exited: {}", e);
        }
    });

    let tx_unlock = tx_event.clone();
    let session_unlock = session.clone();
    tokio::spawn(async move {
        if let Err(e) = monitor_unlock_signal(&session_unlock, tx_unlock).await {
            tracing::warn!("Unlock monitor exited: {}", e);
        }
    });

    loop {
        tokio::select! {
            Some(enabled) = rx_state.recv() => {
                if enabled {
                    println!("1");
                } else {
                    println!("0");
                }
                std::io::stdout().flush()?;
            }
            Some(()) = rx_event.recv() => {
                let enabled = query_enabled().await?;
                if enabled {
                    println!("1");
                } else {
                    println!("0");
                }
                std::io::stdout().flush()?;
            }
            else => break,
        }
    }

    Ok(())
}

async fn monitor_state_changed_signals(
    _session: &SessionInfo,
    object_path: &str,
    tx: tokio::sync::mpsc::Sender<bool>,
) -> Result<()> {
    use futures_util::StreamExt;
    use zbus::MatchRule;

    let connection = Connection::session()
        .await
        .context("Failed to connect to session D-Bus")?;

    let match_rule = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .path(object_path)?
        .interface("com.logind.IdleControl")?
        .member("StateChanged")?
        .build();

    let proxy = zbus::fdo::DBusProxy::new(&connection).await?;
    proxy.add_match_rule(match_rule).await?;

    let mut stream = zbus::MessageStream::from(&connection);

    while let Some(msg) = stream.next().await {
        if let Ok(msg) = msg {
            if let Some(path) = msg.header().path() {
                if path.as_str() == object_path {
                    if let Some(interface) = msg.header().interface() {
                        if interface.as_str() == "com.logind.IdleControl" {
                            if let Some(member) = msg.header().member() {
                                if member.as_str() == "StateChanged" {
                                    if let Ok(enabled) = msg.body().deserialize::<bool>() {
                                        tx.send(enabled).await.ok();
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

async fn monitor_lock_signal(
    session: &SessionInfo,
    tx: tokio::sync::mpsc::Sender<()>,
) -> Result<()> {
    use futures_util::StreamExt;
    use zbus::MatchRule;

    let connection = Connection::system()
        .await
        .context("Failed to connect to system D-Bus")?;

    let session_path = session.path.to_string();

    let match_rule = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .path(session_path.clone())?
        .interface("org.freedesktop.login1.Session")?
        .member("Lock")?
        .build();

    let proxy = zbus::fdo::DBusProxy::new(&connection).await?;
    proxy.add_match_rule(match_rule).await?;

    let mut stream = zbus::MessageStream::from(&connection);

    while let Some(msg) = stream.next().await {
        if let Ok(msg) = msg {
            if let Some(path) = msg.header().path() {
                if path.as_str() == session_path {
                    if let Some(member) = msg.header().member() {
                        if member.as_str() == "Lock" {
                            tx.send(()).await.ok();
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

async fn monitor_unlock_signal(
    session: &SessionInfo,
    tx: tokio::sync::mpsc::Sender<()>,
) -> Result<()> {
    use futures_util::StreamExt;
    use zbus::MatchRule;

    let connection = Connection::system()
        .await
        .context("Failed to connect to system D-Bus")?;

    let session_path = session.path.to_string();

    let match_rule = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .path(session_path.clone())?
        .interface("org.freedesktop.login1.Session")?
        .member("Unlock")?
        .build();

    let proxy = zbus::fdo::DBusProxy::new(&connection).await?;
    proxy.add_match_rule(match_rule).await?;

    let mut stream = zbus::MessageStream::from(&connection);

    while let Some(msg) = stream.next().await {
        if let Ok(msg) = msg {
            if let Some(path) = msg.header().path() {
                if path.as_str() == session_path {
                    if let Some(member) = msg.header().member() {
                        if member.as_str() == "Unlock" {
                            tx.send(()).await.ok();
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

pub async fn listen_signals<F>(session: &SessionInfo, mut callback: F) -> Result<()>
where
    F: FnMut(&str) + Send + 'static,
{
    use futures_util::StreamExt;
    use zbus::MatchRule;

    let object_path = get_object_path_for_session(session);

    let connection = Connection::session()
        .await
        .context("Failed to connect to session D-Bus")?;

    let match_rule = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .path(object_path.clone())?
        .interface("com.logind.IdleControl")?
        .build();

    let proxy = zbus::fdo::DBusProxy::new(&connection).await?;
    proxy.add_match_rule(match_rule).await?;

    let mut stream = zbus::MessageStream::from(&connection);

    tracing::info!(
        "Listening for D-Bus signals on {} (session {})",
        object_path,
        session.id
    );

    while let Some(msg) = stream.next().await {
        if let Ok(msg) = msg {
            if let Some(path) = msg.header().path() {
                if path.as_str() == object_path {
                    if let Some(interface) = msg.header().interface() {
                        if interface.as_str() == "com.logind.IdleControl" {
                            if let Some(member) = msg.header().member() {
                                let member_str = member.as_str();
                                if member_str == "Enable"
                                    || member_str == "Disable"
                                    || member_str == "Toggle"
                                {
                                    callback(member_str);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

pub async fn listen_lock_signals<F>(session: &SessionInfo, mut callback: F) -> Result<()>
where
    F: FnMut() + Send + 'static,
{
    use futures_util::StreamExt;
    use zbus::MatchRule;

    let connection = Connection::system()
        .await
        .context("Failed to connect to system D-Bus")?;

    let session_path = session.path.to_string();

    let match_rule = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .path(session_path.clone())?
        .interface("org.freedesktop.login1.Session")?
        .member("Lock")?
        .build();

    let proxy = zbus::fdo::DBusProxy::new(&connection).await?;
    proxy.add_match_rule(match_rule).await?;

    let mut stream = zbus::MessageStream::from(&connection);

    tracing::info!(
        "Listening for Lock signals on {} (session {})",
        session_path,
        session.id
    );

    while let Some(msg) = stream.next().await {
        if let Ok(msg) = msg {
            if let Some(path) = msg.header().path() {
                if path.as_str() == session_path {
                    if let Some(member) = msg.header().member() {
                        if member.as_str() == "Lock" {
                            tracing::info!("Lock signal detected for session {}", session.id);
                            callback();
                        }
                    }
                }
            }
        }
    }

    Ok(())
}
