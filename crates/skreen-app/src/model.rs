//! Application state shared by the GUI screens: settings, recent rooms, peers.
//!
//! Kept free of egui types so it can be unit-tested headlessly.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Persisted user settings and history, stored next to the TOFU trust store.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub display_name: String,
    pub fps: u32,
    pub bitrate: u32,
    pub scale: f32,
    pub audio: bool,
    /// Most recent rooms/targets, newest first.
    pub recent: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            display_name: crate::discovery::default_device_name(),
            fps: 30,
            bitrate: 4_000_000,
            scale: 1.0,
            audio: false,
            recent: Vec::new(),
        }
    }
}

pub const MAX_RECENT: usize = 5;

impl Settings {
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".into());
        std::fs::write(path, text)
    }

    /// Record a room or target, newest first, de-duplicated, capped.
    pub fn remember(&mut self, target: &str) {
        let target = target.trim();
        if target.is_empty() {
            return;
        }
        self.recent.retain(|r| r != target);
        self.recent.insert(0, target.to_string());
        self.recent.truncate(MAX_RECENT);
    }

    pub fn default_path() -> PathBuf {
        std::env::var_os("SKREENERINO_CONFIG_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(".skreenerino"))
            .join("settings.json")
    }
}

/// Which screen the window is showing.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Screen {
    #[default]
    Home,
    Meeting,
}

/// A peer row rendered on the home screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerRow {
    pub device: String,
    pub endpoint: String,
    pub room: Option<String>,
}

impl PeerRow {
    pub fn from_advertised(peer: &skreen_discovery::AdvertisedPeer) -> Self {
        Self {
            device: peer.device.0.clone(),
            endpoint: peer.endpoint(),
            room: peer.room.clone(),
        }
    }

    pub fn subtitle(&self) -> String {
        match &self.room {
            Some(room) => format!("room {room} Â· {}", self.endpoint),
            None => self.endpoint.clone(),
        }
    }
}

/// Everything the home screen needs, without touching egui.
#[derive(Debug, Default)]
pub struct HomeModel {
    pub peers: Vec<PeerRow>,
    /// Room name typed on the home screen.
    pub room: String,
    pub manual: String,
    pub manual_error: Option<String>,
    pub scanning: bool,
}

impl HomeModel {
    /// Room to create, or `None` when blank.
    pub fn room_value(&self) -> Option<String> {
        let room = self.room.trim();
        (!room.is_empty()).then(|| room.to_string())
    }

    /// Validate a manual `host:port` entry.
    pub fn validate_manual(&mut self) -> Option<String> {
        let text = self.manual.trim();
        if text.is_empty() {
            self.manual_error = None;
            return None;
        }
        match skreen_discovery::parse_manual_endpoint(text) {
            Some(_) => {
                self.manual_error = None;
                Some(text.to_string())
            }
            None => {
                self.manual_error = Some("expected host:port, for example 10.0.0.5:7654".into());
                None
            }
        }
    }

    pub fn set_peers(&mut self, peers: Vec<skreen_discovery::AdvertisedPeer>) {
        self.peers = peers.iter().map(PeerRow::from_advertised).collect();
        self.scanning = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remember_keeps_newest_first_and_dedups() {
        let mut s = Settings::default();
        s.remember("classroom");
        s.remember("desk");
        s.remember("classroom");
        assert_eq!(s.recent, vec!["classroom".to_string(), "desk".to_string()]);
    }

    #[test]
    fn remember_caps_history() {
        let mut s = Settings::default();
        for i in 0..10 {
            s.remember(&format!("room{i}"));
        }
        assert_eq!(s.recent.len(), MAX_RECENT);
        assert_eq!(s.recent[0], "room9");
    }

    #[test]
    fn remember_ignores_blank() {
        let mut s = Settings::default();
        s.remember("   ");
        assert!(s.recent.is_empty());
    }

    #[test]
    fn settings_round_trip() {
        let dir = std::env::temp_dir().join("skreenerino-settings-test");
        let path = dir.join("settings.json");
        let mut s = Settings::default();
        s.fps = 15;
        s.remember("lab");
        s.save(&path).unwrap();
        let loaded = Settings::load(&path);
        assert_eq!(loaded.fps, 15);
        assert_eq!(loaded.recent, vec!["lab".to_string()]);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn manual_entry_validation() {
        let mut model = HomeModel::default();
        model.manual = "10.0.0.5:7654".into();
        assert_eq!(model.validate_manual().as_deref(), Some("10.0.0.5:7654"));
        assert!(model.manual_error.is_none());

        model.manual = "nonsense".into();
        assert!(model.validate_manual().is_none());
        assert!(model.manual_error.is_some());
    }

    #[test]
    fn peer_row_subtitle_mentions_room() {
        let peer = skreen_discovery::AdvertisedPeer {
            device: skreen_proto::DeviceId("desk".into()),
            host: "10.0.0.5".into(),
            port: 7654,
            room: Some("classroom".into()),
        };
        let row = PeerRow::from_advertised(&peer);
        assert!(row.subtitle().contains("classroom"));
        assert!(row.subtitle().contains("10.0.0.5:7654"));
    }
}
