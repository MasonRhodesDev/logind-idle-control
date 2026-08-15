use anyhow::Result;
use clap::{Parser, Subcommand};
use logind_idle_control::{dbus, get_current_session, Config, State};
use std::os::unix::net::UnixDatagram;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::{error, info, warn};

#[derive(Parser)]
#[command(name = "logind-idle-control")]
#[command(about = "Control logind idle inhibitor", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Enable,
    Disable,
    Toggle,
    Status,
    Config,
    Monitor,
    #[command(name = "state-path")]
    StatePath,
    Daemon,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Enable => {
            dbus::enable().await?;
            println!("Idle inhibitor enabled");
        }
        Commands::Disable => {
            dbus::disable().await?;
            println!("Idle inhibitor disabled");
        }
        Commands::Toggle => {
            dbus::toggle().await?;
            println!("Idle inhibitor toggled");
        }
        Commands::Status => {
            let enabled = dbus::query_enabled().await?;
            println!("{}", if enabled { "1" } else { "0" });
        }
        Commands::Config => {
            println!("Config TUI coming soon!");
            println!(
                "Edit config file at: {:?}",
                logind_idle_control::Config::config_path()
            );
        }
        Commands::Monitor => {
            dbus::monitor_state_changes().await?;
        }
        Commands::StatePath => {
            let path = State::state_path()?;
            println!("{}", path.display());
        }
        Commands::Daemon => {
            run_daemon().await?;
        }
    }

    Ok(())
}

fn sd_notify(payload: &str) {
    let Ok(socket) = std::env::var("NOTIFY_SOCKET") else {
        return;
    };
    let Ok(sock) = UnixDatagram::unbound() else {
        return;
    };
    let _ = sock.set_nonblocking(true);
    if let Some(name) = socket.strip_prefix('@') {
        use std::os::linux::net::SocketAddrExt;
        use std::os::unix::net::SocketAddr;
        if let Ok(addr) = SocketAddr::from_abstract_name(name.as_bytes()) {
            if sock.connect_addr(&addr).is_ok() {
                let _ = sock.send(payload.as_bytes());
            }
        }
    } else {
        let _ = sock.send_to(payload.as_bytes(), socket);
    }
}

fn watchdog_interval() -> Duration {
    std::env::var("WATCHDOG_USEC")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|us| *us > 0)
        .map(|us| Duration::from_micros(us / 2))
        .unwrap_or(Duration::from_secs(15))
}

async fn run_daemon() -> Result<()> {
    let config = Config::load()?;

    tracing_subscriber::fmt()
        .with_env_filter(&config.log_level)
        .init();

    let session = get_current_session().await?;

    info!(
        "Starting logind-idle-control daemon for session {} ({})",
        session.id, session.path
    );

    let state = Arc::new(Mutex::new(State::Disabled));
    let inhibitor_lock = Arc::new(Mutex::new(None::<dbus::InhibitorLock>));
    let session_info = Arc::new(session.clone());

    {
        let mut s = state.lock().await;
        *s = State::load()?;
        info!(
            "Initial state: {} (state file: {:?})",
            *s,
            State::state_path()?
        );

        // Acquire inhibitor if state is enabled on startup
        if s.is_enabled() {
            let mut lock = inhibitor_lock.lock().await;
            match dbus::InhibitorLock::acquire().await {
                Ok(inhibitor) => {
                    *lock = Some(inhibitor);
                    s.save()?;
                    info!("Acquired inhibitor lock on startup");
                }
                Err(e) => {
                    error!("Failed to acquire inhibitor lock on startup: {}", e);
                    *s = State::Disabled;
                    s.save()?;
                }
            }
        } else {
            s.save()?;
        }
    }

    let control_conn =
        dbus::serve_control(&session, Arc::clone(&state), Arc::clone(&inhibitor_lock)).await?;
    sd_notify("READY=1");

    let state_clone = Arc::clone(&state);
    let inhibitor_clone = Arc::clone(&inhibitor_lock);
    let session_for_control = (*session_info).clone();

    let control_handle = tokio::spawn(async move {
        loop {
            let state_clone = Arc::clone(&state_clone);
            let inhibitor_clone = Arc::clone(&inhibitor_clone);
            let session_for_control = session_for_control.clone();
            let session_clone = session_for_control.clone();
            match dbus::listen_signals(&session_for_control, move |signal_name| {
                let signal_owned = signal_name.to_string();
                let state = Arc::clone(&state_clone);
                let inhibitor = Arc::clone(&inhibitor_clone);
                let session = session_clone.clone();

                tokio::spawn(async move {
                    if let Err(e) =
                        dbus::handle_signal(&signal_owned, state, inhibitor, Arc::new(session))
                            .await
                    {
                        error!("Error handling signal {}: {}", signal_owned, e);
                    }
                });
            })
            .await
            {
                Ok(()) => warn!("Control signal listener ended; reconnecting"),
                Err(e) => error!("Control signal listener exited: {}; reconnecting", e),
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });

    let lock_handle = if config.disable_on_lock {
        let state_clone = Arc::clone(&state);
        let inhibitor_clone = Arc::clone(&inhibitor_lock);
        let session_for_lock = (*session_info).clone();

        Some(tokio::spawn(async move {
            loop {
                let state_clone = Arc::clone(&state_clone);
                let inhibitor_clone = Arc::clone(&inhibitor_clone);
                let session_for_lock = session_for_lock.clone();
                let session_clone = session_for_lock.clone();
                match dbus::listen_lock_signals(&session_for_lock, move || {
                    let state = Arc::clone(&state_clone);
                    let inhibitor = Arc::clone(&inhibitor_clone);
                    let session = session_clone.clone();

                    tokio::spawn(async move {
                        info!("Lock detected, disabling idle inhibitor");
                        if let Err(e) =
                            dbus::handle_signal("Disable", state, inhibitor, Arc::new(session))
                                .await
                        {
                            error!("Error handling lock signal: {}", e);
                        }
                    });
                })
                .await
                {
                    Ok(()) => warn!("Lock signal listener ended; reconnecting"),
                    Err(e) => warn!("Lock signal listener exited: {}; reconnecting", e),
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }))
    } else {
        info!("Disable on lock is disabled in config");
        None
    };

    let unlock_handle = {
        let session_for_unlock = (*session_info).clone();

        tokio::spawn(async move {
            loop {
                match dbus::listen_unlock_signals(&session_for_unlock, move || {
                    info!("Unlock detected");
                })
                .await
                {
                    Ok(()) => warn!("Unlock signal listener ended; reconnecting"),
                    Err(e) => warn!("Unlock signal listener exited: {}; reconnecting", e),
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        })
    };

    let bus_watch = {
        let connection = control_conn.clone();
        tokio::spawn(async move {
            let interval = watchdog_interval();
            loop {
                tokio::time::sleep(interval).await;
                match dbus::name_is_owned(&connection).await {
                    Ok(true) => sd_notify("WATCHDOG=1"),
                    Ok(false) => {
                        error!("Lost D-Bus name com.logind.IdleControl");
                        return;
                    }
                    Err(e) => {
                        error!("D-Bus watchdog check failed: {}", e);
                        return;
                    }
                }
            }
        })
    };

    let bus_died = tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            info!("Received shutdown signal");
            false
        }
        _ = bus_watch => {
            error!("D-Bus name lost or bus died");
            true
        }
    };

    control_handle.abort();
    if let Some(handle) = lock_handle {
        handle.abort();
    }
    unlock_handle.abort();

    let mut lock = inhibitor_lock.lock().await;
    *lock = None;

    if bus_died {
        anyhow::bail!("D-Bus name lost or bus died");
    }

    Ok(())
}
