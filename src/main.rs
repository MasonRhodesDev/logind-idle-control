use anyhow::Result;
use clap::{Parser, Subcommand};
use logind_idle_control::{dbus, Config, State, get_current_session};
use std::sync::Arc;
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
            dbus::emit_signal("Enable").await?;
            println!("Idle inhibitor enabled");
        }
        Commands::Disable => {
            dbus::emit_signal("Disable").await?;
            println!("Idle inhibitor disabled");
        }
        Commands::Toggle => {
            dbus::emit_signal("Toggle").await?;
            println!("Idle inhibitor toggled");
        }
        Commands::Status => {
            let state = State::load()?;
            println!("{}", state);
        }
        Commands::Config => {
            println!("Config TUI coming soon!");
            println!("Edit config file at: {:?}", logind_idle_control::Config::config_path());
        }
        Commands::Monitor => {
            dbus::monitor_state_changes().await?;
        }
        Commands::StatePath => {
            let path = State::state_path();
            println!("{}", path.display());
        }
        Commands::Daemon => {
            run_daemon().await?;
        }
    }
    
    Ok(())
}

async fn run_daemon() -> Result<()> {
    let config = Config::load()?;
    
    tracing_subscriber::fmt()
        .with_env_filter(&config.log_level)
        .init();
    
    let session = get_current_session().await?;
    
    info!("Starting logind-idle-control daemon for session {} ({})", 
        session.id, session.path);
    
    let state = Arc::new(Mutex::new(State::Disabled));
    let inhibitor_lock = Arc::new(Mutex::new(None::<dbus::InhibitorLock>));
    let session_info = Arc::new(session.clone());
    
    {
        let mut s = state.lock().await;
        *s = State::load().unwrap_or(State::Disabled);
        s.save()?;
        info!("Initial state: {} (state file: {:?})", *s, State::state_path());
    }
    
    let state_clone = Arc::clone(&state);
    let inhibitor_clone = Arc::clone(&inhibitor_lock);
    let session_for_control = (*session_info).clone();
    
    let control_handle = tokio::spawn(async move {
        let session_clone = session_for_control.clone();
        if let Err(e) = dbus::listen_signals(&session_for_control, move |signal_name| {
            let signal_owned = signal_name.to_string();
            let state = Arc::clone(&state_clone);
            let inhibitor = Arc::clone(&inhibitor_clone);
            let session = session_clone.clone();
            
            tokio::spawn(async move {
                if let Err(e) = handle_signal(&signal_owned, state, inhibitor, Arc::new(session)).await {
                    error!("Error handling signal {}: {}", signal_owned, e);
                }
            });
        })
        .await {
            error!("Control signal listener exited: {}", e);
        }
    });
    
    let lock_handle = if config.disable_on_lock {
        let state_clone = Arc::clone(&state);
        let inhibitor_clone = Arc::clone(&inhibitor_lock);
        let session_for_lock = (*session_info).clone();
        
        Some(tokio::spawn(async move {
            let session_clone = session_for_lock.clone();
            if let Err(e) = dbus::listen_lock_signals(&session_for_lock, move || {
                let state = Arc::clone(&state_clone);
                let inhibitor = Arc::clone(&inhibitor_clone);
                let session = session_clone.clone();
                
                tokio::spawn(async move {
                    info!("Lock detected, disabling idle inhibitor");
                    if let Err(e) = handle_signal("Disable", state, inhibitor, Arc::new(session)).await {
                        error!("Error handling lock signal: {}", e);
                    }
                });
            })
            .await {
                warn!("Lock signal listener exited: {}", e);
            }
        }))
    } else {
        info!("Disable on lock is disabled in config");
        None
    };
    
    let unlock_handle = {
        let session_for_unlock = (*session_info).clone();
        
        tokio::spawn(async move {
            if let Err(e) = dbus::listen_unlock_signals(&session_for_unlock, move || {
                info!("Unlock detected");
            })
            .await {
                warn!("Unlock signal listener exited: {}", e);
            }
        })
    };
    
    tokio::signal::ctrl_c().await?;
    info!("Received shutdown signal");
    
    control_handle.abort();
    if let Some(handle) = lock_handle {
        handle.abort();
    }
    unlock_handle.abort();
    
    let mut lock = inhibitor_lock.lock().await;
    *lock = None;
    
    Ok(())
}

async fn handle_signal(
    signal_name: &str,
    state: Arc<Mutex<State>>,
    inhibitor: Arc<Mutex<Option<dbus::InhibitorLock>>>,
    session: Arc<logind_idle_control::SessionInfo>,
) -> Result<()> {
    info!("Received D-Bus signal: {}", signal_name);
    
    let mut current_state = state.lock().await;
    
    let new_state = match signal_name {
        "Enable" => State::Enabled,
        "Disable" => State::Disabled,
        "Toggle" => current_state.toggle(),
        _ => return Ok(()),
    };
    
    *current_state = new_state;
    current_state.save()?;
    
    info!("State changed to: {}", new_state);
    
    let mut lock_guard = inhibitor.lock().await;
    if new_state.is_enabled() {
        if lock_guard.is_none() {
            match dbus::InhibitorLock::acquire().await {
                Ok(lock) => {
                    *lock_guard = Some(lock);
                }
                Err(e) => {
                    error!("Failed to acquire inhibitor lock: {}", e);
                }
            }
        }
    } else {
        *lock_guard = None;
    }
    
    drop(lock_guard);
    drop(current_state);
    
    if let Err(e) = dbus::emit_state_changed(&session, new_state.is_enabled()).await {
        error!("Failed to emit StateChanged signal: {}", e);
    }
    
    Ok(())
}


