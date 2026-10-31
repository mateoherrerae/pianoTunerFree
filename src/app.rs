//! Ventana principal: ajustes, afinacion guiada, medicion del piano y
//! afinador libre.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, RichText};

use piano_tuner_free::audio::{
    AudioOutput, AudioSource, MicCapture, PartialAnalyzer, PitchDetector, PitchUpdate, PitchWorker,
};
use piano_tuner_free::beats::{self, BeatInfo};
use piano_tuner_free::settings::{Lang, Level, PianoConfig, Settings, StretchSel};
use piano_tuner_free::tuning::inharmonicity::stretch_from_profile;
use piano_tuner_free::tuning::{
    Note, PianoProfile, Session, Temperament, TuningFlow, TuningMode, TuningOrder,
};

use crate::widgets::{self, solfege, strings_color, GREEN, ORANGE};

#[derive(PartialEq)]
enum Screen {
    Home,
    Tuning,
    Free,
    Measure,
}

/// Ultima lectura del microfono.
struct Reading {
    freq: f32,
    cents: f32,
    confidence: f32,
    /// Nota detectada (solo en el afinador libre).
    midi: Option<u8>,
    samples: Vec<f32>,
    at: Instant,
}

#[derive(Clone, Copy, PartialEq)]
enum StepKind {
    /// Apagar cuerdas: no se mide nada.
    Mute,
    /// Afinar una cuerda sola contra el objetivo (las demas apagadas).
    Pitch,
    /// Igualar una cuerda con las ya afinadas (unisono).
    Unison,
}

type Text = (&'static str, &'static str);

/// Un paso de afinacion de una nota.
struct Step {
    kind: StepKind,
    title: Text,
    detail: Text,
}

fn steps_for(strings: u8, remind_mute: bool) -> Vec<Step> {
    let s = |kind, title, detail| Step {
        kind,
        title,
        detail,
    };
    use StepKind::*;
    let tune_hint = (
        "Tocá la tecla y girá la clavija hasta que la barra quede en la zona verde.",
        "Play the key and turn the pin until the bar sits in the green zone.",
    );
    match strings {
        1 => vec![s(Pitch, ("Afiná la cuerda", "Tune the string"), tune_hint)],
        2 => {
            let mut v = Vec::new();
            if remind_mute {
                v.push(s(
                    Mute,
                    ("Apagá la cuerda derecha", "Mute the right string"),
                    (
                        "Poné una cuña de goma o fieltro para que suene solo la izquierda. Cuando esté, apretá «Listo».",
                        "Insert a rubber or felt wedge so only the left string sounds. Then press \"Done\".",
                    ),
                ));
                v.push(s(
                    Pitch,
                    ("Afiná la cuerda izquierda", "Tune the left string"),
                    tune_hint,
                ));
            } else {
                v.push(s(
                    Pitch,
                    ("Izquierda (derecha apagada)", "Left (right muted)"),
                    tune_hint,
                ));
            }
            v.push(s(
                Unison,
                ("Derecha: unísono", "Right: unison"),
                (
                    "Sacá la cuña y afiná la derecha hasta que no haya batido (el \"uaua\").",
                    "Remove the wedge and tune the right string until there are no beats.",
                ),
            ));
            v
        }
        _ => {
            let mut v = Vec::new();
            if remind_mute {
                v.push(s(
                    Mute,
                    ("Apagá las cuerdas de los costados", "Mute the outer strings"),
                    (
                        "Poné fieltro o cuñas para que suene solo la del medio. Cuando esté, apretá «Listo».",
                        "Use a felt strip or wedges so only the center string sounds. Then press \"Done\".",
                    ),
                ));
                v.push(s(
                    Pitch,
                    ("Afiná la cuerda del medio", "Tune the center string"),
                    tune_hint,
                ));
            } else {
                v.push(s(
                    Pitch,
                    ("Medio (costados apagados)", "Center (outer muted)"),
                    tune_hint,
                ));
            }
            v.push(s(
                Unison,
                ("Izquierda: unísono", "Left: unison"),
                (
                    "Liberá la izquierda y afinala contra la del medio hasta que no haya batido.",
                    "Unmute the left string and tune it to the center one until there are no beats.",
                ),
            ));
            v.push(s(
                Unison,
                ("Derecha: unísono", "Right: unison"),
                (
                    "Liberá la derecha y afinala contra las otras dos hasta que no haya batido.",
                    "Unmute the right string and tune it to the other two until there are no beats.",
                ),
            ));
            v
        }
    }
}

/// Teclas que se tocan para medir el piano (repartidas por el teclado).
const MEASURE_KEYS: [u8; 10] = [33, 45, 52, 57, 64, 69, 76, 81, 88, 93];

/// Escucha del La del piano para completar A4 solo.
struct DetectA4 {
    freqs: Vec<f32>,
    last: Option<Instant>,
    started: Instant,
}

/// Lecturas necesarias para dar el La por detectado.
const DETECT_READINGS: usize = 12;

/// Estado de la medicion del piano.
struct Measure {
    profile: PianoProfile,
    index: usize,
    /// Teclas medidas: cantidad de armonicos encontrados.
    done: HashMap<u8, usize>,
    /// Momento en que se capturo la tecla actual (para pasar a la siguiente).
    captured_at: Option<Instant>,
    result: Option<Result<(), String>>,
}

pub struct TunerApp {
    settings: Settings,
    saved_settings: Settings,
    screen: Screen,
    mic: Option<MicCapture>,
    worker: Option<PitchWorker>,
    output: Option<AudioOutput>,
    audio_error: Option<String>,
    devices: Vec<String>,
    flow: Option<TuningFlow>,
    step: usize,
    step_started: Instant,
    reading: Option<Reading>,
    live: bool,
    /// Desde cuando se cumple la condicion de avance automatico.
    lock_since: Option<Instant>,
    beat: Option<BeatInfo>,
    envelope_db: Vec<(f32, f32)>,
    resumable: Option<Session>,
    measure: Option<Measure>,
    /// Desvio de la cuerda afinada sola (lo que se guarda de la nota).
    pitch_cents: Option<f32>,
    /// Preguntando si borrar la afinacion guardada.
    confirm_restart: bool,
    detect_a4: Option<DetectA4>,
    /// Desplazamiento del estroboscopio (px) y ultimo cuadro.
    strobe_phase: f32,
    last_frame: Instant,
}

impl TunerApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut style = (*cc.egui_ctx.style()).clone();
        for (_, font) in style.text_styles.iter_mut() {
            font.size *= 1.15;
        }
        style.spacing.button_padding = egui::vec2(12.0, 6.0);
        cc.egui_ctx.set_style(style);

        let settings = Settings::load();
        settings.apply_string_layout();
        let mut app = Self {
            saved_settings: settings.clone(),
            settings,
            screen: Screen::Home,
            mic: None,
            worker: None,
            output: None,
            audio_error: None,
            devices: MicCapture::input_device_names(),
            flow: None,
            step: 0,
            step_started: Instant::now(),
            reading: None,
            live: false,
            lock_since: None,
            beat: None,
            envelope_db: Vec::new(),
            resumable: Session::load_recent().ok().flatten(),
            measure: None,
            pitch_cents: None,
            confirm_restart: false,
            detect_a4: None,
            strobe_phase: 0.0,
            last_frame: Instant::now(),
        };
        app.ensure_mic();
        app
    }

    fn t(&self, es: &'static str, en: &'static str) -> &'static str {
        self.settings.t(es, en)
    }

    fn pick(&self, text: Text) -> &'static str {
        self.settings.t(text.0, text.1)
    }

    // ------------------------------------------------------------- Audio

    fn ensure_mic(&mut self) {
        if self.worker.is_some() {
            return;
        }
        match MicCapture::with_device(self.settings.mic_device.as_deref()) {
            Ok(mic) => {
                self.worker = Some(PitchWorker::spawn(mic.reader()));
                self.mic = Some(mic);
                self.audio_error = None;
            }
            Err(e) => {
                self.audio_error = Some(format!(
                    "{}: {e}",
                    self.t(
                        "No pude abrir el micrófono",
                        "Could not open the microphone"
                    )
                ))
            }
        }
    }

    fn restart_mic(&mut self) {
        self.worker = None;
        self.mic = None;
        self.audio_error = None;
        self.ensure_mic();
    }

    fn play_reference(&mut self, freq: f32) {
        if self.output.is_none() {
            match AudioOutput::new() {
                Ok(o) => self.output = Some(o),
                Err(e) => {
                    self.audio_error = Some(format!(
                        "{}: {e}",
                        self.t("No pude usar los parlantes", "Could not use the speakers")
                    ));
                    return;
                }
            }
        }
        if let Some(out) = &self.output {
            let _ = out.play_sine(freq, 2.0);
        }
    }

    /// Frecuencia objetivo segun la pantalla.
    fn current_target(&self) -> Option<f32> {
        match self.screen {
            Screen::Tuning => self.flow.as_ref()?.current_note().map(|n| n.target_freq),
            Screen::Measure => {
                let m = self.measure.as_ref()?;
                let midi = *MEASURE_KEYS.get(m.index)?;
                Some(Temperament::with_a4(self.settings.piano.a4).frequency(midi))
            }
            Screen::Home if self.detect_a4.is_some() => Some(440.0),
            _ => None,
        }
    }

    /// Junta lecturas del La y, con suficientes, completa A4 con la mediana.
    fn update_detect_a4(&mut self) {
        let Some(d) = self.detect_a4.as_mut() else {
            return;
        };
        if d.started.elapsed() > Duration::from_secs(30) {
            self.detect_a4 = None;
            return;
        }
        if let Some(r) = &self.reading {
            let new = d.last != Some(r.at);
            if new && self.live && r.confidence > 0.8 && (380.0..=480.0).contains(&r.freq) {
                d.freqs.push(r.freq);
                d.last = Some(r.at);
            }
        }
        if d.freqs.len() >= DETECT_READINGS {
            let mut v = d.freqs.clone();
            v.sort_by(f32::total_cmp);
            let median = v[v.len() / 2];
            self.settings.piano.a4 = (median * 10.0).round() / 10.0;
            self.detect_a4 = None;
        }
    }

    /// Lee el microfono y actualiza la lectura actual y el batido.
    fn poll_audio(&mut self) {
        let target = self.current_target();
        let Some(worker) = &self.worker else { return };
        worker.set_target(target);
        if let Some(w) = worker.take_audio_warning() {
            self.audio_error = Some(format!(
                "{}: {w}",
                self.t("Problema con el micrófono", "Microphone problem")
            ));
        }

        // Curva de volumen (para el batido y el dibujo)
        let env = worker.envelope();
        let now = Instant::now();
        self.envelope_db = env
            .iter()
            .map(|(t, r)| {
                (
                    -(now.duration_since(*t).as_secs_f32()),
                    20.0 * r.max(1e-6).log10(),
                )
            })
            .collect();
        let since_step: Vec<_> = env
            .into_iter()
            .filter(|(t, _)| *t >= self.step_started)
            .collect();
        self.beat = beats::analyze(&since_step);

        match worker.latest_update() {
            Some(PitchUpdate::Detected {
                frequency,
                confidence,
                samples,
            }) if confidence > 0.6 => {
                let (cents, midi) = match target {
                    Some(t) => (
                        Temperament::with_a4(self.settings.piano.a4)
                            .cents_from_target(frequency, t),
                        None,
                    ),
                    None => {
                        match Temperament::with_a4(self.settings.piano.a4).nearest_note(frequency) {
                            Some((m, c)) => (c, Some(m)),
                            None => return,
                        }
                    }
                };
                self.reading = Some(Reading {
                    freq: frequency,
                    cents,
                    confidence,
                    midi,
                    samples,
                    at: now,
                });
                self.live = true;
            }
            Some(PitchUpdate::Silence) => {
                self.live = false;
            }
            _ => {}
        }
    }

    /// Lectura a mostrar: la actual, o la ultima durante `hold_secs`.
    fn shown_reading(&self) -> Option<(&Reading, bool)> {
        let r = self.reading.as_ref()?;
        let age = r.at.elapsed();
        let fresh = self.live && age < Duration::from_millis(300);
        if fresh || age.as_secs_f32() < self.settings.hold_secs {
            Some((r, fresh))
        } else {
            None
        }
    }

    // ------------------------------------------------------------- Sesiones

    fn start_tuning(&mut self) {
        self.settings.apply_string_layout();
        if let Some(old) = self.resumable.take() {
            let _ = old.delete();
        }
        self.flow = Some(TuningFlow::new(
            TuningMode::Concert,
            TuningOrder::new(),
            Temperament::with_a4(self.settings.piano.a4),
            self.settings.piano.stretch_curve(),
        ));
        self.enter(Screen::Tuning);
    }

    fn resume_tuning(&mut self) {
        let Some(session) = self.resumable.take() else {
            return;
        };
        self.settings.apply_string_layout();
        let a4 = session.a4_reference;
        self.flow = Some(TuningFlow::resume(
            session,
            TuningOrder::new(),
            Temperament::with_a4(a4),
            self.settings.piano.stretch_curve(),
        ));
        self.enter(Screen::Tuning);
    }

    fn start_measure(&mut self) {
        let mut profile = PianoProfile::new();
        profile.set_a4_reference(self.settings.piano.a4);
        self.measure = Some(Measure {
            profile,
            index: 0,
            done: HashMap::new(),
            captured_at: None,
            result: None,
        });
        self.enter(Screen::Measure);
    }

    fn enter(&mut self, screen: Screen) {
        self.reset_note_state();
        self.ensure_mic();
        self.screen = screen;
    }

    fn go_home(&mut self) {
        self.save_session();
        self.resumable = Session::load_recent().ok().flatten();
        self.screen = Screen::Home;
    }

    fn save_session(&self) {
        if let Some(flow) = &self.flow {
            let _ = flow.session().save();
        }
    }

    fn reset_note_state(&mut self) {
        self.step = 0;
        self.pitch_cents = None;
        self.new_step();
    }

    fn new_step(&mut self) {
        self.reading = None;
        self.lock_since = None;
        self.beat = None;
        self.step_started = Instant::now();
    }

    /// Avance automatico: progreso 0..1 (1 = avanzar), o None si no aplica.
    fn auto_progress(&mut self, kind: StepKind) -> Option<f32> {
        let ok = match kind {
            StepKind::Pitch if self.settings.auto_advance => matches!(
                self.shown_reading(),
                Some((r, true)) if r.cents.abs() <= self.settings.tolerance
            ),
            StepKind::Unison if self.settings.auto_unison => {
                matches!(self.beat, Some(b) if b.clean && b.analyzed_secs >= 1.5)
            }
            _ => {
                self.lock_since = None;
                return None;
            }
        };
        if !ok {
            self.lock_since = None;
            return Some(0.0);
        }
        let since = *self.lock_since.get_or_insert_with(Instant::now);
        Some((since.elapsed().as_secs_f32() / self.settings.auto_secs.max(0.2)).min(1.0))
    }

    /// Medidor principal + lupa + estroboscopio segun los ajustes.
    fn meters(&self, ui: &mut egui::Ui, shown: Option<(f32, bool)>, labels: (&str, &str)) {
        let tol = self.settings.tolerance;
        let cents = shown.map(|s| s.0);
        let fresh = shown.map(|s| s.1).unwrap_or(false);
        widgets::cents_meter(ui, cents, tol, fresh, labels);
        if self.settings.fine_meter {
            widgets::fine_meter(ui, cents, tol, fresh);
        }
        if self.settings.strobe {
            let in_tune = cents.is_some_and(|c| c.abs() <= tol);
            widgets::strobe(ui, self.strobe_phase, fresh, in_tune);
        }
    }

    /// Mueve el estroboscopio segun la lectura en vivo.
    fn advance_strobe(&mut self) {
        let dt = self.last_frame.elapsed().as_secs_f32().min(0.1);
        self.last_frame = Instant::now();
        if let Some((r, true)) = self.shown_reading() {
            // 1 cent = 6 px/s (hasta +-50 cents)
            let speed = r.cents.clamp(-50.0, 50.0) * 6.0;
            self.strobe_phase = (self.strobe_phase + speed * dt) % 10_000.0;
        }
    }

    // ------------------------------------------------------------ Barra comun

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if self.screen != Screen::Home && ui.button(self.t("← Inicio", "← Home")).clicked()
            {
                self.go_home();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.selectable_value(&mut self.settings.lang, Lang::En, "English");
                ui.selectable_value(&mut self.settings.lang, Lang::Es, "Español");
                ui.add_space(16.0);
                let before = self.settings.level;
                let mut level = before;
                for (l, es, en) in [
                    (Level::Custom, "Personalizado", "Custom"),
                    (Level::Advanced, "Avanzado", "Advanced"),
                    (Level::Intermediate, "Intermedio", "Intermediate"),
                    (Level::Basic, "Básico", "Basic"),
                ] {
                    ui.selectable_value(&mut level, l, self.t(es, en));
                }
                ui.label(self.t("Nivel:", "Level:"));
                if level != before {
                    self.settings.apply_level(level);
                    self.reset_note_state();
                }
            });
        });
    }

    // ---------------------------------------------------------------- Inicio

    fn ui_home(&mut self, ui: &mut egui::Ui) {
        ui.heading(RichText::new("Piano Tuner Free").size(30.0).strong());
        ui.label(self.t(
            "Afinador de piano gratuito. Configurá tu piano y elegí cómo afinar.",
            "Free piano tuner. Set up your piano and choose how to tune.",
        ));
        ui.add_space(8.0);

        self.update_detect_a4();
        self.section_piano(ui);
        self.section_help(ui);
        self.section_mic(ui);

        if self.settings.piano.trichord_from < self.settings.piano.bichord_from {
            self.settings.piano.trichord_from = self.settings.piano.bichord_from;
        }
        self.settings.apply_string_layout();

        ui.add_space(14.0);
        let big = egui::vec2(290.0, 56.0);
        ui.horizontal_wrapped(|ui| {
            if let Some(s) = &self.resumable {
                let label = format!(
                    "{} ({} {}/88)",
                    self.t("Continuar afinación", "Resume tuning"),
                    self.t("nota", "note"),
                    s.current_note_index + 1,
                );
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new(label)
                                .size(19.0)
                                .strong()
                                .color(Color32::WHITE),
                        )
                        .fill(GREEN)
                        .min_size(big),
                    )
                    .clicked()
                {
                    self.resume_tuning();
                }
            }
            let start = if self.resumable.is_some() {
                self.t("Empezar de nuevo", "Start over")
            } else {
                self.t("Afinar el piano (88 notas)", "Tune the piano (88 notes)")
            };
            if ui
                .add(egui::Button::new(RichText::new(start).size(19.0).strong()).min_size(big))
                .clicked()
            {
                if self.resumable.is_some() {
                    self.confirm_restart = true;
                } else {
                    self.start_tuning();
                }
            }
            if ui
                .add(
                    egui::Button::new(
                        RichText::new(self.t("Afinador libre", "Free tuner")).size(19.0),
                    )
                    .min_size(big),
                )
                .clicked()
            {
                self.enter(Screen::Free);
            }
        });
    }

    /// Pregunta antes de borrar una afinacion a medio hacer.
    fn restart_dialog(&mut self, ctx: &egui::Context) {
        let (title, body, yes, no) = (
            self.t("¿Empezar de nuevo?", "Start over?"),
            self.t(
                "Se pierde el progreso de la afinación guardada.",
                "The saved tuning progress will be lost.",
            ),
            self.t("Sí, empezar de nuevo", "Yes, start over"),
            self.t("Cancelar", "Cancel"),
        );
        let mut start = false;
        let mut cancel = false;
        egui::Window::new(title)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(body);
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    start = ui
                        .add(
                            egui::Button::new(RichText::new(yes).color(Color32::WHITE))
                                .fill(widgets::RED),
                        )
                        .clicked();
                    cancel = ui.button(no).clicked();
                });
            });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            cancel = true;
        }
        if start {
            self.confirm_restart = false;
            self.start_tuning();
        } else if cancel {
            self.confirm_restart = false;
        }
    }

    fn section_piano(&mut self, ui: &mut egui::Ui) {
        let title = self.t("Tu piano", "Your piano");
        egui::CollapsingHeader::new(RichText::new(title).size(20.0).strong())
            .id_salt("piano")
            .default_open(true)
            .show(ui, |ui| {
                let s = &self.settings;
                let (l_name, l_load, l_save, l_new, l_del) = (
                    s.t("Nombre", "Name"),
                    s.t("Pianos guardados", "Saved pianos"),
                    s.t("Guardar", "Save"),
                    s.t("Nuevo", "New"),
                    s.t("Borrar", "Delete"),
                );
                ui.horizontal_wrapped(|ui| {
                    ui.label(l_name);
                    ui.add(
                        egui::TextEdit::singleline(&mut self.settings.piano.name)
                            .desired_width(180.0),
                    );
                    if ui.button(l_save).clicked() {
                        let p = self.settings.piano.clone();
                        match self
                            .settings
                            .saved_pianos
                            .iter_mut()
                            .find(|x| x.name == p.name)
                        {
                            Some(x) => *x = p,
                            None => self.settings.saved_pianos.push(p),
                        }
                    }
                    if ui.button(l_new).clicked() {
                        self.settings.piano = PianoConfig::default();
                    }
                    if !self.settings.saved_pianos.is_empty() {
                        let mut chosen = None;
                        egui::ComboBox::from_id_salt("load_piano")
                            .selected_text(l_load)
                            .show_ui(ui, |ui| {
                                for (i, p) in self.settings.saved_pianos.iter().enumerate() {
                                    if ui
                                        .selectable_label(
                                            false,
                                            format!("{}  ({:.1} Hz)", p.name, p.a4),
                                        )
                                        .clicked()
                                    {
                                        chosen = Some(i);
                                    }
                                }
                            });
                        if let Some(i) = chosen {
                            self.settings.piano = self.settings.saved_pianos[i].clone();
                        }
                        let name = self.settings.piano.name.clone();
                        if self.settings.saved_pianos.iter().any(|p| p.name == name)
                            && ui.button(l_del).clicked()
                        {
                            self.settings.saved_pianos.retain(|p| p.name != name);
                        }
                    }
                });
                ui.add_space(4.0);

                let s = &self.settings;
                let labels = (
                    s.t("La central (A4)", "Middle A (A4)"),
                    s.t(
                        "Pianos viejos: dejalo donde está el piano hoy (ej. 417).",
                        "Old pianos: keep it where the piano is today (e.g. 417).",
                    ),
                    s.t("Empiezan 2 cuerdas en", "2 strings start at"),
                    s.t("Empiezan 3 cuerdas en", "3 strings start at"),
                    s.t("Estiramiento", "Stretch"),
                    s.t("Sin estiramiento", "None"),
                    s.t("Estándar", "Standard"),
                    s.t("Medido en este piano", "Measured on this piano"),
                    s.t("Medir mi piano…", "Measure my piano…"),
                );
                let measured_ok = self.settings.piano.measured.is_some();
                let mut measure = false;
                let s = &self.settings;
                let detect_l = (
                    s.t("🎤 Detectar de mi piano", "🎤 Detect from my piano"),
                    s.t(
                        "Tocá el La central (A4) varias veces y se completa solo.",
                        "Play middle A (A4) a few times and it fills in by itself.",
                    ),
                    s.t("Tocá el La central…", "Play middle A…"),
                    s.t("Cancelar", "Cancel"),
                );
                let mut cancel_detect = false;
                let pitch_warning = pitch_raise_warning(s.piano.a4, s.lang);
                egui::Grid::new("piano_grid")
                    .num_columns(2)
                    .spacing([24.0, 10.0])
                    .show(ui, |ui| {
                        ui.label(RichText::new(labels.0).strong());
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::DragValue::new(&mut self.settings.piano.a4)
                                    .speed(0.1)
                                    .range(380.0..=480.0)
                                    .suffix(" Hz")
                                    .max_decimals(1),
                            );
                            for v in [440.0, 442.0, 435.0] {
                                if ui.button(format!("{v:.0}")).clicked() {
                                    self.settings.piano.a4 = v;
                                }
                            }
                            match &self.detect_a4 {
                                None => {
                                    if ui.button(detect_l.0).on_hover_text(detect_l.1).clicked() {
                                        self.detect_a4 = Some(DetectA4 {
                                            freqs: Vec::new(),
                                            last: None,
                                            started: Instant::now(),
                                        });
                                    }
                                }
                                Some(d) => {
                                    ui.spinner();
                                    ui.label(
                                        RichText::new(format!(
                                            "{} ({}/{DETECT_READINGS})",
                                            detect_l.2,
                                            d.freqs.len()
                                        ))
                                        .color(widgets::BLUE),
                                    );
                                    if ui.button(detect_l.3).clicked() {
                                        cancel_detect = true;
                                    }
                                }
                            }
                        });
                        ui.end_row();
                        ui.label("");
                        ui.vertical(|ui| {
                            ui.label(RichText::new(labels.1).weak());
                            if let Some(w) = &pitch_warning {
                                ui.label(RichText::new(w).color(ORANGE));
                            }
                        });
                        ui.end_row();

                        ui.label(RichText::new(labels.2).strong());
                        note_combo(ui, "bi", &mut self.settings.piano.bichord_from, 22, 107);
                        ui.end_row();

                        ui.label(RichText::new(labels.3).strong());
                        let min = self.settings.piano.bichord_from;
                        note_combo(ui, "tri", &mut self.settings.piano.trichord_from, min, 108);
                        ui.end_row();

                        ui.label(RichText::new(labels.4).strong());
                        ui.horizontal_wrapped(|ui| {
                            ui.selectable_value(
                                &mut self.settings.piano.stretch,
                                StretchSel::Off,
                                labels.5,
                            );
                            ui.selectable_value(
                                &mut self.settings.piano.stretch,
                                StretchSel::Standard,
                                labels.6,
                            );
                            ui.add_enabled_ui(measured_ok, |ui| {
                                ui.selectable_value(
                                    &mut self.settings.piano.stretch,
                                    StretchSel::Measured,
                                    labels.7,
                                );
                            });
                            measure = ui.button(labels.8).clicked();
                        });
                        ui.end_row();
                    });
                if measure {
                    self.start_measure();
                }
                if cancel_detect {
                    self.detect_a4 = None;
                }

                ui.add_space(6.0);
                widgets::keyboard(ui, None, &HashMap::new(), string_count);
                ui.horizontal(|ui| {
                    for n in 1..=3u8 {
                        ui.label(RichText::new("■").color(strings_color(n)));
                        ui.label(strings_label(n, self.settings.lang));
                        ui.add_space(10.0);
                    }
                });
            });
    }

    fn section_help(&mut self, ui: &mut egui::Ui) {
        let title = self.t("Ayudas", "Assists");
        let s = &self.settings;
        let l = (
            s.t(
                "Recordarme apagar las cuerdas antes de afinar",
                "Remind me to mute strings before tuning",
            ),
            s.t(
                "Mostrar explicaciones en cada paso",
                "Show explanations on each step",
            ),
            s.t(
                "Avanzar solo cuando la cuerda queda en verde",
                "Auto-advance when the string is in the green zone",
            ),
            s.t(
                "Avanzar solo en los unísonos cuando no hay batido",
                "Auto-advance unisons when there are no beats",
            ),
            s.t(
                "Mostrar el medidor de batido en los unísonos",
                "Show the beat meter on unisons",
            ),
            s.t("Margen de afinado", "In-tune margin"),
            s.t(
                "Lupa de ±10 cents para el ajuste fino",
                "±10 cent magnifier for fine adjustment",
            ),
            s.t(
                "Estroboscopio (las franjas se quedan quietas cuando está afinada)",
                "Strobe (stripes stand still when in tune)",
            ),
            s.t(
                "Tiempo estable para avanzar",
                "Time stable before advancing",
            ),
            s.t(
                "Mantener última lectura (agudos cortos)",
                "Hold last reading (short treble)",
            ),
            s.t(
                "Elegí un nivel arriba o tocá cada opción a tu gusto (queda en «Personalizado»).",
                "Pick a level at the top or adjust each option (becomes \"Custom\").",
            ),
        );
        egui::CollapsingHeader::new(RichText::new(title).size(20.0).strong())
            .id_salt("help")
            .default_open(true)
            .show(ui, |ui| {
                ui.label(RichText::new(l.10).weak());
                let before = self.settings.assists();
                ui.checkbox(&mut self.settings.remind_mute, l.0);
                ui.checkbox(&mut self.settings.show_details, l.1);
                ui.checkbox(&mut self.settings.auto_advance, l.2);
                ui.checkbox(&mut self.settings.auto_unison, l.3);
                ui.checkbox(&mut self.settings.show_beats, l.4);
                ui.checkbox(&mut self.settings.fine_meter, l.6);
                ui.checkbox(&mut self.settings.strobe, l.7);
                egui::Grid::new("help_grid")
                    .num_columns(2)
                    .spacing([24.0, 8.0])
                    .show(ui, |ui| {
                        ui.label(l.5);
                        ui.add(
                            egui::Slider::new(&mut self.settings.tolerance, 1.0..=10.0)
                                .suffix(" cents"),
                        );
                        ui.end_row();
                        ui.label(l.8);
                        ui.add(
                            egui::Slider::new(&mut self.settings.auto_secs, 0.3..=3.0).suffix(" s"),
                        );
                        ui.end_row();
                        ui.label(l.9);
                        ui.add(
                            egui::Slider::new(&mut self.settings.hold_secs, 0.0..=8.0).suffix(" s"),
                        );
                        ui.end_row();
                    });
                if before != self.settings.assists() {
                    self.settings.level = Level::Custom;
                }
            });
    }

    fn section_mic(&mut self, ui: &mut egui::Ui) {
        let title = self.t("Micrófono", "Microphone");
        let default_label = self.t("Predeterminado de Windows", "Windows default");
        let hint = self.t(
            "Tocá una tecla: la barra tiene que moverse. Desactivá mejoras y cancelación de ruido del micrófono.",
            "Play a key: the bar should move. Turn off mic enhancements and noise cancellation.",
        );
        egui::CollapsingHeader::new(RichText::new(title).size(20.0).strong())
            .id_salt("mic")
            .default_open(true)
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    let before = self.settings.mic_device.clone();
                    egui::ComboBox::from_id_salt("mic_dev")
                        .width(320.0)
                        .selected_text(
                            self.settings
                                .mic_device
                                .clone()
                                .unwrap_or(default_label.into()),
                        )
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.settings.mic_device, None, default_label);
                            for d in &self.devices {
                                ui.selectable_value(
                                    &mut self.settings.mic_device,
                                    Some(d.clone()),
                                    d,
                                );
                            }
                        });
                    if ui
                        .button("⟳")
                        .on_hover_text(self.t("Actualizar lista", "Refresh list"))
                        .clicked()
                    {
                        self.devices = MicCapture::input_device_names();
                    }
                    if before != self.settings.mic_device {
                        self.restart_mic();
                    }
                });
                level_bar(ui, self.worker.as_ref().map(|w| w.level_db()));
                ui.label(RichText::new(hint).weak());
            });
    }

    // ------------------------------------------------------------- Afinacion

    fn ui_tuning(&mut self, ui: &mut egui::Ui) {
        let Some(current) = self.flow.as_ref().and_then(|f| f.current_note()) else {
            self.ui_finished(ui);
            return;
        };
        let steps = steps_for(current.string_count, self.settings.remind_mute);
        self.step = self.step.min(steps.len() - 1);
        let step = &steps[self.step];
        let name = Note::from_midi(current.midi).map(|n| n.name).unwrap_or("?");
        let lang = self.settings.lang;
        let a4 = self
            .flow
            .as_ref()
            .map(|f| f.session().a4_reference)
            .unwrap_or(self.settings.piano.a4);

        ui.label(format!(
            "{} {} {} {}   ·   A4 = {:.1} Hz   ·   {}",
            self.t("Nota", "Note"),
            current.note_index + 1,
            self.t("de", "of"),
            current.total_notes,
            a4,
            self.settings.piano.name
        ));

        ui.horizontal(|ui| {
            ui.label(RichText::new(&current.display_name).size(72.0).strong());
            ui.add_space(14.0);
            ui.vertical(|ui| {
                ui.add_space(10.0);
                if lang == Lang::Es {
                    ui.label(RichText::new(solfege(name)).size(24.0));
                }
                ui.label(
                    RichText::new(strings_label(current.string_count, lang))
                        .color(strings_color(current.string_count))
                        .size(18.0),
                );
                ui.label(format!(
                    "{}: {:.2} Hz",
                    self.t("Objetivo", "Target"),
                    current.target_freq
                ));
            });
            ui.add_space(30.0);
            if ui
                .button(self.t("🔊 Escuchar la nota", "🔊 Play the note"))
                .clicked()
            {
                self.play_reference(current.target_freq);
            }
        });

        // Pasos
        ui.add_space(6.0);
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                for (i, s) in steps.iter().enumerate() {
                    let text = format!("{}. {}", i + 1, self.pick(s.title));
                    let rt = if i == self.step {
                        RichText::new(text).size(20.0).strong().color(widgets::BLUE)
                    } else if i < self.step {
                        RichText::new(format!("✔ {text}")).color(GREEN)
                    } else {
                        RichText::new(text).weak()
                    };
                    ui.label(rt);
                    if i + 1 < steps.len() {
                        ui.label(RichText::new("→").weak());
                    }
                }
            });
            if self.settings.show_details {
                ui.label(RichText::new(self.pick(step.detail)).size(16.0));
            }
        });

        ui.add_space(6.0);
        let labels = (
            self.t("◀ baja: tensar", "◀ flat: tighten"),
            self.t("alta: aflojar ▶", "sharp: loosen ▶"),
        );
        let shown = self.shown_reading().map(|(r, f)| (r.cents, r.freq, f));
        match step.kind {
            StepKind::Mute => {
                widgets::cents_meter(ui, None, self.settings.tolerance, true, labels);
            }
            _ => {
                self.meters(ui, shown.map(|s| (s.0, s.2)), labels);
                reading_line(ui, shown, &self.settings);
                if let Some((cents, freq, true)) = shown {
                    if let Some(h) = wrong_note_hint(freq, cents, a4, current.midi, lang) {
                        ui.vertical_centered(|ui| {
                            ui.label(RichText::new(h).size(17.0).color(ORANGE));
                        });
                    }
                }
            }
        }
        if step.kind == StepKind::Unison && self.settings.show_beats {
            self.beat_panel(ui);
        }

        let auto = self.auto_progress(step.kind);
        if let Some(p) = auto {
            ui.add(
                egui::ProgressBar::new(p)
                    .desired_height(8.0)
                    .text(self.t("avance automático", "auto-advance")),
            );
        }

        ui.add_space(8.0);
        let mut confirm = auto == Some(1.0);
        let mut back = false;
        let mut skip = false;
        ui.horizontal_wrapped(|ui| {
            let size = egui::vec2(170.0, 46.0);
            back = ui
                .add(egui::Button::new(self.t("◀ Anterior", "◀ Back")).min_size(size))
                .clicked();
            skip = ui
                .add(egui::Button::new(self.t("Saltar nota ▶", "Skip note ▶")).min_size(size))
                .clicked();
            confirm |= ui
                .add(
                    egui::Button::new(
                        RichText::new(self.t("Listo ✔  (Espacio)", "Done ✔  (Space)"))
                            .strong()
                            .color(Color32::WHITE),
                    )
                    .fill(GREEN)
                    .min_size(egui::vec2(220.0, 46.0)),
                )
                .clicked();
        });
        ui.input(|i| {
            confirm |= i.key_pressed(egui::Key::Space) || i.key_pressed(egui::Key::Enter);
            back |= i.key_pressed(egui::Key::ArrowLeft);
            skip |= i.key_pressed(egui::Key::ArrowRight);
        });

        ui.add_space(10.0);
        ui.label(
            RichText::new(self.t(
                "Tocá una tecla del dibujo para saltar a esa nota. Verde: afinada · Naranja: quedó fuera del margen.",
                "Click a key to jump to that note. Green: in tune · Orange: left outside the margin.",
            ))
            .weak(),
        );
        let done = self.done_map();
        let clicked = widgets::keyboard(ui, Some(current.midi), &done, string_count);

        // Acciones
        let last_cents = self.reading.as_ref().map(|r| r.cents);
        if confirm && step.kind == StepKind::Pitch {
            self.pitch_cents = last_cents;
        }
        let final_cents = self.pitch_cents.or(last_cents).unwrap_or(0.0);
        let n_steps = steps.len();
        let Some(flow) = self.flow.as_mut() else {
            return;
        };
        let mut changed_note = false;
        let mut changed_step = false;
        if confirm {
            if self.step + 1 < n_steps {
                self.step += 1;
                changed_step = true;
            } else {
                flow.confirm_current_note(final_cents);
                changed_note = true;
            }
        } else if back {
            if self.step > 0 {
                self.step -= 1;
                changed_step = true;
            } else if flow.go_back_note() {
                changed_note = true;
            }
        } else if skip {
            flow.skip_current_note();
            changed_note = true;
        } else if let Some(midi) = clicked {
            changed_note = flow.go_to_midi(midi);
        }
        if changed_note {
            self.reset_note_state();
            self.save_session();
        } else if changed_step {
            self.new_step();
        }
    }

    /// Medidor de batido para los unisonos.
    fn beat_panel(&self, ui: &mut egui::Ui) {
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let (text, color) = match self.beat {
                None => (
                    self.t(
                        "Batido: tocá la tecla y dejala sonar unos 2 segundos…",
                        "Beats: play the key and let it ring for about 2 seconds…",
                    )
                    .to_string(),
                    ui.visuals().weak_text_color(),
                ),
                Some(b) if b.clean => (
                    self.t(
                        "Sin batido ✔  El unísono está limpio.",
                        "No beats ✔  The unison is clean.",
                    )
                    .to_string(),
                    GREEN,
                ),
                Some(BeatInfo {
                    rate_hz: Some(r), ..
                }) => (
                    format!(
                        "{}: {r:.1} {}",
                        self.t("Batido", "Beats"),
                        self.t(
                            "por segundo. Girá de a poco: más lento = más cerca.",
                            "per second. Turn slowly: slower = closer."
                        )
                    ),
                    ORANGE,
                ),
                Some(_) => (
                    self.t(
                        "Batido lento o irregular. Ajustá apenas y volvé a tocar.",
                        "Slow or irregular beats. Adjust slightly and play again.",
                    )
                    .to_string(),
                    ORANGE,
                ),
            };
            ui.label(RichText::new(text).size(20.0).strong().color(color));
            let pts: Vec<(f32, f32)> = self
                .envelope_db
                .iter()
                .map(|(t, db)| (t + 3.0, *db))
                .collect();
            widgets::envelope_plot(ui, &pts, color);
        });
    }

    /// Notas ya afinadas: `true` si quedaron dentro del margen.
    fn done_map(&self) -> HashMap<u8, bool> {
        let tol = self.settings.tolerance;
        self.flow
            .as_ref()
            .map(|f| {
                f.session()
                    .completed_notes
                    .iter()
                    .map(|n| (n.midi, n.final_cents.abs() <= tol))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn ui_finished(&mut self, ui: &mut egui::Ui) {
        let (count, avg) = self
            .flow
            .as_ref()
            .map(|f| {
                (
                    f.session().completed_notes.len(),
                    f.session().average_deviation(),
                )
            })
            .unwrap_or((0, 0.0));
        ui.add_space(30.0);
        ui.heading(
            RichText::new(self.t("¡Terminaste!", "All done!"))
                .size(36.0)
                .strong(),
        );
        ui.label(format!(
            "{}: {count}   ·   {}: {avg:.1} cents",
            self.t("Notas afinadas", "Notes tuned"),
            self.t("desvío promedio", "average deviation")
        ));
        ui.add_space(10.0);
        let done = self.done_map();
        widgets::keyboard(ui, None, &done, string_count);
        ui.add_space(10.0);
        if ui
            .button(self.t("Volver al inicio", "Back to home"))
            .clicked()
        {
            self.go_home();
        }
    }

    // ---------------------------------------------------------- Medir piano

    fn ui_measure(&mut self, ui: &mut egui::Ui) {
        let sample_rate = self.mic.as_ref().map(|m| m.sample_rate()).unwrap_or(48_000);
        let a4 = self.settings.piano.a4;
        ui.heading(
            RichText::new(self.t("Medir mi piano", "Measure my piano"))
                .size(28.0)
                .strong(),
        );
        ui.label(self.t(
            "Tocá cada tecla que se marca, fuerte y dejándola sonar. Con eso se calcula cuánto \
             estirar los agudos y los graves para TU piano. Alcanza con que una cuerda suene \
             (no hace falta apagar las otras).",
            "Play each highlighted key firmly and let it ring. This computes how much to stretch \
             treble and bass for YOUR piano.",
        ));
        ui.add_space(8.0);

        let Some(m) = self.measure.as_mut() else {
            return;
        };

        // Captura de armonicos de la tecla actual
        if m.result.is_none() {
            if let (Some(&midi), Some(r)) = (MEASURE_KEYS.get(m.index), self.reading.as_ref()) {
                let fresh = r.at.elapsed() < Duration::from_millis(200);
                if m.captured_at.is_none() && fresh && r.confidence > 0.75 {
                    let target = Temperament::with_a4(a4).frequency(midi);
                    let win = PitchDetector::window_for_target(target);
                    let want = (sample_rate as f32 * win.as_secs_f32()) as usize;
                    let trimmed = if r.samples.len() > want {
                        &r.samples[r.samples.len() - want..]
                    } else {
                        &r.samples[..]
                    };
                    let partials = PartialAnalyzer::new(sample_rate).analyze(trimmed, r.freq);
                    let needed = if midi >= 88 { 2 } else { 4 };
                    if partials.len() >= needed {
                        m.done.insert(midi, partials.len());
                        m.profile
                            .record_note_with_partials(midi, r.freq, r.cents, partials);
                        m.captured_at = Some(Instant::now());
                    }
                }
            }
            if m.captured_at
                .is_some_and(|t| t.elapsed() > Duration::from_millis(900))
            {
                m.captured_at = None;
                m.index += 1;
                self.reading = None;
            }
        }

        let m = self.measure.as_mut().unwrap();
        let measured = m.done.len();
        let current = MEASURE_KEYS.get(m.index).copied();
        let mut finish = current.is_none() && m.result.is_none();

        if let Some(midi) = current.filter(|_| m.result.is_none()) {
            let name = Note::from_midi(midi)
                .map(|n| n.display_name())
                .unwrap_or_default();
            ui.horizontal(|ui| {
                ui.label(RichText::new(&name).size(64.0).strong());
                ui.add_space(12.0);
                ui.vertical(|ui| {
                    ui.add_space(10.0);
                    ui.label(format!(
                        "{} {} / {}",
                        self.settings.t("Tecla", "Key"),
                        m.index + 1,
                        MEASURE_KEYS.len()
                    ));
                    if m.captured_at.is_some() {
                        ui.label(
                            RichText::new(self.settings.t("✔ Medida", "✔ Measured"))
                                .color(GREEN)
                                .size(20.0),
                        );
                    } else {
                        ui.label(
                            RichText::new(self.settings.t("Tocá esta tecla…", "Play this key…"))
                                .size(20.0),
                        );
                    }
                });
            });
        }
        ui.add_space(6.0);
        level_bar(ui, self.worker.as_ref().map(|w| w.level_db()));
        ui.add_space(6.0);

        let m = self.measure.as_mut().unwrap();
        let mut highlight = HashMap::new();
        for (k, _) in &m.done {
            highlight.insert(*k, true);
        }
        for k in MEASURE_KEYS.iter().skip(m.index) {
            highlight.entry(*k).or_insert(false);
        }
        widgets::keyboard(
            ui,
            current.filter(|_| m.result.is_none()),
            &highlight,
            string_count,
        );

        ui.add_space(10.0);
        let (l_skip, l_finish, l_retry, l_use) = (
            self.settings.t("Saltar tecla ▶", "Skip key ▶"),
            self.settings.t("Terminar y calcular", "Finish and compute"),
            self.settings.t("Medir de nuevo", "Measure again"),
            self.settings.t("Volver al inicio", "Back to home"),
        );
        let mut restart = false;
        let mut home = false;
        let m = self.measure.as_mut().unwrap();
        match &m.result {
            None => {
                ui.horizontal(|ui| {
                    if ui
                        .add(egui::Button::new(l_skip).min_size(egui::vec2(170.0, 46.0)))
                        .clicked()
                    {
                        m.index += 1;
                        m.captured_at = None;
                    }
                    if ui
                        .add_enabled(
                            measured >= 3,
                            egui::Button::new(l_finish).min_size(egui::vec2(220.0, 46.0)),
                        )
                        .clicked()
                    {
                        finish = true;
                    }
                });
            }
            Some(Ok(())) => {
                ui.label(
                    RichText::new(self.settings.t(
                        "✔ Listo. Tu piano quedó medido y el estiramiento «Medido en este piano» está activado.",
                        "✔ Done. Your piano is measured and \"Measured on this piano\" stretch is on.",
                    ))
                    .size(20.0)
                    .color(GREEN),
                );
                ui.horizontal(|ui| {
                    home = ui.button(l_use).clicked();
                    restart = ui.button(l_retry).clicked();
                });
            }
            Some(Err(e)) => {
                ui.label(RichText::new(e).color(widgets::RED));
                ui.horizontal(|ui| {
                    restart = ui.button(l_retry).clicked();
                    home = ui.button(l_use).clicked();
                });
            }
        }

        if finish {
            let result = match stretch_from_profile(&m.profile) {
                Ok(offsets) => {
                    self.settings.piano.measured = Some(offsets.to_vec());
                    self.settings.piano.stretch = StretchSel::Measured;
                    Ok(())
                }
                Err(_) => Err(self
                    .settings
                    .t(
                        "No alcanzaron las mediciones (hacen falta al menos 3 teclas con buenos armónicos). Probá tocar más fuerte o acercar la compu.",
                        "Not enough measurements (at least 3 keys with clear overtones). Try playing louder or moving the laptop closer.",
                    )
                    .to_string()),
            };
            if let Some(m) = self.measure.as_mut() {
                m.result = Some(result);
            }
        }
        if restart {
            self.start_measure();
        }
        if home {
            self.go_home();
        }
    }

    // --------------------------------------------------------- Afinador libre

    fn ui_free(&mut self, ui: &mut egui::Ui) {
        ui.label(format!(
            "{}   ·   A4 = {:.1} Hz",
            self.t("Afinador libre", "Free tuner"),
            self.settings.piano.a4
        ));
        ui.add_space(16.0);
        let shown = self
            .shown_reading()
            .map(|(r, f)| (r.cents, r.freq, f, r.midi));
        let note = shown.and_then(|s| s.3).and_then(Note::from_midi);
        let lang = self.settings.lang;
        let play = self.t("Tocá una nota", "Play a note");
        ui.vertical_centered(|ui| match note {
            Some(n) => {
                ui.label(RichText::new(n.display_name()).size(96.0).strong());
                if lang == Lang::Es {
                    ui.label(RichText::new(solfege(n.name)).size(28.0));
                }
            }
            None => {
                ui.label(RichText::new("—").size(96.0).weak());
                ui.label(RichText::new(play).size(28.0).weak());
            }
        });
        ui.add_space(10.0);
        let labels = (self.t("◀ baja", "◀ flat"), self.t("alta ▶", "sharp ▶"));
        self.meters(ui, shown.map(|s| (s.0, s.2)), labels);
        reading_line(ui, shown.map(|s| (s.0, s.1, s.2)), &self.settings);
        ui.add_space(14.0);
        widgets::keyboard(ui, note.map(|n| n.midi), &HashMap::new(), string_count);
    }
}

/// Aviso cuando lo que suena no es la nota pedida: otra tecla, un armonico
/// o una cuerda muy lejos (mas de medio tono).
fn wrong_note_hint(freq: f32, cents: f32, a4: f32, midi: u8, lang: Lang) -> Option<String> {
    if cents.abs() < 50.0 {
        return None;
    }
    let (heard, _) = Temperament::with_a4(a4).nearest_note(freq)?;
    let diff = heard as i32 - midi as i32;
    let name = Note::from_midi(heard)
        .map(|n| n.display_name())
        .unwrap_or_default();
    let es = lang == Lang::Es;
    Some(if diff != 0 && diff % 12 == 0 {
        if es {
            format!("Escucho {name}, una octava distinta. Puede ser un armónico: tocá de nuevo más suave.")
        } else {
            format!(
                "I hear {name}, a different octave. It may be an overtone: play again more gently."
            )
        }
    } else if diff.abs() <= 2 {
        let semis = (cents / 100.0).abs();
        match (es, cents < 0.0) {
            (true, true) => {
                format!("Muy baja: casi {semis:.1} semitonos. Tensá despacio y en varias vueltas.")
            }
            (true, false) => {
                format!("Muy alta: casi {semis:.1} semitonos. Aflojá un poco y volvé a subir.")
            }
            (false, true) => {
                format!("Very flat: about {semis:.1} semitones. Tighten slowly, in several turns.")
            }
            (false, false) => {
                format!("Very sharp: about {semis:.1} semitones. Loosen a bit, then come back up.")
            }
        }
    } else if es {
        format!("Suena como {name}. ¿Es la tecla correcta?")
    } else {
        format!("Sounds like {name}. Is it the right key?")
    })
}

/// Aviso si el piano esta lejos de 440: subirlo mucho de golpe es riesgoso.
fn pitch_raise_warning(a4: f32, lang: Lang) -> Option<String> {
    let cents = 1200.0 * (a4 / 440.0).log2();
    if cents > -20.0 {
        return None;
    }
    Some(match lang {
        Lang::Es => format!(
            "Tu piano está {:.0} cents abajo de 440. Afinarlo donde está es lo más seguro.              Para subirlo a 440 conviene ir de a poco (ej. dos pasadas, primero a la mitad):              subir mucho de golpe puede romper cuerdas viejas.",
            -cents
        ),
        Lang::En => format!(
            "Your piano is {:.0} cents below 440. Tuning it where it is is safest.              To bring it to 440, go in steps (e.g. two passes, halfway first):              a big raise at once can break old strings.",
            -cents
        ),
    })
}

fn string_count(midi: u8) -> u8 {
    Note::from_midi(midi).map(|n| n.string_count()).unwrap_or(1)
}

fn strings_label(n: u8, lang: Lang) -> String {
    match (n, lang) {
        (1, Lang::Es) => "1 cuerda".into(),
        (1, Lang::En) => "1 string".into(),
        (n, Lang::Es) => format!("{n} cuerdas"),
        (n, Lang::En) => format!("{n} strings"),
    }
}

/// Barra de volumen de entrada.
fn level_bar(ui: &mut egui::Ui, db: Option<f32>) {
    let db = db.unwrap_or(-120.0);
    let norm = ((db + 70.0) / 70.0).clamp(0.0, 1.0);
    ui.add(
        egui::ProgressBar::new(norm)
            .desired_width(ui.available_width().min(500.0))
            .text(format!("{db:.0} dB")),
    );
}

/// Linea con cents y Hz. `shown = (cents, Hz, lectura en vivo)`.
fn reading_line(ui: &mut egui::Ui, shown: Option<(f32, f32, bool)>, s: &Settings) {
    ui.vertical_centered(|ui| match shown {
        Some((cents, freq, fresh)) => {
            let ok = cents.abs() <= s.tolerance;
            let text = format!(
                "{:+.1} cents   ·   {:.2} Hz{}{}",
                cents,
                freq,
                if ok {
                    s.t("   ·   ¡Afinada!", "   ·   In tune!")
                } else {
                    ""
                },
                if fresh {
                    ""
                } else {
                    s.t("   (última lectura)", "   (last reading)")
                }
            );
            let mut rt = RichText::new(text).size(22.0).strong();
            if ok {
                rt = rt.color(GREEN);
            }
            ui.label(rt);
        }
        None => {
            ui.label(
                RichText::new(s.t("Esperando sonido…", "Listening…"))
                    .size(22.0)
                    .weak(),
            );
        }
    });
}

fn note_combo(ui: &mut egui::Ui, id: &str, value: &mut u8, min: u8, max: u8) {
    let label = |m: u8| {
        Note::from_midi(m)
            .map(|n| format!("{}  ({})", n.display_name(), solfege(n.name)))
            .unwrap_or_default()
    };
    egui::ComboBox::from_id_salt(id)
        .selected_text(label(*value))
        .height(400.0)
        .show_ui(ui, |ui| {
            for m in min..=max {
                ui.selectable_value(value, m, label(m));
            }
        });
}

impl eframe::App for TunerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_audio();
        self.advance_strobe();
        ctx.request_repaint_after(Duration::from_millis(30));

        if let Some(err) = self.audio_error.clone() {
            let retry = self.t("Reintentar", "Retry");
            egui::TopBottomPanel::top("error").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.colored_label(widgets::RED, err);
                    if ui.button(retry).clicked() {
                        self.restart_mic();
                    }
                });
            });
        }
        egui::TopBottomPanel::top("bar").show(ctx, |ui| self.top_bar(ui));
        if self.confirm_restart {
            self.restart_dialog(ctx);
        }
        egui::TopBottomPanel::bottom("footer").show(ctx, |ui| {
            ui.label(
                RichText::new(self.t(
                    "Piano Tuner Free · open source (MIT) · motor basado en pianito de Axel Stevens",
                    "Piano Tuner Free · open source (MIT) · engine based on pianito by Axel Stevens",
                ))
                .weak()
                .small(),
            );
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| match self.screen {
                Screen::Home => self.ui_home(ui),
                Screen::Tuning => self.ui_tuning(ui),
                Screen::Free => self.ui_free(ui),
                Screen::Measure => self.ui_measure(ui),
            });
        });

        if self.settings != self.saved_settings {
            self.settings.save();
            self.saved_settings = self.settings.clone();
        }
    }
}

impl Drop for TunerApp {
    fn drop(&mut self) {
        self.save_session();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hints_wrong_key_and_octave() {
        let t = Temperament::with_a4(440.0);
        // en afinacion: sin aviso
        assert!(wrong_note_hint(440.0, 0.0, 440.0, 69, Lang::En).is_none());
        // octava arriba
        let f = t.frequency(81);
        let c = t.cents_from_target(f, 440.0);
        assert!(wrong_note_hint(f, c, 440.0, 69, Lang::En)
            .unwrap()
            .contains("octave"));
        // otra tecla (una quinta)
        let f = t.frequency(76);
        let c = t.cents_from_target(f, 440.0);
        assert!(wrong_note_hint(f, c, 440.0, 69, Lang::En)
            .unwrap()
            .contains("E5"));
        // muy baja
        let f = t.frequency(68);
        let c = t.cents_from_target(f, 440.0);
        assert!(wrong_note_hint(f, c, 440.0, 69, Lang::En)
            .unwrap()
            .contains("flat"));
    }

    #[test]
    fn warns_on_big_pitch_raise() {
        assert!(pitch_raise_warning(417.0, Lang::Es).is_some());
        assert!(pitch_raise_warning(440.0, Lang::Es).is_none());
        assert!(pitch_raise_warning(436.0, Lang::Es).is_none());
    }
}
