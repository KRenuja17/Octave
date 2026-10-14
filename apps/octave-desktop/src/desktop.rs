//! Native callbacks enqueue commands; the UI thread owns all playback state.
use octave_core::Track;
use octave_player::{PlaybackState, Snapshot};
use std::time::Duration;
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Play,
    Pause,
    Toggle,
    Next,
    Previous,
    Stop,
    Seek(Duration),
    Show,
    Quit,
}

#[cfg(target_os = "windows")]
mod native {
    use super::*;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use souvlaki::{
        MediaControlEvent as Event, MediaControls, MediaMetadata, MediaPlayback, MediaPosition,
        PlatformConfig,
    };
    use std::sync::mpsc::{self, Receiver};
    use tray_icon::{
        TrayIcon, TrayIconBuilder, TrayIconEvent,
        menu::{Menu, MenuEvent, MenuItem},
    };
    pub struct Desktop {
        controls: Option<MediaControls>,
        _tray: Option<TrayIcon>,
        events: Receiver<Command>,
        items: Vec<(MenuItem, Command)>,
        last: Option<(Option<Track>, u64, MediaPlayback)>,
        pub close_to_tray: bool,
        pub status: String,
    }
    impl Drop for Desktop {
        fn drop(&mut self) {
            MenuEvent::set_event_handler(None::<fn(MenuEvent)>);
            TrayIconEvent::set_event_handler(None::<fn(TrayIconEvent)>);
        }
    }
    impl Desktop {
        pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
            let (tx, events) = mpsc::channel();
            let hwnd = cc.window_handle().ok().and_then(|h| {
                if let RawWindowHandle::Win32(w) = h.as_raw() {
                    Some(w.hwnd.get() as *mut std::ffi::c_void)
                } else {
                    None
                }
            });
            let mut controls = hwnd.and_then(|hwnd| {
                MediaControls::new(PlatformConfig {
                    dbus_name: "octave",
                    display_name: "Octave",
                    hwnd: Some(hwnd),
                })
                .ok()
            });
            if let Some(media) = controls.as_mut() {
                let tx = tx.clone();
                let ctx = cc.egui_ctx.clone();
                if media
                    .attach(move |event| {
                        let command = match event {
                            Event::Play => Some(Command::Play),
                            Event::Pause => Some(Command::Pause),
                            Event::Toggle => Some(Command::Toggle),
                            Event::Next => Some(Command::Next),
                            Event::Previous => Some(Command::Previous),
                            Event::Stop => Some(Command::Stop),
                            Event::SetPosition(p) => Some(Command::Seek(p.0)),
                            Event::Raise => Some(Command::Show),
                            Event::Quit => Some(Command::Quit),
                            _ => None,
                        };
                        if let Some(command) = command {
                            let _ = tx.send(command);
                            ctx.request_repaint();
                        }
                    })
                    .is_err()
                {
                    controls = None;
                }
            }
            let menu = Menu::new();
            let items: Vec<_> = [
                ("Show Octave", Command::Show),
                ("Play / pause", Command::Toggle),
                ("Previous", Command::Previous),
                ("Next", Command::Next),
                ("Stop", Command::Stop),
                ("Quit Octave", Command::Quit),
            ]
            .into_iter()
            .map(|(label, command)| (MenuItem::new(label, true, None), command))
            .collect();
            let mut menu_ok = true;
            for (item, _) in &items {
                menu_ok &= menu.append(item).is_ok();
            }
            let mut pixels = Vec::with_capacity(32 * 32 * 4);
            for y in 0..32_i32 {
                for x in 0..32_i32 {
                    let ring = (x - 16).pow(2) + (y - 16).pow(2);
                    pixels.extend_from_slice(if (49..169).contains(&ring) {
                        &[198, 236, 163, 255]
                    } else {
                        &[16, 21, 25, 255]
                    });
                }
            }
            let tray = if menu_ok {
                tray_icon::Icon::from_rgba(pixels, 32, 32)
                    .ok()
                    .and_then(|icon| {
                        TrayIconBuilder::new()
                            .with_tooltip("Octave")
                            .with_icon(icon)
                            .with_menu(Box::new(menu))
                            .build()
                            .ok()
                    })
            } else {
                None
            };
            // Menu events are forwarded through our own channel because installing a handler disables the global receiver.
            let mapping: Vec<_> = items
                .iter()
                .map(|(item, command)| (item.id().clone(), command.clone()))
                .collect();
            let menu_tx = tx.clone();
            let ctx = cc.egui_ctx.clone();
            MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
                if let Some((_, command)) = mapping.iter().find(|(id, _)| *id == event.id) {
                    let _ = menu_tx.send(command.clone());
                    ctx.request_repaint();
                }
            }));
            let ctx = cc.egui_ctx.clone();
            TrayIconEvent::set_event_handler(Some(move |event| {
                if matches!(event, TrayIconEvent::DoubleClick { .. }) {
                    let _ = tx.send(Command::Show);
                    ctx.request_repaint();
                }
            }));
            let status = match (controls.is_some(), tray.is_some()) {
                (true, true) => "Windows media controls and tray ready.",
                (false, true) => "Tray ready; Windows media controls unavailable.",
                (true, false) => "Media controls ready; tray unavailable (closing will quit).",
                _ => "Windows desktop integration unavailable (closing will quit).",
            }
            .into();
            Self {
                controls,
                _tray: tray,
                events,
                items,
                last: None,
                close_to_tray: false,
                status,
            }
        }
        pub fn can_hide(&self) -> bool {
            self._tray.is_some()
        }
        pub fn drain(&self) -> Vec<Command> {
            self.events.try_iter().collect()
        }
        pub fn update(&mut self, track: Option<&Track>, snapshot: &Snapshot) {
            let playback = match snapshot.state {
                PlaybackState::Playing(_) => MediaPlayback::Playing {
                    progress: Some(MediaPosition(Duration::from_secs(
                        snapshot.position.as_secs(),
                    ))),
                },
                PlaybackState::Paused(_) | PlaybackState::Loading(_) => MediaPlayback::Paused {
                    progress: Some(MediaPosition(Duration::from_secs(
                        snapshot.position.as_secs(),
                    ))),
                },
                _ => MediaPlayback::Stopped,
            };
            let current = (
                track.cloned(),
                snapshot.duration.as_secs(),
                playback.clone(),
            );
            if self.last.as_ref() == Some(&current) {
                return;
            }
            if let Some(controls) = self.controls.as_mut() {
                let artist = track.map(|t| t.artists.join(", "));
                let metadata_changed = self
                    .last
                    .as_ref()
                    .is_none_or(|last| last.0 != current.0 || last.1 != current.1);
                let metadata = !metadata_changed
                    || controls
                        .set_metadata(MediaMetadata {
                            title: track.map(|t| t.title.as_str()),
                            artist: artist.as_deref(),
                            duration: Some(snapshot.duration),
                            ..Default::default()
                        })
                        .is_ok();
                if !metadata || controls.set_playback(playback).is_err() {
                    self.status =
                        "Windows media state update failed. App controls remain available.".into();
                    return;
                }
            }
            for (item, command) in &self.items {
                if !matches!(command, Command::Show | Command::Quit) {
                    item.set_enabled(track.is_some());
                }
            }
            self.last = Some(current);
        }
    }
}
#[cfg(target_os = "windows")]
pub use native::Desktop;
#[cfg(not(target_os = "windows"))]
pub struct Desktop {
    pub close_to_tray: bool,
    pub status: String,
}
#[cfg(not(target_os = "windows"))]
impl Desktop {
    pub fn new(_: &eframe::CreationContext<'_>) -> Self {
        Self {
            close_to_tray: false,
            status: "Desktop integration currently targets Windows.".into(),
        }
    }
    pub fn can_hide(&self) -> bool {
        false
    }
    pub fn drain(&self) -> Vec<Command> {
        vec![]
    }
    pub fn update(&mut self, _: Option<&Track>, _: &Snapshot) {}
}
