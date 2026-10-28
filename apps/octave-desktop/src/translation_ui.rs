use eframe::egui;
use octave_core::lyrics::Lyrics;
use octave_provider::{
    Error,
    translation::{Language, TranslatedLyrics, Translator, valid_endpoint},
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver},
};
use zeroize::{Zeroize, Zeroizing};
type LanguageResponse = (String, Receiver<Result<Vec<Language>, Error>>);
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
    languages: Vec<Language>,
    language_endpoint: String,
    language_rx: Option<LanguageResponse>,
    language_cancel: Arc<AtomicBool>,
    language_message: String,
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
            languages: Vec::new(),
            language_endpoint: String::new(),
            language_rx: None,
            language_cancel: Default::default(),
            language_message: String::new(),
        }
    }
}
impl Drop for TranslationUi {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
        self.language_cancel.store(true, Ordering::Release);
    }
}
impl TranslationUi {
    fn sync_endpoint(&mut self) {
        if self.language_endpoint == self.endpoint {
            return;
        }
        if !self.language_endpoint.is_empty() {
            self.key.zeroize();
        }
        self.cancel.store(true, Ordering::Release);
        self.language_cancel.store(true, Ordering::Release);
        self.rx = None;
        self.language_rx = None;
        self.result = None;
        self.cache.clear();
        self.languages.clear();
        self.message.clear();
        self.language_message.clear();
        self.language_endpoint = self.endpoint.clone();
        self.context = (self.endpoint.clone(), self.target.clone());
    }
    pub fn cancel_for(&mut self, id: Option<&octave_core::TrackId>) {
        self.sync_endpoint();
        if self.context.1 != self.target {
            self.cancel.store(true, Ordering::Release);
            self.rx = None;
            self.result = None;
            self.message.clear();
        }
        if self.original.as_ref().map(|document| &document.track_id) != id {
            self.cancel.store(true, Ordering::Release);
            self.rx = None;
            self.original = None;
            self.result = None;
            self.message.clear();
        }
    }
    fn poll_languages(&mut self, ctx: &egui::Context) {
        self.sync_endpoint();
        let result = self
            .language_rx
            .as_ref()
            .and_then(|(endpoint, rx)| match rx.try_recv() {
                Ok(result) => Some((endpoint.clone(), result)),
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some((endpoint.clone(), Err(Error::Network)))
                }
                Err(mpsc::TryRecvError::Empty) => None,
            });
        if let Some((endpoint, result)) = result {
            self.language_rx = None;
            if endpoint == self.endpoint {
                match result {
                    Ok(languages) => {
                        self.language_message = format!(
                            "{} server languages confirmed · no lyrics submitted",
                            languages.len()
                        );
                        self.languages = languages;
                    }
                    Err(_) => self.language_message =
                        "Could not read this server's languages. Check its URL and availability."
                            .into(),
                }
            }
        }
        if self.language_rx.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }
    pub fn settings(&mut self, ui: &mut egui::Ui) {
        self.poll_languages(ui.ctx());
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
            let label = ui.label("Target language code");
            ui.add(
                egui::TextEdit::singleline(&mut self.target)
                    .desired_width(80.0)
                    .char_limit(12),
            )
            .labelled_by(label.id);
        });
        if !self.endpoint.is_empty() && !valid_endpoint(&self.endpoint) {
            ui.weak(
                "Use HTTPS, or loopback HTTP for a local server. Put API keys in the key field.",
            );
        }
        if ui
            .add_enabled(
                valid_endpoint(&self.endpoint) && self.language_rx.is_none(),
                egui::Button::new("Check server languages"),
            )
            .clicked()
        {
            let endpoint = self.endpoint.clone();
            self.language_cancel.store(true, Ordering::Release);
            let cancel = Arc::new(AtomicBool::new(false));
            self.language_cancel = cancel.clone();
            let (tx, rx) = mpsc::sync_channel(1);
            self.language_rx = Some((endpoint.clone(), rx));
            std::thread::spawn(move || {
                let _ = tx.send(
                    Translator::new(&endpoint, String::new()).and_then(|server| {
                        server.languages_cancellable(|| cancel.load(Ordering::Acquire))
                    }),
                );
            });
        }
        if self.language_rx.is_some() {
            ui.spinner();
            if ui.small_button("Cancel language check").clicked() {
                self.language_cancel.store(true, Ordering::Release);
                self.language_rx = None;
                self.language_message = "Language check canceled.".into();
            }
        }
        if !self.language_message.is_empty() {
            ui.weak(&self.language_message);
        }
        if !self.languages.is_empty() {
            ui.collapsing("Server language names and codes", |ui| {
                egui::ScrollArea::vertical()
                    .max_height(160.0)
                    .show(ui, |ui| {
                        for language in &self.languages {
                            ui.label(format!("{} · {}", language.name, language.code));
                        }
                    });
            });
        }
        ui.weak("Service language availability, terms and request limits depend on your endpoint.");
        ui.horizontal_wrapped(|ui| {
            ui.hyperlink_to("Hosted API & keys", "https://portal.libretranslate.com/");
            ui.hyperlink_to(
                "Local server setup",
                "https://docs.libretranslate.com/guides/installation/",
            );
        });
        if (!self.endpoint.is_empty() || !self.key.is_empty())
            && ui.button("Clear translation service and key").clicked()
        {
            self.endpoint.clear();
            self.key.zeroize();
        }
        self.sync_endpoint();
    }
    pub fn header(&mut self, ui: &mut egui::Ui, document: Option<&Lyrics>) -> bool {
        self.poll_languages(ui.ctx());
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
                    let common = [
                        ("en", "English"),
                        ("es", "Spanish"),
                        ("fr", "French"),
                        ("de", "German"),
                        ("it", "Italian"),
                        ("pt", "Portuguese"),
                        ("ru", "Russian"),
                        ("ja", "Japanese"),
                        ("ko", "Korean"),
                        ("zh-Hans", "Chinese (Simplified)"),
                        ("zh-Hant", "Chinese (Traditional)"),
                        ("ar", "Arabic"),
                        ("hi", "Hindi"),
                    ];
                    let choices: Vec<_> = if self.languages.is_empty() {
                        common.to_vec()
                    } else {
                        self.languages
                            .iter()
                            .map(|l| (l.code.as_str(), l.name.as_str()))
                            .collect()
                    };
                    for (code, name) in choices {
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
        if self.languages.is_empty() {
            ui.weak("Common language choices · use Check server languages in Settings to confirm availability.");
        }
        if self.rx.is_some() {
            ui.spinner();
            ui.weak("Translating · original stays available");
            if ui.small_button("Cancel translation").clicked() {
                self.cancel.store(true, Ordering::Release);
                self.rx = None;
                self.message = "Translation canceled. Original lyrics remain available.".into();
            }
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
    #[test]
    fn translation_settings_expose_language_label_and_mask_api_key() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut app = TranslationUi::default();
        app.key = Zeroizing::new("synthetic-secret-for-accessibility-test".into());
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| app.settings(ui));
        output.textures_delta.clear();
        let tree = output.platform_output.accesskit_update.unwrap();
        let (_, field) = tree
            .nodes
            .iter()
            .find(|(_, n)| {
                n.role() == egui::accesskit::Role::TextInput && !n.labelled_by().is_empty()
            })
            .unwrap();
        let (_, label) = tree
            .nodes
            .iter()
            .find(|(id, _)| field.labelled_by().contains(id))
            .unwrap();
        let label_text: String = label
            .children()
            .iter()
            .map(|id| {
                tree.nodes
                    .iter()
                    .find(|(node_id, _)| node_id == id)
                    .unwrap()
                    .1
                    .value()
                    .unwrap_or_default()
            })
            .collect();
        assert_eq!(label_text, "Target language code");
        assert!(
            tree.nodes
                .iter()
                .any(|(_, n)| n.role() == egui::accesskit::Role::PasswordInput)
        );
        for (_, node) in &tree.nodes {
            assert!(
                !node
                    .value()
                    .unwrap_or_default()
                    .contains("synthetic-secret")
            );
            assert!(
                !node
                    .label()
                    .unwrap_or_default()
                    .contains("synthetic-secret")
            );
        }
    }
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
    fn hidden_track_changes_cancel_translation_and_endpoint_changes_clear_old_keys() {
        let mut app = TranslationUi::default();
        app.endpoint = "https://first.example.com".into();
        let first = doc("first");
        frame(&mut app, &first);
        app.key = Zeroizing::new("first-service-secret".into());
        let pending = Arc::new(AtomicBool::new(false));
        app.cancel = pending.clone();
        let (_, rx) = mpsc::channel();
        app.rx = Some(rx);
        app.cancel_for(Some(&doc("second").track_id));
        assert!(pending.load(Ordering::Acquire) && app.rx.is_none());
        assert!(
            !app.key.is_empty(),
            "Changing track does not change the service key"
        );
        let metadata = Arc::new(AtomicBool::new(false));
        app.language_cancel = metadata.clone();
        let (_, rx) = mpsc::channel();
        app.language_rx = Some((app.endpoint.clone(), rx));
        app.endpoint = "https://second.example.com".into();
        app.cancel_for(None);
        assert!(metadata.load(Ordering::Acquire));
        assert!(app.key.is_empty() && app.language_rx.is_none() && app.languages.is_empty());
        assert!(app.result.is_none() && app.cache.is_empty());
    }
    #[test]
    fn language_metadata_from_a_previous_endpoint_is_discarded() {
        let mut app = TranslationUi::default();
        app.endpoint = "https://first.example.com".into();
        let ctx = egui::Context::default();
        app.poll_languages(&ctx);
        let (tx, rx) = mpsc::channel();
        app.language_rx = Some((app.endpoint.clone(), rx));
        app.endpoint = "https://second.example.com".into();
        tx.send(Ok(vec![Language {
            code: "en".into(),
            name: "English".into(),
        }]))
        .unwrap();
        app.poll_languages(&ctx);
        assert!(app.language_rx.is_none() && app.languages.is_empty());
        let (tx, rx) = mpsc::channel();
        app.language_rx = Some((app.endpoint.clone(), rx));
        tx.send(Ok(vec![Language {
            code: "es".into(),
            name: "Spanish".into(),
        }]))
        .unwrap();
        app.poll_languages(&ctx);
        assert_eq!(app.languages[0].code, "es");
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
