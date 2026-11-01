//! Optional local Rich Presence. No OAuth, bot token, or Discord account storage.
mod ipc;
#[cfg(test)]
mod tests;

use octave_core::{Track, TrackId};
use octave_player::{PlaybackState, Snapshot};
use serde_json::{Value, json};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use unicode_segmentation::UnicodeSegmentation;

// Public application identifier, provided by Octave's maintainer. Not a token.
const APPLICATION_ID: &str = "1566363175241125928";

pub fn valid_id(id: &str) -> bool {
    (17..=20).contains(&id.len())
        && !id.starts_with('0')
        && id.bytes().all(|b| b.is_ascii_digit())
        && id.parse::<u64>().is_ok()
}

fn clip(text: &str, fallback: &str) -> String {
    let text: String = text.chars().filter(|c| !c.is_control()).collect();
    let mut result = String::new();
    for grapheme in text.trim().graphemes(true) {
        if result.len() + grapheme.len() > 128 {
            break;
        }
        result.push_str(grapheme);
    }
    if result.is_empty() {
        fallback.into()
    } else {
        result
    }
}

struct Anchor {
    id: TrackId,
    sequence: u64,
    position: Duration,
    observed: Instant,
    start: u64,
}
#[derive(Default)]
struct Clock(Option<Anchor>);
impl Clock {
    fn activity(
        &mut self,
        track: Option<&Track>,
        snapshot: &Snapshot,
        wall: Duration,
        now: Instant,
    ) -> Option<Value> {
        let Some(track) = track.filter(|track| {
            !snapshot.buffering
                && matches!(&snapshot.state, PlaybackState::Playing(id) if id == &track.id)
        }) else {
            self.0 = None;
            return None;
        };
        // Keep a stable epoch during ordinary consumption. Re-anchor on seek,
        // repeat/crossfade sequence changes, resume, or significant clock drift.
        let continuous = self.0.as_ref().is_some_and(|anchor| {
            anchor.id == track.id
                && anchor.sequence == snapshot.sequence
                && anchor
                    .position
                    .saturating_add(now.duration_since(anchor.observed))
                    .abs_diff(snapshot.position)
                    <= Duration::from_secs(1)
        });
        if !continuous {
            self.0 = Some(Anchor {
                id: track.id.clone(),
                sequence: snapshot.sequence,
                position: snapshot.position,
                observed: now,
                start: wall.saturating_sub(snapshot.position).as_secs(),
            });
        }
        let start = self.0.as_ref()?.start;
        let mut value = json!({
            "type": 2, "details": clip(&track.title, "Unknown title"),
            "state": clip(&track.artists.join(" · "), "Unknown artist"),
            "timestamps": {"start": start}, "instance": false
        });
        if !snapshot.duration.is_zero() && snapshot.position < snapshot.duration {
            value["timestamps"]["end"] = json!(start.saturating_add(snapshot.duration.as_secs()));
        }
        if track.id.0.len() == 11
            && track
                .id
                .0
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
        {
            let url = format!("https://music.youtube.com/watch?v={}", track.id.0);
            value["details_url"] = json!(url);
            value["buttons"] = json!([{"label": "Listen on YouTube Music", "url": url}]);
            value["assets"] = json!({
                "large_image": format!("https://i.ytimg.com/vi/{}/hqdefault.jpg", track.id.0),
                "large_text": "Octave", "large_url": url
            });
        }
        Some(value)
    }
}

#[derive(Default)]
pub struct Presence {
    worker: Option<ipc::Worker>,
    clock: Clock,
}
impl Presence {
    pub fn settings(&self, ui: &mut eframe::egui::Ui, enabled: &mut bool, id: &mut String) {
        use eframe::egui;
        ui.separator();
        ui.strong("Discord Rich Presence");
        ui.weak("Show the song you’re listening to on your Discord profile.");
        let label = ui.label("Custom Discord application ID (optional)");
        let input = ui
            .add(
                egui::TextEdit::singleline(id)
                    .hint_text("Leave empty to use Octave")
                    .char_limit(20),
            )
            .labelled_by(label.id);
        // Changing the destination requires an explicit opt-in again.
        if input.changed() {
            *enabled = false;
        }
        if !id.is_empty() && !valid_id(id) {
            ui.weak("Enter the numeric Application ID from Discord’s Developer Portal.");
        }
        ui.add_enabled(
            id.is_empty() || valid_id(id) || *enabled,
            egui::Checkbox::new(enabled, "Share current song on Discord"),
        );
        ui.weak("Off by default. Shares title, artist, artwork, playback timing and a song link.");
        ui.weak(
            "Pause, buffering, stop and disabling sharing clear the song. Discord must be running.",
        );
        ui.horizontal_wrapped(|ui| {
            ui.hyperlink_to(
                "Developer Portal",
                "https://discord.com/developers/applications",
            );
            if ui.button("Reset Discord setup").clicked() {
                *enabled = false;
                id.clear();
            }
        });
        ui.weak("Uses Octave’s application when empty. Never enter a bot token or client secret.");
        ui.label(self.status());
        ui.weak(
            "Discord controls visibility. Invisible status or activity privacy can hide the card.",
        );
        ui.hyperlink_to("Discord visibility help", "https://support.discord.com/hc/en-us/articles/7931156448919-Activity-Sharing-on-Discord-FAQ");
    }
    pub fn update(&mut self, enabled: bool, id: &str, track: Option<&Track>, snapshot: &Snapshot) {
        let id = if id.is_empty() { APPLICATION_ID } else { id };
        let desired = if enabled && valid_id(id) {
            ipc::Desired {
                id: id.into(),
                activity: self.clock.activity(
                    track,
                    snapshot,
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default(),
                    Instant::now(),
                ),
            }
        } else {
            self.clock.0 = None;
            ipc::Desired::default()
        };
        if self.worker.is_none() && !desired.id.is_empty() {
            self.worker = Some(ipc::Worker::start(desired.clone()));
        }
        if let Some(worker) = &self.worker {
            worker.update(desired);
        }
    }
    pub fn status(&self) -> String {
        self.worker
            .as_ref()
            .map(|worker| worker.status())
            .unwrap_or_else(|| "Disabled".into())
    }
}
