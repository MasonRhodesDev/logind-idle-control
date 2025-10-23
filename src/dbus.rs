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

pub async fn listen_signals<F>(session: &SessionInfo, mut callback: F) -> Result<()>
where
    F: FnMut(&str) + Send + 'static,
{
    use futures_util::StreamExt;
    
    let object_path = get_object_path_for_session(session);
    
    let connection = Connection::session()
        .await
        .context("Failed to connect to session D-Bus")?;
    
    let mut stream = zbus::MessageStream::from(&connection);
    
    tracing::info!("Listening for D-Bus signals on {} (session {})", object_path, session.id);
    
    while let Some(msg) = stream.next().await {
        if let Ok(msg) = msg {
            if let Some(path) = msg.path() {
                if path.as_str() == object_path {
                    if let Some(interface) = msg.interface() {
                        if interface.as_str() == "com.logind.IdleControl" {
                            if let Some(member) = msg.member() {
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
    
    let connection = Connection::system()
        .await
        .context("Failed to connect to system D-Bus")?;
    
    let session_path = session.path.to_string();
    
    let mut stream = zbus::MessageStream::from(&connection);
    
    tracing::info!("Listening for Lock signals on {} (session {})", session_path, session.id);
    
    while let Some(msg) = stream.next().await {
        if let Ok(msg) = msg {
            if let Some(path) = msg.path() {
                if path.as_str() == session_path {
                    if let Some(member) = msg.member() {
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
