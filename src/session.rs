use anyhow::{Context, Result, bail};
use zbus::{proxy, Connection};
use zbus::zvariant::OwnedObjectPath;

#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub id: String,
    pub path: OwnedObjectPath,
}

#[proxy(
    interface = "org.freedesktop.login1.Manager",
    default_service = "org.freedesktop.login1",
    default_path = "/org/freedesktop/login1"
)]
trait Login1Manager {
    fn get_session_by_pid(&self, pid: u32) -> zbus::Result<(String, OwnedObjectPath)>;
}

#[proxy(
    interface = "org.freedesktop.login1.Session",
    default_service = "org.freedesktop.login1"
)]
trait Login1Session {
    #[zbus(property)]
    fn type_(&self) -> zbus::Result<String>;
    
    #[zbus(property)]
    fn display(&self) -> zbus::Result<String>;
}

pub async fn get_current_session() -> Result<SessionInfo> {
    let connection = Connection::system()
        .await
        .context("Failed to connect to system D-Bus")?;
    
    let manager_proxy = Login1ManagerProxy::new(&connection)
        .await
        .context("Failed to create logind manager proxy")?;
    
    let (session_id, session_path) = manager_proxy
        .get_session_by_pid(std::process::id())
        .await
        .context("Failed to get session by PID")?;
    
    let session_proxy = Login1SessionProxy::builder(&connection)
        .path(&session_path)?
        .build()
        .await
        .context("Failed to create session proxy")?;
    
    let session_type = session_proxy
        .type_()
        .await
        .context("Failed to get session type")?;
    
    if session_type != "x11" && session_type != "wayland" {
        bail!("Not a graphical session (type: {})", session_type);
    }
    
    tracing::debug!(
        "Detected graphical session: id={}, type={}, path={}",
        session_id,
        session_type,
        session_path
    );
    
    Ok(SessionInfo {
        id: session_id,
        path: session_path,
    })
}

pub fn get_current_session_sync() -> Result<SessionInfo> {
    tokio::runtime::Runtime::new()?.block_on(get_current_session())
}
