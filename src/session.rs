use anyhow::{Context, Result};
use zbus::zvariant::OwnedObjectPath;
use zbus::Connection;

#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub id: String,
    pub path: OwnedObjectPath,
}

pub async fn get_current_session() -> Result<SessionInfo> {
    let connection = Connection::system()
        .await
        .context("Failed to connect to system D-Bus")?;

    let session = logind_session::resolve_graphical_session(&connection)
        .await
        .context("Failed to resolve graphical logind session")?;

    tracing::debug!(
        "Detected graphical session: id={}, path={}",
        session.id(),
        session.path()
    );

    Ok(SessionInfo {
        id: session.id().to_owned(),
        path: session.path().clone(),
    })
}
