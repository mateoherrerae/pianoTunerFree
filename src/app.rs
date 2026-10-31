//! Ventana principal: ajustes, afinacion guiada y afinador libre.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, RichText};

use piano_tuner_free::audio::{AudioOutput, MicCapture, PitchUpdate, PitchWorker};
use piano_tuner_free::settings::{Lang, Mode, Settings};
use piano_tuner_free::tuning::{
    Note, Session, StretchCurve, Temperament, TuningFlow, TuningMode, TuningOrder,
};

use crate::widgets::{self, solfege, strings_color, GREEN};

#[derive(PartialEq)]
enum Screen {
    Home,
    Tuning,
    Free,
}

/// Ultima lectura del microfono.
struct Reading {
    freq: f32,
    cents: f32,
    /// Nota detectada (solo en el afinador libre).
    midi: Option<u8>,
    at: Instant,
}

#[derive(Clone, Copy, PartialEq)]
enum StepKind {
    /// Apagar cuerdas: no se mide nada.
    Mute,
    /// Afinar una cuerda sola contra el objetivo (las demas apagadas).
    Pitch,
    /// Igualar una cuerda con las ya afinadas (unisono, de oido).
    Unison,
}

/// Un paso de afinacion de una nota.
struct Step {
    kind: StepKind,
    title: (&'static str, &'static str),
    detail: (&'static str, &'static str),
}

const fn step(
    kind: StepKind,
    title: (&'static str, &'static str),
    detail: (&'static str, &'static str),
) -> Step {
    Step {
        kind,
        title,
        detail,
    }
}

use StepKind::{Mute, Pitch, Unison};

const ONE: &[Step] = &[step(
    Pitch,
    ("Afiná la cuerda", "Tune the string"),
    (
        "Tocá la tecla y girá la clavija hasta que la barra quede en la zona verde.",
        "Play the key and turn the pin until the bar sits in the green zone.",
    ),
)];

const TWO_APPRENTICE: &[Step] = &[
    step(
        Mute,
        ("Apagá la cuerda derecha", "Mute the right string"),
        (
            "Poné una cuña de goma o fieltro para que suene solo la cuerda izquierda. Cuando esté, apretá «Listo».",
            "Insert a rubber or felt wedge so only the left string sounds. Then press \"Done\".",
        ),
    ),
    step(
        Pitch,
        ("Afiná la cuerda izquierda", "Tune the left string"),
        (
            "Tocá la tecla y llevá la barra a la zona verde.",
            "Play the key and bring the bar into the green zone.",
        ),
    ),
    step(
        Unison,
        ("Afiná la cuerda derecha", "Tune the right string"),
        (
            "Sacá la cuña. Afiná la derecha hasta que las dos suenen juntas sin batido (el \"uaua\").",
            "Remove the wedge. Tune the right string until both sound together with no beats.",
        ),
    ),
];

const THREE_APPRENTICE: &[Step] = &[
    step(
        Mute,
        ("Apagá las cuerdas de los costados", "Mute the outer strings"),
        (
            "Poné fieltro o cuñas para que suene solo la cuerda del medio. Cuando esté, apretá «Listo».",
            "Use a felt strip or wedges so only the center string sounds. Then press \"Done\".",
        ),
    ),
    step(
        Pitch,
        ("Afiná la cuerda del medio", "Tune the center string"),
        (
            "Tocá la tecla y llevá la barra a la zona verde.",
            "Play the key and bring the bar into the green zone.",
        ),
    ),
    step(
        Unison,
        ("Afiná la cuerda izquierda", "Tune the left string"),
        (
            "Liberá la izquierda y afinala contra la del medio hasta que no haya batido.",
            "Unmute the left string and tune it to the center one until there are no beats.",
        ),
    ),
    step(
        Unison,
        ("Afiná la cuerda derecha", "Tune the right string"),
        (
            "Liberá la derecha y afinala contra las otras dos hasta que no haya batido.",
            "Unmute the right string and tune it to the other two until there are no beats.",
        ),
    ),
];

const TWO_ADVANCED: &[Step] = &[
    step(
        Pitch,
        ("Izquierda (derecha apagada)", "Left (right muted)"),
        ("", ""),
    ),
    step(Unison, ("Derecha: unísono", "Right: unison"), ("", "")),
];

const THREE_ADVANCED: &[Step] = &[
    step(
        Pitch,
        ("Medio (costados apagados)", "Center (outer muted)"),
        ("", ""),
    ),
    step(Unison, ("Izquierda: unísono", "Left: unison"), ("", "")),
    step(Unison, ("Derecha: unísono", "Right: unison"), ("", "")),
];

fn steps_for(strings: u8, mode: Mode) -> &'static [Step] {
    match (strings, mode) {
        (1, _) => ONE,
        (2, Mode::Apprentice) => TWO_APPRENTICE,
        (2, Mode::Advanced) => TWO_ADVANCED,
        (_, Mode::Apprentice) => THREE_APPRENTICE,
        (_, Mode::Advanced) => THREE_ADVANCED,
    }
}

pub struct TunerApp {
    settings: Settings,
    saved_settings: Settings,
    screen: Screen,
    mic: Option<MicCapture>,
    worker: Option<PitchWorker>,
    output: Option<AudioOutput>,
    audio_error: Option<String>,
    flow: Option<TuningFlow>,
    step: usize,
    reading: Option<Reading>,
    live: bool,
    /// Desde cuando la nota esta estable en la zona verde (avance automatico).
    lock_since: Option<Instant>,
    /// Sesion sin terminar que se puede retomar.
    resumable: Option<Session>,
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
        Self {
            saved_settings: settings.clone(),
            settings,
            screen: Screen::Home,
            mic: None,
            worker: None,
            output: None,
            audio_error: None,
            flow: None,
            step: 0,
            reading: None,
            live: false,
            lock_since: None,
            resumable: Session::load_recent().ok().flatten(),
        }
    }

    fn t(&self, es: &'static str, en: &'static str) -> &'static str {
        self.settings.t(es, en)
    }

    fn pick(&self, pair: (&'static str, &'static str)) -> &'static str {
        self.settings.t(pair.0, pair.1)
    }

    fn ensure_mic(&mut self) {
        if self.worker.is_some() {
            return;
        }
        match MicCapture::new() {
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

    fn stretch(&self) -> Option<StretchCurve> {
        self.settings.stretch.then(StretchCurve::railsback_default)
    }

    fn start_tuning(&mut self) {
        self.settings.apply_string_layout();
        if let Some(old) = self.resumable.take() {
            let _ = old.delete();
        }
        self.flow = Some(TuningFlow::new(
            TuningMode::Concert,
            TuningOrder::new(),
            Temperament::with_a4(self.settings.a4),
            self.stretch(),
        ));
        self.enter_tuning();
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
            self.stretch(),
        ));
        self.enter_tuning();
    }

    fn enter_tuning(&mut self) {
        self.reset_note_state();
        self.ensure_mic();
        self.screen = Screen::Tuning;
    }

    fn go_home(&mut self) {
        self.save_session();
        self.resumable = Session::load_recent().ok().flatten();
        self.screen = Screen::Home;
    }

    fn start_free(&mut self) {
        self.reading = None;
        self.ensure_mic();
        self.screen = Screen::Free;
    }

    fn save_session(&self) {
        if let Some(flow) = &self.flow {
            let _ = flow.session().save();
        }
    }

    fn reset_note_state(&mut self) {
        self.step = 0;
        self.reading = None;
        self.lock_since = None;
        if let Some(f) = &mut self.flow {
            f.rearm_beep_latch();
        }
    }

    /// Lee el microfono y actualiza la lectura actual.
    fn poll_audio(&mut self) {
        let Some(worker) = &self.worker else { return };
        let target = match (&self.screen, &self.flow) {
            (Screen::Tuning, Some(flow)) => flow.current_note().map(|n| n.target_freq),
            _ => None,
        };
        worker.set_target(target);
        if let Some(w) = worker.take_audio_warning() {
            self.audio_error = Some(format!(
                "{}: {w}",
                self.t("Problema con el micrófono", "Microphone problem")
            ));
        }
        match worker.latest_update() {
            Some(PitchUpdate::Detected {
                frequency,
                confidence,
                ..
            }) if confidence > 0.6 => {
                let (cents, midi) = match (target, &self.flow) {
                    (Some(t), Some(flow)) => (flow.cents_from_target(frequency, t), None),
                    _ => match Temperament::with_a4(self.settings.a4).nearest_note(frequency) {
                        Some((m, c)) => (c, Some(m)),
                        None => return,
                    },
                };
                self.reading = Some(Reading {
                    freq: frequency,
                    cents,
                    midi,
                    at: Instant::now(),
                });
                self.live = true;
            }
            Some(PitchUpdate::Silence) => {
                self.live = false;
                self.lock_since = None;
                if let Some(f) = &mut self.flow {
                    f.rearm_beep_latch();
                }
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

    /// Progreso (0..1) del avance automatico en el paso actual, si aplica.
    /// Devuelve `Some(1.0)` cuando hay que avanzar.
    fn auto_progress(&mut self, kind: StepKind) -> Option<f32> {
        if !self.settings.auto_advance || kind != StepKind::Pitch {
            self.lock_since = None;
            return None;
        }
        let in_zone = matches!(
            self.shown_reading(),
            Some((r, true)) if r.cents.abs() <= self.settings.tolerance
        );
        if !in_zone {
            self.lock_since = None;
            return Some(0.0);
        }
        let since = *self.lock_since.get_or_insert_with(Instant::now);
        Some((since.elapsed().as_secs_f32() / self.settings.auto_secs.max(0.2)).min(1.0))
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
                let advanced = self.t("Avanzado", "Advanced");
                let apprentice = self.t("Aprendiz", "Apprentice");
                let before = self.settings.mode;
                ui.selectable_value(&mut self.settings.mode, Mode::Advanced, advanced);
                ui.selectable_value(&mut self.settings.mode, Mode::Apprentice, apprentice);
                ui.label(self.t("Modo:", "Mode:"));
                if before != self.settings.mode {
                    self.step = 0;
                    self.lock_since = None;
                }
            });
        });
        ui.separator();
    }

    // ---------------------------------------------------------------- Inicio

    fn ui_home(&mut self, ui: &mut egui::Ui) {
        ui.heading(RichText::new("Piano Tuner Free").size(30.0).strong());
        ui.label(self.t(
            "Afinador de piano gratuito. Configurá tu piano y elegí cómo afinar.",
            "Free piano tuner. Set up your piano and choose how to tune.",
        ));
        ui.add_space(12.0);

        let t = |es, en| self.settings.t(es, en);
        let (
            l_a4,
            l_old,
            l_bi,
            l_bi_hint,
            l_tri,
            l_tol,
            l_hold,
            l_hold_hint,
            l_stretch,
            l_stretch_hint,
            l_auto,
            l_auto_hint,
        ) = (
            t("La central (A4)", "Middle A (A4)"),
            t(
                "Pianos viejos: dejalo donde está el piano hoy (ej. 417).",
                "Old pianos: keep it where the piano is today (e.g. 417).",
            ),
            t("Empiezan 2 cuerdas en", "2 strings start at"),
            t(
                "(antes de esta nota: 1 cuerda)",
                "(below this note: 1 string)",
            ),
            t("Empiezan 3 cuerdas en", "3 strings start at"),
            t("Margen de afinado", "In-tune margin"),
            t("Mantener última lectura", "Hold last reading"),
            t(
                "Útil para los agudos que se apagan rápido.",
                "Helps with short treble notes.",
            ),
            t("Estiramiento", "Stretch"),
            t(
                "Subir un poco los agudos y bajar los graves (como un afinador profesional)",
                "Slightly raise the treble and lower the bass (like a pro tuner)",
            ),
            t("Avance automático", "Auto-advance"),
            t(
                "Pasa solo de paso o de nota cuando la cuerda queda estable en verde.",
                "Moves on by itself when the string stays in the green zone.",
            ),
        );

        egui::Grid::new("settings")
            .num_columns(2)
            .spacing([24.0, 12.0])
            .show(ui, |ui| {
                ui.label(RichText::new(l_a4).strong());
                ui.horizontal(|ui| {
                    ui.add(
                        egui::DragValue::new(&mut self.settings.a4)
                            .speed(0.1)
                            .range(380.0..=480.0)
                            .suffix(" Hz")
                            .max_decimals(1),
                    );
                    for v in [440.0, 442.0, 435.0] {
                        if ui.button(format!("{v:.0}")).clicked() {
                            self.settings.a4 = v;
                        }
                    }
                    ui.label(RichText::new(l_old).weak());
                });
                ui.end_row();

                ui.label(RichText::new(l_bi).strong());
                ui.horizontal(|ui| {
                    note_combo(ui, "bi", &mut self.settings.bichord_from, 22, 107);
                    ui.label(RichText::new(l_bi_hint).weak());
                });
                ui.end_row();

                ui.label(RichText::new(l_tri).strong());
                let min = self.settings.bichord_from;
                note_combo(ui, "tri", &mut self.settings.trichord_from, min, 108);
                ui.end_row();

                ui.label(RichText::new(l_tol).strong());
                ui.add(
                    egui::Slider::new(&mut self.settings.tolerance, 1.0..=10.0).suffix(" cents"),
                );
                ui.end_row();

                ui.label(RichText::new(l_hold).strong());
                ui.horizontal(|ui| {
                    ui.add(egui::Slider::new(&mut self.settings.hold_secs, 0.0..=8.0).suffix(" s"));
                    ui.label(RichText::new(l_hold_hint).weak());
                });
                ui.end_row();

                ui.label(RichText::new(l_auto).strong());
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.settings.auto_advance, "");
                    ui.add_enabled(
                        self.settings.auto_advance,
                        egui::Slider::new(&mut self.settings.auto_secs, 0.3..=3.0).suffix(" s"),
                    );
                    ui.label(RichText::new(l_auto_hint).weak());
                });
                ui.end_row();

                ui.label(RichText::new(l_stretch).strong());
                ui.checkbox(&mut self.settings.stretch, l_stretch_hint);
                ui.end_row();
            });

        if self.settings.trichord_from < self.settings.bichord_from {
            self.settings.trichord_from = self.settings.bichord_from;
        }
        self.settings.apply_string_layout();

        ui.add_space(12.0);
        ui.label(RichText::new(self.t("Cuerdas de tu piano", "Your piano's strings")).strong());
        widgets::keyboard(ui, None, &HashMap::new(), string_count);
        ui.horizontal(|ui| {
            for n in 1..=3u8 {
                ui.label(RichText::new("■").color(strings_color(n)));
                ui.label(match (n, self.settings.lang) {
                    (1, Lang::Es) => "1 cuerda",
                    (1, Lang::En) => "1 string",
                    (_, Lang::Es) => {
                        if n == 2 {
                            "2 cuerdas"
                        } else {
                            "3 cuerdas"
                        }
                    }
                    (_, Lang::En) => {
                        if n == 2 {
                            "2 strings"
                        } else {
                            "3 strings"
                        }
                    }
                });
                ui.add_space(10.0);
            }
        });

        ui.add_space(18.0);
        let big = egui::vec2(300.0, 56.0);
        ui.horizontal(|ui| {
            if let Some(s) = &self.resumable {
                let label = format!(
                    "{} ({} {}/88, {:.1} Hz)",
                    self.t("Continuar afinación", "Resume tuning"),
                    self.t("nota", "note"),
                    s.current_note_index + 1,
                    s.a4_reference
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
                ui.add_space(12.0);
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
                self.start_tuning();
            }
            ui.add_space(12.0);
            if ui
                .add(
                    egui::Button::new(
                        RichText::new(self.t("Afinador libre", "Free tuner")).size(19.0),
                    )
                    .min_size(big),
                )
                .clicked()
            {
                self.start_free();
            }
        });
        ui.add_space(6.0);
        ui.label(
            RichText::new(self.t(
                "Modo aprendiz: te recuerda apagar las cuerdas antes de afinar. Modo avanzado: pasos directos, sin recordatorios.",
                "Apprentice mode reminds you to mute strings first. Advanced mode goes straight to tuning.",
            ))
            .weak(),
        );
    }

    // ------------------------------------------------------------- Afinacion

    fn ui_tuning(&mut self, ui: &mut egui::Ui) {
        let Some(current) = self.flow.as_ref().and_then(|f| f.current_note()) else {
            self.ui_finished(ui);
            return;
        };
        let steps = steps_for(current.string_count, self.settings.mode);
        self.step = self.step.min(steps.len() - 1);
        let step = &steps[self.step];
        let name = Note::from_midi(current.midi).map(|n| n.name).unwrap_or("?");
        let lang = self.settings.lang;

        ui.label(format!(
            "{} {} {} {}   ·   A4 = {:.1} Hz",
            self.t("Nota", "Note"),
            current.note_index + 1,
            self.t("de", "of"),
            current.total_notes,
            self.flow
                .as_ref()
                .map(|f| f.session().a4_reference)
                .unwrap_or(self.settings.a4)
        ));

        ui.horizontal(|ui| {
            ui.label(RichText::new(&current.display_name).size(72.0).strong());
            ui.add_space(14.0);
            ui.vertical(|ui| {
                ui.add_space(10.0);
                if lang == Lang::Es {
                    ui.label(RichText::new(solfege(name)).size(24.0));
                }
                let strings = match (current.string_count, lang) {
                    (1, Lang::Es) => "1 cuerda".to_string(),
                    (1, Lang::En) => "1 string".to_string(),
                    (n, Lang::Es) => format!("{n} cuerdas"),
                    (n, Lang::En) => format!("{n} strings"),
                };
                ui.label(
                    RichText::new(strings)
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

        // Paso actual + lista de pasos
        ui.add_space(6.0);
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
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
            let detail = self.pick(step.detail);
            if !detail.is_empty() {
                ui.label(RichText::new(detail).size(16.0));
            }
        });

        ui.add_space(6.0);
        let labels = (
            self.t("◀ baja: tensar", "◀ flat: tighten"),
            self.t("alta: aflojar ▶", "sharp: loosen ▶"),
        );
        let shown = self.shown_reading().map(|(r, f)| (r.cents, r.freq, f));
        if step.kind == StepKind::Mute {
            widgets::cents_meter(ui, None, self.settings.tolerance, true, labels);
        } else {
            widgets::cents_meter(
                ui,
                shown.map(|s| s.0),
                self.settings.tolerance,
                shown.map(|s| s.2).unwrap_or(false),
                labels,
            );
        }
        reading_line(ui, shown, &self.settings);

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
        ui.horizontal(|ui| {
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
        let Some(flow) = self.flow.as_mut() else {
            return;
        };
        let mut changed_note = false;
        if confirm {
            if self.step + 1 < steps.len() {
                self.step += 1;
                self.reading = None;
                self.lock_since = None;
                flow.rearm_beep_latch();
            } else {
                flow.confirm_current_note(last_cents.unwrap_or(0.0));
                changed_note = true;
            }
        } else if back {
            if self.step > 0 {
                self.step -= 1;
                self.reading = None;
                self.lock_since = None;
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
        }
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

    // --------------------------------------------------------- Afinador libre

    fn ui_free(&mut self, ui: &mut egui::Ui) {
        ui.label(format!(
            "{}   ·   A4 = {:.1} Hz",
            self.t("Afinador libre", "Free tuner"),
            self.settings.a4
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
        widgets::cents_meter(
            ui,
            shown.map(|s| s.0),
            self.settings.tolerance,
            shown.map(|s| s.2).unwrap_or(false),
            labels,
        );
        reading_line(ui, shown.map(|s| (s.0, s.1, s.2)), &self.settings);
        ui.add_space(14.0);
        widgets::keyboard(ui, note.map(|n| n.midi), &HashMap::new(), string_count);
    }
}

fn string_count(midi: u8) -> u8 {
    Note::from_midi(midi).map(|n| n.string_count()).unwrap_or(1)
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
        if self.screen != Screen::Home {
            self.poll_audio();
            ctx.request_repaint_after(Duration::from_millis(30));
        }
        if let Some(err) = self.audio_error.clone() {
            let retry = self.t("Reintentar", "Retry");
            egui::TopBottomPanel::top("error").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.colored_label(widgets::RED, err);
                    if ui.button(retry).clicked() {
                        self.worker = None;
                        self.mic = None;
                        self.audio_error = None;
                        if self.screen != Screen::Home {
                            self.ensure_mic();
                        }
                    }
                });
            });
        }
        egui::TopBottomPanel::top("bar").show(ctx, |ui| self.top_bar(ui));
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
