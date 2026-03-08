use image::GenericImageView;
use ksni::menu::StandardItem;
use ksni::{Icon, MenuItem, OfflineReason, Tray};
use std::sync::LazyLock;

use logind_idle_control::dbus;

pub struct IdleControlTray {
    pub enabled: bool,
    pub session_id: String,
}

impl Tray for IdleControlTray {
    fn id(&self) -> String {
        format!("logind-idle-control-{}", self.session_id)
    }

    fn icon_name(&self) -> String {
        // Use theme icons if available
        if self.enabled {
            "caffeine-cup-full".into()
        } else {
            "caffeine-cup-empty".into()
        }
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        // Fallback to embedded icons
        if self.enabled {
            vec![ICON_ENABLED.clone()]
        } else {
            vec![ICON_DISABLED.clone()]
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
            if let Err(e) = dbus::emit_signal("Toggle").await {
                tracing::error!("Failed to toggle: {}", e);
            }
        });
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![
            StandardItem {
                label: if self.enabled {
                    "Disable Idle Inhibitor".into()
                } else {
                    "Enable Idle Inhibitor".into()
                },
                activate: Box::new(|_| {
                    tokio::spawn(async {
                        if let Err(e) = dbus::emit_signal("Toggle").await {
                            tracing::error!("Failed to toggle: {}", e);
                        }
                    });
                }),
                ..Default::default()
            }
            .into(),
        ]
    }

    fn watcher_offline(&self, _reason: OfflineReason) -> bool {
        tracing::warn!("StatusNotifierWatcher went offline, will retry");
        true // Return true to keep running
    }
}

// Embedded icons (ARGB32 format)
static ICON_ENABLED: LazyLock<Icon> = LazyLock::new(|| load_icon(include_bytes!("../../icons/enabled.png")));

static ICON_DISABLED: LazyLock<Icon> = LazyLock::new(|| load_icon(include_bytes!("../../icons/disabled.png")));

fn load_icon(png_bytes: &[u8]) -> Icon {
    let img = image::load_from_memory_with_format(png_bytes, image::ImageFormat::Png)
        .expect("valid embedded PNG");
    let (width, height) = img.dimensions();
    let rgba = img.into_rgba8();
    let mut data = rgba.into_vec();

    // Convert RGBA to ARGB (StatusNotifierItem requires ARGB in network byte order)
    for pixel in data.chunks_exact_mut(4) {
        let r = pixel[0];
        let g = pixel[1];
        let b = pixel[2];
        let a = pixel[3];
        pixel[0] = a;
        pixel[1] = r;
        pixel[2] = g;
        pixel[3] = b;
    }

    Icon {
        width: width as i32,
        height: height as i32,
        data,
    }
}
