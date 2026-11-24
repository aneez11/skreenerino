//! Windows system tray icon: Show / Stop sharing / Exit, plus tray-on-close.
//!
//! The tray runs on the main thread alongside the winit loop, as `tray-icon`
//! requires. Menu events arrive on the event-loop proxy so they land in the same
//! place as every other UI event.

use crate::app::{UiCommand, UiEvent, UiProxy};
use anyhow::Result;
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

/// Menu items the app will act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    Show,
    StopSharing,
    Exit,
}

impl TrayAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::Show => "Show Skreenerino",
            Self::StopSharing => "Stop Sharing",
            Self::Exit => "Exit",
        }
    }
}

/// The live tray icon. Dropping it removes the icon from the notification area.
pub struct Tray {
    _icon: TrayIcon,
    show: MenuItem,
    stop: MenuItem,
    exit: MenuItem,
}

impl Tray {
    pub fn install() -> Result<Self> {
        let menu = Menu::new();
        let show = MenuItem::new(TrayAction::Show.label(), true, None);
        let stop = MenuItem::new(TrayAction::StopSharing.label(), false, None);
        let exit = MenuItem::new(TrayAction::Exit.label(), true, None);
        menu.append(&show)?;
        menu.append(&stop)?;
        menu.append(&PredefinedMenuItem::separator())?;
        menu.append(&exit)?;

        let icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("Skreenerino")
            .with_icon(build_icon())
            .build()?;

        Ok(Self {
            _icon: icon,
            show,
            stop,
            exit,
        })
    }

    /// Enable "Stop Sharing" while presenting.
    pub fn set_sharing(&self, sharing: bool) {
        self.stop.set_enabled(sharing);
    }

    /// Map a menu event to an action, if it belongs to this tray.
    pub fn action_for(&self, event: &MenuEvent) -> Option<TrayAction> {
        let id = event.id();
        if id == self.show.id() {
            Some(TrayAction::Show)
        } else if id == self.stop.id() {
            Some(TrayAction::StopSharing)
        } else if id == self.exit.id() {
            Some(TrayAction::Exit)
        } else {
            None
        }
    }
}

/// Translate a tray action into app commands.
pub fn handle(action: TrayAction, proxy: &UiProxy) {
    match action {
        TrayAction::Show => {
            let _ = proxy.send_event(UiEvent::TrayShow);
        }
        TrayAction::StopSharing => {
            let _ = proxy.send_event(UiEvent::TrayStopSharing);
        }
        TrayAction::Exit => {
            let _ = proxy.send_event(UiEvent::TrayExit);
        }
    }
}

/// Commands a tray action expands to, kept pure so it can be tested.
pub fn commands_for(action: TrayAction) -> Vec<UiCommand> {
    match action {
        TrayAction::Show => Vec::new(),
        TrayAction::StopSharing => vec![UiCommand::StopShare],
        TrayAction::Exit => vec![UiCommand::StopShare, UiCommand::Leave],
    }
}

/// Draw a small monitor glyph as the tray icon (16x16 RGBA).
fn build_icon() -> Icon {
    const SIZE: u32 = 32;
    let mut rgba = vec![0u8; (SIZE * SIZE * 4) as usize];

    let set = |rgba: &mut Vec<u8>, x: u32, y: u32, color: [u8; 4]| {
        let i = ((y * SIZE + x) * 4) as usize;
        rgba[i..i + 4].copy_from_slice(&color);
    };

    // Rounded screen outline in Windows accent blue.
    let accent = [0x4C, 0xC2, 0xFF, 0xFF];
    for y in 6..24 {
        for x in 4..28 {
            let edge = x == 4 || x == 27 || y == 6 || y == 23;
            if edge {
                set(&mut rgba, x, y, accent);
            }
        }
    }
    // Stand.
    for y in 24..27 {
        for x in 13..19 {
            set(&mut rgba, x, y, accent);
        }
    }
    for x in 9..23 {
        set(&mut rgba, x, 27, accent);
    }

    Icon::from_rgba(rgba, SIZE, SIZE).expect("icon buffer is the right size")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_match_the_mockup_menu() {
        assert_eq!(TrayAction::Show.label(), "Show Skreenerino");
        assert_eq!(TrayAction::StopSharing.label(), "Stop Sharing");
        assert_eq!(TrayAction::Exit.label(), "Exit");
    }

    #[test]
    fn exit_stops_sharing_before_leaving() {
        let commands = commands_for(TrayAction::Exit);
        assert_eq!(commands.len(), 2);
        assert!(matches!(commands[0], UiCommand::StopShare));
        assert!(matches!(commands[1], UiCommand::Leave));
    }

    #[test]
    fn show_sends_no_commands() {
        assert!(commands_for(TrayAction::Show).is_empty());
    }

    #[test]
    fn icon_buffer_matches_declared_size() {
        // 32x32 RGBA must be exactly 4096 bytes or `Icon::from_rgba` rejects it.
        const SIZE: u32 = 32;
        assert_eq!((SIZE * SIZE * 4) as usize, 4096);
        let icon = build_icon();
        let _ = icon;
    }
}
