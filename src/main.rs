use anyhow::Result;
use clap::{Parser, Subcommand};
use logind_idle_control::{dbus, State};

#[derive(Parser)]
#[command(name = "logind-idle-ctl")]
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
    }
    
    Ok(())
}


