use anyhow::{Context, Result};
use futures_util::StreamExt;
use ksni::TrayMethods;
use std::sync::Arc;
use tokio::sync::Mutex;
use zbus::{Connection, MatchRule};

use logind_idle_control::{get_current_session, SessionInfo, State};

#[path = "mod.rs"]
mod tray_impl;
use tray_impl::IdleControlTray;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let session = get_current_session().await?;
    tracing::info!("Starting tray for session {}", session.id);

    // Load initial state from file
    let initial_state = State::load().unwrap_or(State::Disabled);
    tracing::info!("Initial state: {:?}", initial_state);

    let tray = IdleControlTray {
        enabled: initial_state.is_enabled(),
        session_id: session.id.clone(),
        icon_invalidated: false,
    };

    // Spawn the tray service
    let handle = tray.spawn().await?;
    let handle = Arc::new(Mutex::new(handle));

    // Listen for StateChanged signals and update tray
    let handle_clone = Arc::clone(&handle);
    let session_clone = session.clone();
    tokio::spawn(async move {
        if let Err(e) = listen_state_changes(handle_clone, &session_clone).await {
            tracing::error!("State change listener error: {}", e);
        }
    });

    // Listen for icon-theme changes and force tray icon refresh
    let handle_clone = Arc::clone(&handle);
    tokio::spawn(async move {
        if let Err(e) = listen_theme_changes(handle_clone).await {
            tracing::error!("Theme change listener error: {}", e);
        }
    });

    tracing::info!("Tray icon running, press Ctrl+C to exit");

    // Keep running until signal
    tokio::signal::ctrl_c().await?;
    tracing::info!("Shutting down tray");
    Ok(())
}

fn get_object_path_for_session(session: &SessionInfo) -> String {
    format!(
        "/com/logind/IdleControl/session_{}",
        session.id.replace('-', "_")
    )
}

async fn listen_state_changes(
    handle: Arc<Mutex<ksni::Handle<tray_impl::IdleControlTray>>>,
    session: &SessionInfo,
) -> Result<()> {
    let object_path = get_object_path_for_session(session);

    let connection = Connection::session()
        .await
        .context("Failed to connect to session D-Bus")?;

    let match_rule = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .path(object_path.clone())?
        .interface("com.logind.IdleControl")?
        .member("StateChanged")?
        .build();

    let proxy = zbus::fdo::DBusProxy::new(&connection).await?;
    proxy.add_match_rule(match_rule.into()).await?;

    let mut stream = zbus::MessageStream::from(&connection);

    tracing::info!("Listening for StateChanged on {}", object_path);

    while let Some(msg) = stream.next().await {
        if let Ok(msg) = msg {
            if let Some(path) = msg.header().path() {
                if path.as_str() == object_path {
                    if let Some(member) = msg.header().member() {
                        if member.as_str() == "StateChanged" {
                            if let Ok(enabled) = msg.body().deserialize::<bool>() {
                                tracing::info!("State changed: {}", enabled);
                                let h = handle.lock().await;
                                h.update(|tray| {
                                    tray.enabled = enabled;
                                }).await;
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

/// Listen for icon-theme gsettings changes and force tray icon re-resolution.
/// When lmtt switches themes, it changes org.gnome.desktop.interface icon-theme
/// (e.g. breeze → breeze-dark). We need waybar to re-fetch our icon from the
/// new theme, so we invalidate the icon name to trigger ksni's NewIcon signal.
async fn listen_theme_changes(
    handle: Arc<Mutex<ksni::Handle<tray_impl::IdleControlTray>>>,
) -> Result<()> {
    let connection = Connection::session()
        .await
        .context("Failed to connect to session D-Bus for theme changes")?;

    // Watch for dconf changes on org.gnome.desktop.interface (covers icon-theme, color-scheme)
    let match_rule = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface("ca.desrt.dconf.Writer")?
        .member("Notify")?
        .build();

    let proxy = zbus::fdo::DBusProxy::new(&connection).await?;
    proxy.add_match_rule(match_rule.into()).await?;

    let mut stream = zbus::MessageStream::from(&connection);

    tracing::info!("Listening for icon-theme changes");

    while let Some(msg) = stream.next().await {
        if let Ok(msg) = msg {
            if let Some(member) = msg.header().member() {
                if member.as_str() == "Notify" {
                    // dconf Notify sends (path, keys, tag)
                    if let Ok((path, _, _)) =
                        msg.body().deserialize::<(&str, Vec<&str>, &str)>()
                    {
                        if path.contains("desktop/interface") {
                            tracing::info!("Icon theme changed, debouncing before refresh");
                            // Debounce: wait for all gsettings keys to settle
                            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                            // Drain any additional notifications that queued up
                            while let Ok(Some(_)) = tokio::time::timeout(
                                std::time::Duration::from_millis(50),
                                stream.next(),
                            ).await {}
                            // Single invalidation cycle
                            let h = handle.lock().await;
                            h.update(|tray| {
                                tray.icon_invalidated = true;
                            }).await;
                            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                            h.update(|tray| {
                                tray.icon_invalidated = false;
                            }).await;
                            tracing::info!("Tray icon refreshed");
                        }
                    }
                }
            }
        }
    }

    Ok(())
}
