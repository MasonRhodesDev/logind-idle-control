use anyhow::Result;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Enabled,
    Disabled,
}

impl State {
    pub fn load() -> Result<Self> {
        let state_path = Self::state_path();
        
        if state_path.exists() {
            let content = std::fs::read_to_string(&state_path)?;
            match content.trim() {
                "1" => Ok(Self::Enabled),
                _ => Ok(Self::Disabled),
            }
        } else {
            Ok(Self::Disabled)
        }
    }
    
    pub fn save(&self) -> Result<()> {
        let state_path = Self::state_path();
        
        if let Some(parent) = state_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        
        let content = match self {
            Self::Enabled => "1",
            Self::Disabled => "0",
        };
        
        std::fs::write(state_path, content)?;
        Ok(())
    }
    
    pub fn toggle(&self) -> Self {
        match self {
            Self::Enabled => Self::Disabled,
            Self::Disabled => Self::Enabled,
        }
    }
    
    pub fn is_enabled(&self) -> bool {
        matches!(self, Self::Enabled)
    }
    
    pub fn state_path() -> PathBuf {
        let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
            .ok()
            .and_then(|d| PathBuf::from(d).canonicalize().ok())
            .unwrap_or_else(|| PathBuf::from("/tmp"));
        
        if let Ok(session) = crate::session::get_current_session_sync() {
            runtime_dir.join(format!("logind-idle-control-session-{}.state", session.id))
        } else {
            runtime_dir.join("logind-idle-control.state")
        }
    }
}

impl std::fmt::Display for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Enabled => write!(f, "1"),
            Self::Disabled => write!(f, "0"),
        }
    }
}
