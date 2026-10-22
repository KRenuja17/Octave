use eframe::egui;
use octave_core::lyrics::Lyrics;
use octave_provider::{
    Error,
    translation::{TranslatedLyrics, Translator, valid_endpoint},
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver},
};
use zeroize::Zeroizing;
#[derive(Clone, Copy, Default, PartialEq)]
enum Mode {
    #[default]
    Original,
    Translation,
    Both,
}
pub struct TranslationUi {
    pub endpoint: String,
    pub target: String,
    key: Zeroizing<String>,
    query: String,
    mode: Mode,
    original: Option<Lyrics>,
    context: (String, String),
    result: Option<TranslatedLyrics>,
    rx: Option<Receiver<Result<TranslatedLyrics, Error>>>,
    cancel: Arc<AtomicBool>,
    message: String,
    cache: std::collections::VecDeque<TranslatedLyrics>,
}
impl Default for TranslationUi {
    fn default() -> Self {
        Self {
            endpoint: String::new(),
            target: "en".into(),
            key: Zeroizing::new(String::new()),
            query: String::new(),
            mode: Mode::Original,
            original: None,
            context: Default::default(),
            result: None,
            rx: None,
            cancel: Default::default(),
            message: String::new(),
            cache: Default::default(),
        }
    }
}
impl Drop for TranslationUi {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
}
impl TranslationUi {
    pub fn settings(&mut self, ui: &mut egui::Ui) {
        ui.strong("Lyric translation · LibreTranslate");
        ui.weak("Use your chosen service or local server. Only Translate submits lyric text; opening a view does not.");
        ui.add(
            egui::TextEdit::singleline(&mut self.endpoint)
                .hint_text("https://your-service.example or http://127.0.0.1:5000")
                .char_limit(512),
        );
        ui.add(
            egui::TextEdit::singleline(&mut *self.key)
                .password(true)
                .hint_text("Optional API key · kept in memory until exit")
                .char_limit(512),
        );
        ui.horizontal(|ui| {
            ui.label("Target language code");
            ui.add(
                egui::TextEdit::singleline(&mut self.target)
                    .desired_width(80.0)
                    .char_limit(12),
            );
        });
        if !self.endpoint.is_empty() && !valid_endpoint(&self.endpoint) {
            ui.weak(
                "Use HTTPS, or loopback HTTP for a local server. Put API keys in the key field.",
            );
        }
        ui.weak("Service language availability, terms and request limits depend on your endpoint.");
    }
    pub fn header(&mut self, ui: &mut egui::Ui, document: Option<&Lyrics>) -> bool {
        let mut changed = false;
        let context = (self.endpoint.clone(), self.target.clone());
        if self.original.as_ref() != document || self.context != context {
            if self.context.0 != self.endpoint {
                self.cache.clear();
            }
            self.cancel.store(true, Ordering::Release);
            self.rx = None;
            self.original = document.cloned();
            self.context = context;
            self.message.clear();
            self.result = self
                .cache
                .iter()
                .find(|result| Some(&result.original) == document && result.target == self.target)
                .cloned();
            changed = true;
        }
        if let Some(result) = self.rx.as_ref().and_then(|rx| match rx.try_recv() {
            Ok(value) => Some(value),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err(Error::Network)),
        }) {
            self.rx = None;
            match result {
                Ok(result) if Some(&result.original)==document&&result.target==self.target=>{
                    if self.cache.len()==4{self.cache.pop_front();}self.cache.push_back(result.clone());self.result=Some(result);self.message.clear();changed=true;
                },
                _=>self.message="Translation unavailable. Original lyrics remain available; check endpoint, key, language and connection.".into(),
            }
        }
        if self.endpoint.is_empty() {
            ui.collapsing("Translate lyrics", |ui| {
                ui.weak(
                    "Choose a LibreTranslate endpoint in Settings, then request translation here.",
                );
            });
            return changed;
        }
        let before = self.mode;
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.query)
                    .hint_text("Find translation language")
                    .desired_width(150.0)
                    .char_limit(40),
            );
            egui::ComboBox::from_label("Language")
                .selected_text(&self.target)
                .show_ui(ui, |ui| {
                    let query = self.query.to_lowercase();
                    for (code, name) in [
                        ("en", "English"),
                        ("es", "Spanish"),
                        ("fr", "French"),
                        ("de", "German"),
                        ("it", "Italian"),
                        ("pt", "Portuguese"),
                        ("ru", "Russian"),
                        ("ja", "Japanese"),
                        ("ko", "Korean"),
                        ("zh", "Chinese"),
                        ("ar", "Arabic"),
                        ("hi", "Hindi"),
                        ("si", "Sinhala"),
                    ] {
                        if query.is_empty()
                            || code.contains(&query)
                            || name.to_lowercase().contains(&query)
                        {
                            ui.selectable_value(&mut self.target, code.into(), name);
                        }
                    }
                });
            if ui
                .add_enabled(
                    document.is_some() && valid_endpoint(&self.endpoint) && self.rx.is_none(),
                    egui::Button::new("Translate via LibreTranslate"),
                )
                .clicked()
                && let Some(document) = document.cloned()
            {
                self.cancel.store(true, Ordering::Release);
                let cancel = Arc::new(AtomicBool::new(false));
                self.cancel = cancel.clone();
                let endpoint = self.endpoint.clone();
                let key = self.key.to_string();
                let target = self.target.clone();
                let (tx, rx) = mpsc::sync_channel(1);
                self.rx = Some(rx);
                self.context = (endpoint.clone(), target.clone());
                self.mode = Mode::Both;
                std::thread::spawn(move || {
                    let result = Translator::new(&endpoint, key).and_then(|translator| {
                        translator.translate(&document, &target, || cancel.load(Ordering::Acquire))
                    });
                    let _ = tx.send(result);
                });
            }
            ui.selectable_value(&mut self.mode, Mode::Original, "Original");
            ui.add_enabled_ui(self.result.is_some(), |ui| {
                ui.selectable_value(&mut self.mode, Mode::Translation, "Translation");
                ui.selectable_value(&mut self.mode, Mode::Both, "Both");
            });
        });
        if !valid_endpoint(&self.endpoint) {
            ui.weak("Configure a translation endpoint in Settings.");
        }
        if self.rx.is_some() {
            ui.spinner();
            ui.weak("Translating · original stays available");
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }
        if self.result.is_some() {
            ui.weak("Translation: configured LibreTranslate service");
        }
        if !self.message.is_empty() {
            ui.weak(&self.message);
        }
        changed || before != self.mode
    }
    pub fn line<'a>(&'a self, index: usize, original: &'a str) -> (&'a str, Option<&'a str>) {
        let translation = self
            .result
            .as_ref()
            .filter(|r| r.target == self.target)
            .and_then(|r| r.lines.get(index))
            .map(String::as_str);
        match self.mode {
            Mode::Original => (original, None),
            Mode::Translation => (translation.unwrap_or(original), None),
            Mode::Both => (original, translation),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn doc(id: &str) -> Lyrics {
        Lyrics {
            track_id: octave_core::TrackId(id.into()),
            text: "Original fixture".into(),
            source: "Test".into(),
            cues: Vec::new(),
        }
    }
    fn frame(app: &mut TranslationUi, document: &Lyrics) {
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            app.header(ui, Some(document));
        });
        output.textures_delta.clear();
    }
    #[test]
    fn old_song_responses_are_discarded_and_failed_refresh_keeps_original_and_translation() {
        let mut app = TranslationUi::default();
        app.endpoint.push_str("https://translate.example.com");
        let first = doc("first");
        let next = doc("next");
        frame(&mut app, &first);
        let (tx, rx) = mpsc::sync_channel(1);
        app.rx = Some(rx);
        tx.send(Ok(TranslatedLyrics {
            original: first,
            target: "en".into(),
            lines: vec!["Stale translation".into()],
        }))
        .unwrap();
        frame(&mut app, &next);
        assert!(app.result.is_none() && app.rx.is_none());
        let result = TranslatedLyrics {
            original: next.clone(),
            target: "en".into(),
            lines: vec!["Current translation".into()],
        };
        app.result = Some(result.clone());
        app.mode = Mode::Both;
        let (tx, rx) = mpsc::sync_channel(1);
        app.rx = Some(rx);
        tx.send(Err(Error::Timeout)).unwrap();
        frame(&mut app, &next);
        assert_eq!(app.result, Some(result));
        assert_eq!(
            app.line(0, "Original fixture"),
            ("Original fixture", Some("Current translation"))
        );
        app.endpoint = "https://different.example.com".into();
        frame(&mut app, &next);
        assert!(app.result.is_none() && app.cache.is_empty());
    }
}
