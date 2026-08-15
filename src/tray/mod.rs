use ksni::menu::StandardItem;
use ksni::{MenuItem, OfflineReason, Tray};

use logind_idle_control::dbus;

#[derive(Clone)]
pub struct IdleControlTray {
    pub enabled: bool,
    pub session_id: String,
    /// When true, icon_name returns a dummy value to force ksni to emit NewIcon.
    /// Set true, call update, then set false and call update again.
    pub icon_invalidated: bool,
}

impl Tray for IdleControlTray {
    fn id(&self) -> String {
        format!("logind-idle-control-{}", self.session_id)
    }

    fn icon_name(&self) -> String {
        if self.icon_invalidated {
            // Return a known-valid icon so ksni detects a change and emits NewIcon
            // without showing a missing-icon placeholder during the transition
            return "content-loading-symbolic".into();
        }
        if self.enabled {
            "caffeine-cup-full-symbolic".into()
        } else {
            "caffeine-cup-empty-symbolic".into()
        }
    }

    fn title(&self) -> String {
        "Idle Inhibitor".into()
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: if self.enabled {
                "Idle inhibitor: ENABLED".into()
            } else {
                "Idle inhibitor: disabled".into()
            },
            description: if self.enabled {
                "System will not go idle".into()
            } else {
                "System can go idle normally".into()
            },
            ..Default::default()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        tokio::spawn(async {
            if let Err(e) = dbus::toggle().await {
                tracing::error!("Failed to toggle: {}", e);
            }
        });
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![StandardItem {
            label: if self.enabled {
                "Disable Idle Inhibitor".into()
            } else {
                "Enable Idle Inhibitor".into()
            },
            activate: Box::new(|_| {
                tokio::spawn(async {
                    if let Err(e) = dbus::toggle().await {
                        tracing::error!("Failed to toggle: {}", e);
                    }
                });
            }),
            ..Default::default()
        }
        .into()]
    }

    fn watcher_offline(&self, _reason: OfflineReason) -> bool {
        tracing::warn!("StatusNotifierWatcher went offline, will retry");
        true // Return true to keep running
    }
}
