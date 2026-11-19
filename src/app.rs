//! Ventana principal: ajustes, afinacion guiada, medicion del piano y
//! afinador libre.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, RichText};

use piano_tuner_free::audio::{
    AudioOutput, AudioSource, MicCapture, PartialAnalyzer, PitchDetector, PitchUpdate, PitchWorker,
};
use piano_tuner_free::beats::{self, BeatInfo};
use piano_tuner_free::intervals;
use piano_tuner_free::settings::{Lang, Level, PianoConfig, Settings, StretchSel, StrikeMode};
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

/// Resultado de un golpe de tecla (captura por golpe).
#[derive(Debug, Clone, Copy)]
struct Strike {
    cents: f32,
    freq: f32,
    /// Dispersion dentro del golpe, en cents.
    iqr: f32,
    /// Lecturas juntadas.
    n: usize,
}

impl Strike {
    /// Suficientes lecturas para confiar en el numero.
    fn enough(&self) -> bool {
        self.n >= 8
    }

    /// Confiable: suficientes lecturas y sonido parejo.
    fn good(&self) -> bool {
        self.enough() && self.iqr <= 6.0
    }
}

/// Golpes que se recuerdan para el historial.
const STRIKE_HISTORY: usize = 5;

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

#[derive(Debug, Clone, Copy, PartialEq)]
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

/// Pasada del modo tira de fieltro.
#[derive(Clone, Copy, PartialEq)]
enum StripPass {
    /// Sin tira: cada nota completa (cuerda + unisonos).
    Off,
    /// Pasada 1: solo la cuerda libre de cada nota.
    Pitch,
    /// Pasada 2: solo los unisonos.
    Unisons,
}

/// Pasos en el modo tira de fieltro. `move_strip` agrega un recordatorio
/// para poner o correr la tira (al empezar cada etapa).
fn strip_steps(strings: u8, pass: StripPass, move_strip: bool) -> Vec<Step> {
    let s = |kind, title, detail| Step {
        kind,
        title,
        detail,
    };
    use StepKind::*;
    let mut v = Vec::new();
    match pass {
        StripPass::Pitch => {
            if move_strip {
                v.push(s(
                    Mute,
                    ("Poné o corré la tira", "Place or move the strip"),
                    (
                        "Poné la tira de fieltro en la zona que vas a afinar para que suene una sola cuerda por nota. Cuando esté, apretá «Listo».",
                        "Place the felt strip over the section you are about to tune so only one string per note sounds. Then press \"Done\".",
                    ),
                ));
            }
            v.push(s(
                Pitch,
                ("Afiná la cuerda libre", "Tune the free string"),
                (
                    "Con la tira puesta suena una sola cuerda: afinala. Los unísonos van en la segunda pasada.",
                    "With the strip in place only one string sounds: tune it. Unisons come in the second pass.",
                ),
            ));
        }
        StripPass::Unisons => match strings {
            1 => {}
            2 => v.push(s(
                Unison,
                ("Unísono", "Unison"),
                (
                    "Sacá la tira de esta nota y afiná la otra cuerda hasta que no haya batido.",
                    "Pull the strip from this note and tune the other string until there are no beats.",
                ),
            )),
            _ => {
                v.push(s(
                    Unison,
                    ("Primer unísono", "First unison"),
                    (
                        "Liberá una cuerda de los costados y afinala contra la del medio hasta que no haya batido.",
                        "Free one outer string and tune it to the center one until there are no beats.",
                    ),
                ));
                v.push(s(
                    Unison,
                    ("Segundo unísono", "Second unison"),
                    (
                        "Liberá la otra cuerda y afinala contra las dos hasta que no haya batido.",
                        "Free the other string and tune it to the other two until there are no beats.",
                    ),
                ));
            }
        },
        StripPass::Off => {}
    }
    v
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

/// Ventana para decidir si la lectura esta quieta.
const STABLE_WINDOW: Duration = Duration::from_millis(600);

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
    /// Preguntando si terminar la afinacion aunque falten notas.
    confirm_finish: bool,
    /// Afinaciones guardadas (se relee al volver al inicio).
    history: Vec<Session>,
    /// Preguntando con que La afinar (hoy o el de la ultima vez).
    choose_a4: bool,
    detect_a4: Option<DetectA4>,
    /// Resultado del ultimo exportado: (ok, ruta o error).
    export_msg: Option<(bool, String)>,
    /// No tomar lecturas hasta este momento (suena la referencia).
    ignore_until: Option<Instant>,
    /// Desplazamiento del estroboscopio (px) y ultimo cuadro.
    strobe_phase: f32,
    last_frame: Instant,
    /// Lecturas recientes (para saber si la aguja esta quieta).
    recent: std::collections::VecDeque<(Instant, f32)>,
    /// Ultimos golpes de la nota actual (el mas nuevo al final).
    strikes: std::collections::VecDeque<Strike>,
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
            confirm_finish: false,
            history: Session::list_all().unwrap_or_default(),
            choose_a4: false,
            detect_a4: None,
            export_msg: None,
            ignore_until: None,
            strobe_phase: 0.0,
            last_frame: Instant::now(),
            recent: Default::default(),
            strikes: Default::default(),
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
            out.clear();
            // El micro escucharia la referencia como "afinada": ignorarlo.
            self.ignore_until = Some(Instant::now() + Duration::from_millis(2800));
            let _ = if self.settings.ref_piano {
                out.play_piano(freq, 2.5)
            } else {
                out.play_sine(freq, 2.0)
            };
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

        let finished = worker.take_strikes();
        if let Some(t) = target {
            for s in finished {
                // Golpes de antes de esta nota o mientras sonaba la referencia.
                if s.at < self.step_started || self.ignore_until.is_some_and(|u| s.at < u) {
                    continue;
                }
                let cents = if s.n > 0 {
                    1200.0 * (s.median_hz / t).log2()
                } else {
                    0.0
                };
                self.strikes.push_back(Strike {
                    cents,
                    freq: s.median_hz,
                    iqr: s.iqr_cents,
                    n: s.n,
                });
                while self.strikes.len() > STRIKE_HISTORY {
                    self.strikes.pop_front();
                }
            }
        }

        let update = worker.latest_update();
        if self.ignore_until.is_some_and(|t| now < t) {
            self.beat = None;
            return;
        }
        match update {
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
                            Some((m, c)) => {
                                // Mismo objetivo que la afinacion guiada.
                                let offset = if self.settings.free_stretch {
                                    self.settings
                                        .piano
                                        .stretch_curve()
                                        .map(|s| s.offset_cents(m))
                                        .unwrap_or(0.0)
                                } else {
                                    0.0
                                };
                                (c - offset, Some(m))
                            }
                            None => return,
                        }
                    }
                };
                self.recent.push_back((now, cents));
                while self
                    .recent
                    .front()
                    .is_some_and(|(t, _)| now.duration_since(*t) > STABLE_WINDOW)
                {
                    self.recent.pop_front();
                }
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

    /// Cuanto se mueve la lectura en el ultimo momento (cents de punta a
    /// punta), o None si hay pocas lecturas.
    fn spread(&self) -> Option<f32> {
        let now = Instant::now();
        let v: Vec<f32> = self
            .recent
            .iter()
            .filter(|(t, _)| now.duration_since(*t) <= STABLE_WINDOW)
            .map(|r| r.1)
            .collect();
        if v.len() < 4 {
            return None;
        }
        let max = v.iter().copied().fold(f32::MIN, f32::max);
        let min = v.iter().copied().fold(f32::MAX, f32::min);
        Some(max - min)
    }

    /// Indicador "estable / moviendose" debajo de la lectura.
    fn stability_label(&self, ui: &mut egui::Ui) {
        let Some(spread) = self.spread() else { return };
        if !self.live {
            return;
        }
        let (text, color) = if spread <= 2.0 {
            (self.t("● Estable", "● Steady").to_string(), GREEN)
        } else {
            (
                format!(
                    "● {} (±{:.0} cents)",
                    self.t(
                        "Moviéndose: esperá antes de girar",
                        "Moving: wait before turning"
                    ),
                    spread / 2.0
                ),
                ORANGE,
            )
        };
        ui.vertical_centered(|ui| {
            ui.label(RichText::new(text).color(color));
        });
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

    fn strip_pass(&self) -> StripPass {
        match (self.settings.strip_mode, self.settings.unison_pass) {
            (false, _) => StripPass::Off,
            (true, false) => StripPass::Pitch,
            (true, true) => StripPass::Unisons,
        }
    }

    /// Termino la pasada 1 con tira: volver al principio para los unisonos.
    fn start_unison_pass(&mut self) {
        self.settings.unison_pass = true;
        let first = TuningOrder::new().note_at(0).map(|n| n.midi);
        if let (Some(f), Some(m)) = (self.flow.as_mut(), first) {
            f.go_to_midi(m);
        }
        self.reset_note_state();
        self.save_session();
    }

    fn start_tuning(&mut self) {
        self.settings.unison_pass = false;
        self.settings.apply_string_layout();
        // La afinacion a medio hacer no se borra: queda en el historial
        // como terminada y se puede reabrir.
        if let Some(mut old) = self.resumable.take() {
            old.current_note_index = old.current_note_index.max(88);
            let _ = old.save();
        }
        let mut flow = TuningFlow::new(
            TuningMode::Concert,
            TuningOrder::new(),
            Temperament::with_a4(self.settings.piano.a4),
            self.settings.piano.stretch_curve(),
        );
        flow.session_mut().piano_name = Some(self.settings.piano.name.clone());
        self.flow = Some(flow);
        self.save_session();
        self.enter(Screen::Tuning);
    }

    /// La ultima afinacion guardada de este piano (con notas hechas).
    fn last_tuning(&self) -> Option<&Session> {
        let name = &self.settings.piano.name;
        self.history
            .iter()
            .filter(|s| s.piano_name.as_ref() == Some(name) && !s.completed_notes.is_empty())
            .max_by_key(|s| s.updated_at)
    }

    /// Empezar una afinacion nueva; si este piano ya se afino con otro La,
    /// primero preguntar con cual.
    fn request_start(&mut self) {
        let differs = self
            .last_tuning()
            .is_some_and(|s| cents_between(self.settings.piano.a4, s.a4_reference).abs() >= 1.0);
        if differs {
            self.choose_a4 = true;
        } else {
            self.start_tuning();
        }
    }

    /// Elegir el La: el de la ultima vez, el de hoy o la mitad.
    fn a4_dialog(&mut self, ctx: &egui::Context) {
        let Some(last) = self.last_tuning().cloned() else {
            self.choose_a4 = false;
            return;
        };
        let now = self.settings.piano.a4;
        let prev = last.a4_reference;
        let diff = cents_between(now, prev);
        let es = self.settings.lang == Lang::Es;
        let body = if es {
            format!(
                "La última vez ({}) afinaste este piano con La = {prev:.1} Hz.\nHoy está en {now:.1} Hz: {:.0} cents más {}.",
                last.date_label(),
                diff.abs(),
                if diff < 0.0 { "bajo" } else { "alto" }
            )
        } else {
            format!(
                "Last time ({}) you tuned this piano to A = {prev:.1} Hz.\nToday it's at {now:.1} Hz: {:.0} cents {}.",
                last.date_label(),
                diff.abs(),
                if diff < 0.0 { "lower" } else { "higher" }
            )
        };
        let half = (now * prev).sqrt();
        let big_raise = diff < -20.0;
        let mut pick: Option<f32> = None;
        let mut cancel = false;
        egui::Window::new(self.t("¿Con qué La afinás?", "Which A do you tune to?"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(RichText::new(body).strong());
                ui.add_space(6.0);
                let up = format!(
                    "{} ({prev:.1} Hz)",
                    if diff < 0.0 {
                        self.t("⬆ Subirlo a la de la última vez", "⬆ Raise it to last time's")
                    } else {
                        self.t("⬇ Bajarlo a la de la última vez", "⬇ Lower it to last time's")
                    }
                );
                if ui
                    .add(egui::Button::new(RichText::new(up).color(Color32::WHITE)).fill(GREEN))
                    .clicked()
                {
                    pick = Some(prev);
                }
                if big_raise {
                    ui.label(RichText::new(self.t(
                        "Es mucho para subir de una: puede cortar cuerdas viejas. Mejor dos pasadas: primero a la mitad, después a la de la última vez.",
                        "That's a big raise at once: old strings can break. Better two passes: halfway first, then last time's pitch.",
                    )).color(ORANGE));
                    let mid = format!(
                        "{} ({half:.1} Hz)",
                        self.t("Primera pasada: a la mitad", "First pass: halfway")
                    );
                    if ui.button(mid).clicked() {
                        pick = Some(half);
                    }
                }
                let stay = format!(
                    "{} ({now:.1} Hz)",
                    self.t("Afinarlo donde está hoy", "Tune it where it is today")
                );
                if ui.button(stay).clicked() {
                    pick = Some(now);
                }
                ui.add_space(4.0);
                cancel = ui.button(self.t("Cancelar", "Cancel")).clicked();
            });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            cancel = true;
        }
        if let Some(a4) = pick {
            self.choose_a4 = false;
            self.settings.piano.a4 = (a4 * 10.0).round() / 10.0;
            self.start_tuning();
        } else if cancel {
            self.choose_a4 = false;
        }
    }

    /// Antes de seguir una afinacion, cargar el piano con el que se hizo
    /// (cuerdas, teclas) y su La, para no mezclar tonalidades.
    fn select_piano_of(&mut self, session: &Session) {
        if let Some(name) = &session.piano_name {
            if let Some(p) = self.settings.saved_pianos.iter().find(|p| &p.name == name) {
                self.settings.piano = p.clone();
            }
        }
        self.settings.piano.a4 = session.a4_reference;
    }

    /// Reabrir una afinacion guardada (terminada o no) y seguirla.
    fn reopen_session(&mut self, mut session: Session) {
        // La otra afinacion en curso sigue guardada tal cual.
        self.resumable = None;
        self.settings.unison_pass = false;
        session.updated_at = chrono::Utc::now();
        self.select_piano_of(&session);
        self.settings.apply_string_layout();
        let a4 = session.a4_reference;
        let mut flow = TuningFlow::resume(
            session,
            TuningOrder::new(),
            Temperament::with_a4(a4),
            self.settings.piano.stretch_curve(),
        );
        if flow.is_finished() {
            flow.reopen();
        }
        self.flow = Some(flow);
        self.save_session();
        self.enter(Screen::Tuning);
    }

    fn resume_tuning(&mut self) {
        let Some(session) = self.resumable.take() else {
            return;
        };
        self.select_piano_of(&session);
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
        self.history = Session::list_all().unwrap_or_default();
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
        self.recent.clear();
        self.strikes.clear();
        self.lock_since = None;
        self.beat = None;
        self.step_started = Instant::now();
    }

    /// La nota actual se afina con captura por golpe.
    fn strike_on(&self) -> bool {
        self.screen == Screen::Tuning
            && self
                .current_target()
                .is_some_and(|t| self.settings.strike_mode.applies(t))
    }

    /// El ultimo golpe confiable, si lo hay.
    fn last_good_strike(&self) -> Option<Strike> {
        self.strikes.back().copied().filter(|s| s.good())
    }

    /// Avance por golpes: 2 golpes seguidos buenos y dentro del margen.
    /// En los unisonos el golpe se compara con la cuerda ya afinada y
    /// ademas tiene que sonar parejo (sin batido).
    fn strike_progress(&self, kind: StepKind) -> Option<f32> {
        let tol = self.settings.tolerance;
        let reference = self.pitch_cents.unwrap_or(0.0);
        let ok = |s: &Strike| match kind {
            StepKind::Pitch => s.good() && s.cents.abs() <= tol,
            StepKind::Unison => s.good() && s.iqr <= 3.0 && (s.cents - reference).abs() <= tol,
            _ => false,
        };
        let run = self.strikes.iter().rev().take_while(|s| ok(s)).count();
        Some((run as f32 / 2.0).min(1.0))
    }

    /// Avance automatico: progreso 0..1 (1 = avanzar), o None si no aplica.
    fn auto_progress(&mut self, kind: StepKind) -> Option<f32> {
        let wants = match kind {
            StepKind::Pitch => self.settings.auto_advance,
            StepKind::Unison => self.settings.auto_unison,
            _ => false,
        };
        if wants && self.strike_on() {
            self.lock_since = None;
            return self.strike_progress(kind);
        }
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
        self.section_history(ui);

        if self.settings.piano.trichord_from < self.settings.piano.bichord_from {
            self.settings.piano.trichord_from = self.settings.piano.bichord_from;
        }
        self.settings.apply_string_layout();

        ui.add_space(14.0);
        let big = egui::vec2(290.0, 56.0);
        ui.horizontal_wrapped(|ui| {
            if let Some(s) = &self.resumable {
                let piano = s
                    .piano_name
                    .clone()
                    .map(|n| format!("{n} · "))
                    .unwrap_or_default();
                let label = format!(
                    "{}\n{piano}La {:.1} Hz · {}/88 {}",
                    self.t("Continuar afinación", "Resume tuning"),
                    s.a4_reference,
                    s.completed_notes.len(),
                    self.t("hechas", "done"),
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
                    self.request_start();
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
                "La afinación a medio hacer no se borra: queda en «Afinaciones guardadas» y la podés reabrir.",
                "The unfinished tuning isn't deleted: it stays in \"Saved tunings\" and you can reopen it.",
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
            self.request_start();
        } else if cancel {
            self.confirm_restart = false;
        }
    }

    /// Todas las afinaciones guardadas, con boton para abrirlas.
    fn section_history(&mut self, ui: &mut egui::Ui) {
        if self.history.is_empty() {
            return;
        }
        let title = format!(
            "{} ({})",
            self.t("Afinaciones guardadas", "Saved tunings"),
            self.history.len()
        );
        let (l_open, l_cont, l_done, l_going, l_avg) = (
            self.t("Abrir", "Open"),
            self.t("Seguir", "Continue"),
            self.t("terminada", "finished"),
            self.t("en curso", "in progress"),
            self.t("prom.", "avg."),
        );
        let mut open = None;
        egui::CollapsingHeader::new(RichText::new(title).size(18.0).strong())
            .id_salt("history")
            .show(ui, |ui| {
                ui.label(
                    RichText::new(self.t(
                        "Nada se borra: si terminaste sin querer, abrila y seguí donde estabas.",
                        "Nothing gets deleted: if you finished by mistake, open it and keep going.",
                    ))
                    .weak(),
                );
                egui::Grid::new("history_grid")
                    .striped(true)
                    .num_columns(6)
                    .show(ui, |ui| {
                        for (i, s) in self.history.iter().take(40).enumerate() {
                            ui.label(s.date_label());
                            ui.label(RichText::new(s.piano_name.as_deref().unwrap_or("—")).strong());
                            ui.label(format!("La {:.1} Hz", s.a4_reference));
                            ui.label(format!(
                                "{}/88 · {l_avg} {:.1} c",
                                s.completed_notes.len(),
                                s.average_deviation()
                            ));
                            ui.label(if s.is_complete() { l_done } else { l_going });
                            let b = if s.is_complete() { l_open } else { l_cont };
                            if ui.button(b).clicked() {
                                open = Some(i);
                            }
                            ui.end_row();
                        }
                    });
            });
        if let Some(i) = open {
            let s = self.history[i].clone();
            self.reopen_session(s);
        }
    }

    /// Pregunta antes de terminar: muestra las notas que faltan.
    fn finish_dialog(&mut self, ctx: &egui::Context) {
        let pending = self
            .flow
            .as_ref()
            .map(|f| f.pending_notes())
            .unwrap_or_default();
        let es = self.settings.lang == Lang::Es;
        let body = if pending.is_empty() {
            self.t("Están todas las notas.", "Every note is done.").to_string()
        } else {
            format!(
                "{} {}: {}",
                if es { "Faltan" } else { "Still missing" },
                pending.len(),
                notes_list(&pending, 12)
            )
        };
        let (title, hint, keep, finish) = (
            self.t("¿Terminar la afinación?", "Finish the tuning?"),
            self.t(
                "Queda guardada y la podés reabrir cuando quieras desde «Afinaciones guardadas».",
                "It stays saved and you can reopen it anytime from \"Saved tunings\".",
            ),
            self.t("Seguir afinando", "Keep tuning"),
            self.t("Terminar", "Finish"),
        );
        let mut yes = false;
        let mut no = false;
        egui::Window::new(title)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(RichText::new(body).strong());
                ui.label(hint);
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    no = ui
                        .add(
                            egui::Button::new(RichText::new(keep).color(Color32::WHITE))
                                .fill(GREEN),
                        )
                        .clicked();
                    yes = ui.button(finish).clicked();
                });
            });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            no = true;
        }
        if yes {
            if let Some(f) = self.flow.as_mut() {
                f.finish();
            }
            self.confirm_finish = false;
            self.reset_note_state();
            self.save_session();
        } else if no {
            self.confirm_finish = false;
        }
    }

    /// Reabrir la afinacion que se esta mostrando como terminada.
    fn reopen_current(&mut self) {
        self.settings.unison_pass = false;
        if let Some(f) = self.flow.as_mut() {
            f.reopen();
        }
        self.reset_note_state();
        self.save_session();
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
                let keys_l = (
                    s.t("Teclas", "Keys"),
                    s.t("88 (termina en Do)", "88 (ends on C)"),
                    s.t("85 (termina en La)", "85 (ends on A)"),
                    s.t(
                        "Muchos pianos viejos tienen 85: mirá si la última tecla blanca es un La.",
                        "Many old pianos have 85: check if the last white key is an A.",
                    ),
                );
                let pitch_warning = pitch_raise_warning(s.piano.a4, s.lang);
                let last_l = self.last_tuning().map(|l| {
                    let diff = cents_between(s.piano.a4, l.a4_reference);
                    let today = if diff.abs() >= 1.0 {
                        match s.lang {
                            Lang::Es => format!(" (hoy {diff:+.0} cents)"),
                            Lang::En => format!(" (today {diff:+.0} cents)"),
                        }
                    } else {
                        String::new()
                    };
                    let text = match s.lang {
                        Lang::Es => format!(
                            "Última afinación de este piano: {} con La = {:.1} Hz{today}",
                            l.date_label(),
                            l.a4_reference
                        ),
                        Lang::En => format!(
                            "Last tuning of this piano: {} at A = {:.1} Hz{today}",
                            l.date_label(),
                            l.a4_reference
                        ),
                    };
                    (text, l.a4_reference, diff.abs() >= 1.0)
                });
                let use_last_l = s.t("↺ Usar la de la última vez", "↺ Use last time's");
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
                            if let Some((text, a4, differs)) = &last_l {
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(RichText::new(text).color(widgets::BLUE));
                                    if *differs && ui.small_button(use_last_l).clicked() {
                                        self.settings.piano.a4 = *a4;
                                    }
                                });
                            }
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

                        ui.label(RichText::new(keys_l.0).strong());
                        ui.horizontal(|ui| {
                            ui.selectable_value(&mut self.settings.piano.keys, 88, keys_l.1);
                            ui.selectable_value(&mut self.settings.piano.keys, 85, keys_l.2);
                            ui.label(RichText::new(keys_l.3).weak());
                        });
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
        let strike_l = (
            s.t("Captura por golpe", "Strike capture"),
            s.t("Agudos (desde C6)", "Treble (from C6)"),
            s.t("Todo el piano", "Whole piano"),
            s.t("Apagada", "Off"),
            s.t(
                "Junta todo lo que suena en cada golpe y deja el resultado fijo hasta el próximo. Ideal para los agudos que duran poco.",
                "Gathers everything each strike sounds and freezes the result until the next one. Best for short treble notes.",
            ),
        );
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
            s.t(
                "Intervalos de control con su batido esperado (afinadores)",
                "Check intervals with their expected beat rate (for tuners)",
            ),
            s.t(
                "Afino con tira de fieltro (2 pasadas: cuerdas libres y después unísonos)",
                "I use a felt strip (2 passes: free strings, then unisons)",
            ),
            s.t(
                "Como los afinadores: con la tira puesta se afina una cuerda por nota en todo el piano, y en una segunda pasada se hacen los unísonos.",
                "Like pro tuners: with the strip in place you tune one string per note across the piano, then do the unisons in a second pass.",
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
                ui.checkbox(&mut self.settings.show_intervals, l.11);
                ui.checkbox(&mut self.settings.strip_mode, l.12)
                    .on_hover_text(l.13);
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
                        ui.label(strike_l.0).on_hover_text(strike_l.4);
                        ui.horizontal(|ui| {
                            let m = &mut self.settings.strike_mode;
                            ui.selectable_value(m, StrikeMode::Treble, strike_l.1);
                            ui.selectable_value(m, StrikeMode::Always, strike_l.2);
                            ui.selectable_value(m, StrikeMode::Off, strike_l.3);
                        });
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
        let pass = self.strip_pass();
        let steps = match pass {
            StripPass::Off => steps_for(current.string_count, self.settings.remind_mute),
            _ => strip_steps(
                current.string_count,
                pass,
                self.settings.remind_mute && matches!(current.note_index, 0 | 13 | 56),
            ),
        };
        if steps.is_empty() || !self.settings.piano.has_key(current.midi) {
            // Nota de una cuerda en la pasada de unisonos, o tecla que este
            // piano no tiene (pianos de 85): no hay nada que hacer.
            if let Some(f) = self.flow.as_mut() {
                if pass == StripPass::Unisons {
                    f.pass_note();
                } else {
                    f.skip_current_note();
                }
            }
            self.reset_note_state();
            return;
        }
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
        let done_count = self
            .flow
            .as_ref()
            .map(|f| f.session().completed_notes.len())
            .unwrap_or(0);
        let (phase, phase_hint) = phase_text(current.note_index, lang);
        match pass {
            StripPass::Pitch => {
                ui.label(
                    RichText::new(self.t(
                        "Tira de fieltro · Pasada 1 de 2: cuerdas libres",
                        "Felt strip · Pass 1 of 2: free strings",
                    ))
                    .strong()
                    .color(ORANGE),
                );
            }
            StripPass::Unisons => {
                ui.label(
                    RichText::new(self.t(
                        "Tira de fieltro · Pasada 2 de 2: unísonos",
                        "Felt strip · Pass 2 of 2: unisons",
                    ))
                    .strong()
                    .color(ORANGE),
                );
            }
            StripPass::Off => {}
        }
        ui.horizontal(|ui| {
            ui.add(
                egui::ProgressBar::new(done_count as f32 / self.settings.piano.keys.max(1) as f32)
                    .desired_width(260.0)
                    .text(format!(
                        "{done_count}/{} {}",
                        self.settings.piano.keys,
                        self.t("afinadas", "tuned")
                    )),
            );
            ui.label(RichText::new(phase).strong().color(widgets::BLUE))
                .on_hover_text(phase_hint);
            if self.settings.show_details {
                ui.label(RichText::new(phase_hint).weak());
            }
        });

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
            ui.vertical(|ui| {
                if ui
                    .button(self.t("🔊 Escuchar la nota  (R)", "🔊 Play the note  (R)"))
                    .clicked()
                    || ui.input(|i| i.key_pressed(egui::Key::R))
                {
                    self.play_reference(current.target_freq);
                }
                let label = self.t(
                    "Sonido de piano (si no, tono puro)",
                    "Piano sound (else pure tone)",
                );
                ui.checkbox(&mut self.settings.ref_piano, label);
            });
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
        let strike_on = self.strike_on();
        let mut shown = self.shown_reading().map(|(r, f)| (r.cents, r.freq, f));
        if strike_on && !shown.is_some_and(|s| s.2) {
            // Entre golpes queda fijo el resultado del ultimo golpe, no la
            // cola del sonido.
            shown = self
                .strikes
                .back()
                .filter(|s| s.enough())
                .map(|s| (s.cents, s.freq, false));
        }
        match step.kind {
            StepKind::Mute => {
                widgets::cents_meter(ui, None, self.settings.tolerance, true, labels);
            }
            _ => {
                self.meters(ui, shown.map(|s| (s.0, s.2)), labels);
                if strike_on {
                    self.strike_panel(ui, step.kind);
                } else {
                    reading_line(ui, shown, &self.settings);
                }
                if step.kind == StepKind::Pitch {
                    self.stability_label(ui);
                }
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
        if step.kind == StepKind::Pitch && self.settings.show_intervals {
            self.interval_panel(ui, current.midi);
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
        if pass != StripPass::Unisons {
            let missing = self
                .flow
                .as_ref()
                .map(|f| f.pending_notes().len())
                .unwrap_or(0);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!(
                        "{} {missing}",
                        self.t("Notas que faltan:", "Notes left:")
                    ))
                    .weak(),
                );
                if ui
                    .button(self.t("🏁 Terminar afinación…", "🏁 Finish tuning…"))
                    .clicked()
                {
                    self.confirm_finish = true;
                }
            });
        }
        egui::CollapsingHeader::new(self.t("Informe hasta ahora", "Report so far"))
            .id_salt("report_live")
            .default_open(false)
            .show(ui, |ui| self.report(ui));

        // Acciones
        let last_cents = if strike_on {
            self.last_good_strike()
                .map(|s| s.cents)
                .or(self.reading.as_ref().map(|r| r.cents))
        } else {
            self.reading.as_ref().map(|r| r.cents)
        };
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
            } else if pass == StripPass::Unisons {
                // Conserva el desvio medido en la primera pasada.
                flow.pass_note();
                changed_note = true;
            } else {
                flow.confirm_current_note(final_cents);
                changed_note = true;
            }
        } else if back {
            if self.step > 0 {
                self.step -= 1;
                changed_step = true;
            } else if pass == StripPass::Unisons {
                let prev = current
                    .note_index
                    .checked_sub(1)
                    .and_then(|i| TuningOrder::new().note_at(i))
                    .map(|n| n.midi);
                changed_note = prev.is_some_and(|m| flow.go_to_midi(m));
            } else if flow.go_back_note() {
                changed_note = true;
                // Saltar hacia atras las teclas que el piano no tiene.
                let top = self.settings.piano.top_midi();
                while flow.current_note().is_some_and(|n| n.midi > top) && flow.go_back_note() {}
            }
        } else if skip {
            if pass == StripPass::Unisons {
                flow.pass_note();
            } else {
                flow.skip_current_note();
            }
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

    /// Resultado fijo del ultimo golpe + historial de golpes.
    fn strike_panel(&self, ui: &mut egui::Ui, kind: StepKind) {
        let tol = self.settings.tolerance;
        let unison = kind == StepKind::Unison;
        let reference = self.pitch_cents.unwrap_or(0.0);
        ui.vertical_centered(|ui| {
            let Some(last) = self.strikes.back().copied() else {
                ui.label(
                    RichText::new(self.t(
                        "Tocá la tecla con un golpe firme: cada golpe queda capturado.",
                        "Strike the key firmly: each strike gets captured.",
                    ))
                    .size(20.0)
                    .weak(),
                );
                return;
            };
            if !last.enough() {
                ui.label(
                    RichText::new(self.t(
                        "Golpe débil: tocá más fuerte o acercá el micrófono.",
                        "Weak strike: play harder or move the mic closer.",
                    ))
                    .size(20.0)
                    .color(ORANGE),
                );
            } else {
                let off = if unison { last.cents - reference } else { last.cents };
                let ok = last.good() && off.abs() <= tol && (!unison || last.iqr <= 3.0);
                let mut text = format!(
                    "{} {:+.1} cents   ·   {:.2} Hz",
                    self.t("Golpe:", "Strike:"),
                    last.cents,
                    last.freq
                );
                if ok {
                    text.push_str(self.t("   ·   ¡Afinada!", "   ·   In tune!"));
                }
                let mut rt = RichText::new(text).size(24.0).strong();
                if ok {
                    rt = rt.color(GREEN);
                }
                ui.label(rt);
                if unison {
                    ui.label(format!(
                        "{} {reference:+.1} cents",
                        self.t(
                            "Igualala a la cuerda ya afinada:",
                            "Match it to the string already tuned:"
                        )
                    ));
                }
                if last.iqr > if unison { 3.0 } else { 6.0 } {
                    ui.label(
                        RichText::new(if unison {
                            self.t(
                                "Hay batido: las cuerdas todavía no suenan iguales.",
                                "Beating: the strings don't match yet.",
                            )
                        } else {
                            self.t(
                                "Golpe inestable (¿suena otra cuerda o hay ruido?): repetilo.",
                                "Unsteady strike (another string sounding, or noise?): repeat it.",
                            )
                        })
                        .color(ORANGE),
                    );
                }
            }
            if self.strikes.len() > 1 {
                let hist: Vec<String> = self
                    .strikes
                    .iter()
                    .map(|s| {
                        if s.enough() {
                            format!("{:+.1}", s.cents)
                        } else {
                            "—".into()
                        }
                    })
                    .collect();
                ui.label(
                    RichText::new(format!(
                        "{} {}",
                        self.t("Últimos golpes:", "Last strikes:"),
                        hist.join("  ·  ")
                    ))
                    .weak(),
                );
            }
        });
    }

    /// Intervalos con notas ya afinadas y su batido esperado.
    fn interval_panel(&self, ui: &mut egui::Ui, midi: u8) {
        let Some(flow) = &self.flow else { return };
        let done = self.done_map();
        let checks =
            intervals::checks(midi, |m| done.contains_key(&m), |m| flow.target_for_midi(m));
        if checks.is_empty() {
            return;
        }
        let es = self.settings.lang == Lang::Es;
        egui::CollapsingHeader::new(
            RichText::new(self.t(
                "Intervalos de control (tocá las dos teclas juntas)",
                "Check intervals (play both keys together)",
            ))
            .strong(),
        )
        .id_salt("intervals")
        .default_open(true)
        .show(ui, |ui| {
            egui::Grid::new("intervals_grid")
                .num_columns(3)
                .spacing([20.0, 4.0])
                .striped(true)
                .show(ui, |ui| {
                    for c in &checks {
                        let other = Note::from_midi(c.other)
                            .map(|n| n.display_name())
                            .unwrap_or_default();
                        let dir = match (c.other_below, es) {
                            (true, true) => "abajo",
                            (false, true) => "arriba",
                            (true, false) => "below",
                            (false, false) => "above",
                        };
                        ui.label(format!("{} {dir}: {other}", c.interval.name(es)));
                        ui.label(
                            RichText::new(format!(
                                "≈ {:.1} {}",
                                c.beats,
                                self.t("batidos/s", "beats/s")
                            ))
                            .strong(),
                        );
                        ui.label(
                            RichText::new(if c.beats < 0.3 {
                                self.t("casi sin batido", "almost beatless")
                            } else if c.wide {
                                self.t("ancha", "wide")
                            } else {
                                self.t("angosta", "narrow")
                            })
                            .weak(),
                        );
                        ui.end_row();
                    }
                });
            if self.settings.show_details {
                ui.label(
                    RichText::new(self.t(
                        "Si el batido que escuchás es mucho más rápido o más lento que el esperado, revisá la nota. Las terceras tienen que acelerar parejo al subir.",
                        "If the beat you hear is much faster or slower than expected, recheck the note. Thirds should speed up evenly as you go up.",
                    ))
                    .weak(),
                );
            }
        });
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
        if self.strip_pass() == StripPass::Pitch {
            ui.heading(
                RichText::new(self.t("¡Primera pasada lista!", "First pass done!"))
                    .size(36.0)
                    .strong(),
            );
            ui.label(self.t(
                "Todas las cuerdas libres están afinadas. Ahora van los unísonos, nota por nota, sacando la tira de a poco.",
                "Every free string is tuned. Now the unisons, note by note, pulling the strip as you go.",
            ));
            if ui
                .add(
                    egui::Button::new(
                        RichText::new(self.t("Empezar los unísonos ▶", "Start the unisons ▶"))
                            .size(19.0)
                            .strong()
                            .color(Color32::WHITE),
                    )
                    .fill(GREEN)
                    .min_size(egui::vec2(290.0, 52.0)),
                )
                .clicked()
            {
                self.start_unison_pass();
            }
            return;
        }
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
        let (pending, skipped) = self
            .flow
            .as_ref()
            .map(|f| (f.pending_notes(), f.session().skipped.clone()))
            .unwrap_or_default();
        if !pending.is_empty() {
            ui.colored_label(
                ORANGE,
                format!(
                    "{} ({}): {}",
                    self.t("Quedaron sin afinar", "Left untuned"),
                    pending.len(),
                    notes_list(&pending, 20)
                ),
            );
        }
        if !skipped.is_empty() {
            ui.label(format!(
                "{} ({}): {}",
                self.t("Salteadas", "Skipped"),
                skipped.len(),
                notes_list(&skipped, 20)
            ));
        }
        ui.add_space(10.0);
        self.report(ui);
        ui.add_space(10.0);
        ui.horizontal_wrapped(|ui| {
            if ui
                .button(self.t("Volver al inicio", "Back to home"))
                .clicked()
            {
                self.go_home();
            }
            let reopen = if pending.is_empty() {
                self.t("↺ Reabrir esta afinación", "↺ Reopen this tuning")
            } else {
                self.t("↺ Seguir con las que faltan", "↺ Continue with the missing ones")
            };
            if ui.button(reopen).clicked() {
                self.reopen_current();
            }
        });
    }

    /// Grafico de desvios + teclado + exportar CSV.
    fn report(&mut self, ui: &mut egui::Ui) {
        let cents: HashMap<u8, f32> = self
            .flow
            .as_ref()
            .map(|f| {
                f.session()
                    .completed_notes
                    .iter()
                    .map(|n| (n.midi, n.final_cents))
                    .collect()
            })
            .unwrap_or_default();
        widgets::deviation_chart(ui, &cents, self.settings.tolerance);
        let done = self.done_map();
        widgets::keyboard(ui, None, &done, string_count);
        ui.horizontal(|ui| {
            if ui
                .button(self.t("📄 Exportar informe (CSV)", "📄 Export report (CSV)"))
                .on_hover_text(self.t(
                    "Se guarda en el Escritorio. Se abre con Excel.",
                    "Saved to the Desktop. Opens in Excel.",
                ))
                .clicked()
            {
                self.export_msg = Some(match self.export_csv() {
                    Ok(path) => (true, path),
                    Err(e) => (false, e.to_string()),
                });
            }
            if let Some((ok, msg)) = &self.export_msg {
                let color = if *ok { GREEN } else { widgets::RED };
                ui.label(RichText::new(msg).color(color));
            }
        });
    }

    /// Escribe el informe de la sesion actual en el Escritorio.
    fn export_csv(&self) -> anyhow::Result<String> {
        let flow = self
            .flow
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("sin sesión"))?;
        let dir = directories::UserDirs::new()
            .and_then(|u| u.desktop_dir().map(|d| d.to_path_buf()))
            .or_else(|| std::env::current_dir().ok())
            .ok_or_else(|| anyhow::anyhow!("sin carpeta"))?;
        let safe: String = self
            .settings
            .piano
            .name
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '_' })
            .collect();
        let stamp = chrono::Local::now().format("%Y-%m-%d_%H%M");
        let path = dir.join(format!("afinacion_{safe}_{stamp}.csv"));
        let mut notes = flow.session().completed_notes.clone();
        notes.sort_by_key(|n| n.midi);
        let mut out = String::from("nota;midi;cuerdas;objetivo_hz;desvio_cents;fecha\n");
        for n in &notes {
            out.push_str(&format!(
                "{};{};{};{:.2};{:.1};{}\n",
                n.note,
                n.midi,
                string_count(n.midi),
                flow.target_for_midi(n.midi),
                n.final_cents,
                n.timestamp
                    .with_timezone(&chrono::Local)
                    .format("%Y-%m-%d %H:%M")
            ));
        }
        std::fs::write(&path, out)?;
        Ok(path.display().to_string())
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
        ui.horizontal(|ui| {
            ui.label(format!(
                "{}   ·   A4 = {:.1} Hz   ·",
                self.t("Afinador libre", "Free tuner"),
                self.settings.piano.a4
            ));
            let label = self.t(
                "Con el estiramiento de mi piano",
                "With my piano's stretch",
            );
            let hint = self.t(
                "Prendido: compara contra el mismo objetivo que la afinación guiada. Apagado: temperamento igual puro.",
                "On: compares against the same target as guided tuning. Off: plain equal temperament.",
            );
            ui.checkbox(&mut self.settings.free_stretch, label)
                .on_hover_text(hint);
        });
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
        self.stability_label(ui);
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
            "Tu piano está {:.0} cents abajo de 440. Afinarlo donde está es lo más seguro. Para subirlo a 440 conviene ir de a poco (ej. dos pasadas, primero a la mitad): subir mucho de golpe puede romper cuerdas viejas.",
            -cents
        ),
        Lang::En => format!(
            "Your piano is {:.0} cents below 440. Tuning it where it is is safest. To bring it to 440, go in steps (e.g. two passes, halfway first): a big raise at once can break old strings.",
            -cents
        ),
    })
}

/// Etapa del orden tradicional segun la posicion: nombre y consejo.
fn phase_text(position: usize, lang: Lang) -> (&'static str, &'static str) {
    let es = lang == Lang::Es;
    if position < 13 {
        if es {
            (
                "Octava de temperamento (F3–F4)",
                "Es la base de todo el piano: tomate tu tiempo y dejala bien pareja.",
            )
        } else {
            (
                "Temperament octave (F3–F4)",
                "The foundation of the whole piano: take your time and get it even.",
            )
        }
    } else if position < 56 {
        if es {
            (
                "Subiendo hacia los agudos",
                "Cada nota se puede chequear con su octava de abajo, ya afinada.",
            )
        } else {
            (
                "Going up to the treble",
                "Each note can be checked against the octave below, already tuned.",
            )
        }
    } else if es {
        (
            "Bajando hacia los graves",
            "Cada nota se puede chequear con su octava de arriba, ya afinada.",
        )
    } else {
        (
            "Going down to the bass",
            "Each note can be checked against the octave above, already tuned.",
        )
    }
}

/// Distancia en cents de `a` respecto de `b` (negativo: `a` mas bajo).
fn cents_between(a: f32, b: f32) -> f32 {
    1200.0 * (a / b).log2()
}

/// "E3, D#3, D3 …" con como mucho `max` notas.
fn notes_list(midis: &[u8], max: usize) -> String {
    let mut names: Vec<String> = midis
        .iter()
        .take(max)
        .filter_map(|m| Note::from_midi(*m).map(|n| n.display_name()))
        .collect();
    if midis.len() > max {
        names.push("…".into());
    }
    names.join(", ")
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
        let es = self.settings.lang == Lang::Es;
        ctx.data_mut(|d| d.insert_temp(egui::Id::new(widgets::LANG_ES_ID), es));
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
        if self.confirm_finish {
            self.finish_dialog(ctx);
        }
        if self.choose_a4 {
            self.a4_dialog(ctx);
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
    fn strip_passes_split_the_work() {
        let kinds = |v: Vec<Step>| v.iter().map(|s| s.kind).collect::<Vec<_>>();
        use StepKind::*;
        assert_eq!(kinds(strip_steps(3, StripPass::Pitch, false)), vec![Pitch]);
        assert_eq!(
            kinds(strip_steps(3, StripPass::Pitch, true)),
            vec![Mute, Pitch]
        );
        assert!(strip_steps(1, StripPass::Unisons, false).is_empty());
        assert_eq!(
            kinds(strip_steps(2, StripPass::Unisons, false)),
            vec![Unison]
        );
        assert_eq!(
            kinds(strip_steps(3, StripPass::Unisons, false)),
            vec![Unison, Unison]
        );
        // sin tira: 3 cuerdas con recordatorio = 4 pasos
        assert_eq!(steps_for(3, true).len(), 4);
        assert_eq!(steps_for(3, false).len(), 3);
    }

    #[test]
    fn cents_between_pitches() {
        assert!((cents_between(436.1, 440.4) + 17.0).abs() < 0.5);
        assert!(cents_between(440.0, 440.0).abs() < 1e-4);
        assert_eq!(notes_list(&[52, 51, 50], 2), "E3, D#3, …");
    }

    #[test]
    fn warns_on_big_pitch_raise() {
        assert!(pitch_raise_warning(417.0, Lang::Es).is_some());
        assert!(pitch_raise_warning(440.0, Lang::Es).is_none());
        assert!(pitch_raise_warning(436.0, Lang::Es).is_none());
    }
}
