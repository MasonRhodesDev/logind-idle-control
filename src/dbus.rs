use anyhow::{Context, Result};
use zbus::{proxy, Connection};
use zbus::zvariant::{OwnedFd, OwnedObjectPath};
use crate::session::SessionInfo;

#[proxy(
    interface = "org.freedesktop.login1.Manager",
    default_service = "org.freedesktop.login1",
    default_path = "/org/freedesktop/login1"
)]
trait Login1Manager {
    fn inhibit(
        &self,
        what: &str,
        who: &str,
        why: &str,
        mode: &str,
    ) -> zbus::Result<OwnedFd>;
    
    fn get_session_by_pid(&self, pid: u32) -> zbus::Result<(String, OwnedObjectPath)>;
}

pub struct InhibitorLock {
    _fd: OwnedFd,
}

impl InhibitorLock {
    pub async fn acquire() -> Result<Self> {
        let connection = Connection::system()
            .await
            .context("Failed to connect to system D-Bus")?;
        
        let proxy = Login1ManagerProxy::new(&connection)
            .await
            .context("Failed to create logind proxy")?;
        
        let fd = proxy
            .inhibit(
                "idle",
                "logind-idle-control",
                "User requested idle inhibition",
                "block",
            )
            .await
            .context("Failed to acquire inhibitor lock from logind")?;
        
        tracing::info!("Acquired idle inhibitor lock");
        
        Ok(Self { _fd: fd })
    }
}

impl Drop for InhibitorLock {
    fn drop(&mut self) {
        tracing::info!("Released idle inhibitor lock");
    }
}

fn get_object_path_for_session(session: &SessionInfo) -> String {
    format!("/com/logind/IdleControl/session_{}", session.id.replace('-', "_"))
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
    proxy.add_match_rule(match_rule.into()).await?;
    
    let mut stream = zbus::MessageStream::from(&connection);
    
    tracing::info!("Listening for Unlock signals on {} (session {})", session_path, session.id);
    
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
    
    let state = crate::State::load()?;
    println!("{}", state);
    std::io::stdout().flush()?;
    
    let (tx_state, mut rx_state) = tokio::sync::mpsc::channel::<bool>(10);
    let (tx_event, mut rx_event) = tokio::sync::mpsc::channel::<()>(10);
    
    let session_state = session.clone();
    let object_path = get_object_path_for_session(&session);
    tokio::spawn(async move {
        if let Err(e) = monitor_state_changed_signals(&session_state, &object_path, tx_state).await {
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
                let state = crate::State::load()?;
                println!("{}", state);
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
    proxy.add_match_rule(match_rule.into()).await?;
    
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
    proxy.add_match_rule(match_rule.into()).await?;
    
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
    proxy.add_match_rule(match_rule.into()).await?;
    
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
    proxy.add_match_rule(match_rule.into()).await?;
    
    let mut stream = zbus::MessageStream::from(&connection);
    
    tracing::info!("Listening for D-Bus signals on {} (session {})", object_path, session.id);

    while let Some(msg) = stream.next().await {
        if let Ok(msg) = msg {
            if let Some(path) = msg.header().path() {
                if path.as_str() == object_path {
                    if let Some(interface) = msg.header().interface() {
                        if interface.as_str() == "com.logind.IdleControl" {
                            if let Some(member) = msg.header().member() {
                                let member_str = member.as_str();
                                if member_str == "Enable" || member_str == "Disable" || member_str == "Toggle" {
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
    proxy.add_match_rule(match_rule.into()).await?;
    
    let mut stream = zbus::MessageStream::from(&connection);
    
    tracing::info!("Listening for Lock signals on {} (session {})", session_path, session.id);

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
