//! System tray icon (Windows only). The window can close to the tray while the replay
//! watcher keeps running.

#[cfg(windows)]
use std::sync::mpsc;
use std::sync::mpsc::Receiver;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(windows), allow(dead_code))]
pub enum TrayAction {
    Open,
    ScanNow,
    ToggleWatch,
    ToggleOverlay,
    Quit,
}

pub struct Tray {
    actions: Receiver<TrayAction>,
    #[cfg(windows)]
    inner: windows_tray::Inner,
}

impl Tray {
    pub fn poll(&self) -> Option<TrayAction> {
        self.actions.try_recv().ok()
    }

    #[cfg(windows)]
    pub fn set_status(&self, tooltip: &str, watching: bool, overlay: bool) {
        self.inner.set_status(tooltip, watching, overlay);
    }

    #[cfg(not(windows))]
    pub fn set_status(&self, _tooltip: &str, _watching: bool, _overlay: bool) {}
}

#[cfg(windows)]
pub fn create(ctx: &eframe::egui::Context, watching: bool, overlay: bool) -> Option<Tray> {
    let (tx, rx) = mpsc::channel();
    let inner = windows_tray::Inner::new(ctx.clone(), tx, watching, overlay)?;
    Some(Tray { actions: rx, inner })
}

#[cfg(not(windows))]
pub fn create(_ctx: &eframe::egui::Context, _watching: bool, _overlay: bool) -> Option<Tray> {
    None
}

#[cfg(windows)]
mod windows_tray {
    use std::sync::mpsc::Sender;

    use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

    use super::TrayAction;
    use crate::{icon, platform, APP_NAME};

    pub struct Inner {
        icon: TrayIcon,
        watch_item: CheckMenuItem,
        overlay_item: CheckMenuItem,
    }

    impl Inner {
        pub fn new(ctx: eframe::egui::Context, tx: Sender<TrayAction>, watching: bool, overlay: bool) -> Option<Self> {
            let open = MenuItem::new("Open", true, None);
            let scan = MenuItem::new("Scan replays now", true, None);
            let watch_item = CheckMenuItem::new("Watch for new matches", true, watching, None);
            let overlay_item = CheckMenuItem::new("Show overlay in game", true, overlay, None);
            let quit = MenuItem::new("Quit", true, None);
            let menu = Menu::new();
            menu.append_items(&[&open, &scan, &watch_item, &overlay_item, &PredefinedMenuItem::separator(), &quit])
                .ok()?;

            let ids = [
                (open.id().clone(), TrayAction::Open),
                (scan.id().clone(), TrayAction::ScanNow),
                (watch_item.id().clone(), TrayAction::ToggleWatch),
                (overlay_item.id().clone(), TrayAction::ToggleOverlay),
                (quit.id().clone(), TrayAction::Quit),
            ];
            let menu_tx = tx.clone();
            let menu_ctx = ctx.clone();
            MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
                if let Some((_, action)) = ids.iter().find(|(id, _)| *id == event.id) {
                    if *action == TrayAction::Open {
                        platform::show_main_window();
                    }
                    let _ = menu_tx.send(*action);
                    menu_ctx.request_repaint();
                }
            }));
            TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
                if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } =
                    event
                {
                    platform::show_main_window();
                    let _ = tx.send(TrayAction::Open);
                    ctx.request_repaint();
                }
            }));

            let size = 32;
            let icon = Icon::from_rgba(icon::chest_rgba(size), size, size).ok()?;
            let icon = TrayIconBuilder::new()
                .with_tooltip(APP_NAME)
                .with_icon(icon)
                .with_menu(Box::new(menu))
                .with_menu_on_left_click(false)
                .build()
                .ok()?;
            Some(Self { icon, watch_item, overlay_item })
        }

        pub fn set_status(&self, tooltip: &str, watching: bool, overlay: bool) {
            let _ = self.icon.set_tooltip(Some(tooltip));
            self.watch_item.set_checked(watching);
            self.overlay_item.set_checked(overlay);
        }
    }
}
