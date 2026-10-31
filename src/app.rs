//! Ventana principal: ajustes, afinacion guiada y afinador libre.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, RichText};

use piano_tuner_free::audio::{AudioOutput, MicCapture, PitchUpdate, PitchWorker};
use piano_tuner_free::settings::Settings;
use piano_tuner_free::tuning::{
    Note, StretchCurve, Temperament, TuningFlow, TuningMode, TuningOrder,
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

/// Un paso de afinacion para una nota con varias cuerdas.
struct Step {
    title: &'static str,
    detail: &'static str,
    /// Paso de apagar cuerdas: no hay que medir nada.
    mute: bool,
}

fn steps_for(strings: u8) -> &'static [Step] {
    const ONE: &[Step] = &[Step {
        title: "Afiná la cuerda",
        detail: "Tocá la tecla y girá la clavija hasta que la barra quede en la zona verde.",
        mute: false,
    }];
    const TWO: &[Step] = &[
        Step {
            title: "Apagá la cuerda derecha",
            detail: "Poné una cuña de goma o fieltro para que suene solo la cuerda izquierda.",
            mute: true,
        },
        Step {
            title: "Afiná la cuerda izquierda",
            detail: "Tocá la tecla y llevá la barra a la zona verde.",
            mute: false,
        },
        Step {
            title: "Afiná la cuerda derecha",
            detail: "Sacá la cuña. Afiná la derecha hasta que las dos suenen juntas sin batido (el \"uaua\").",
            mute: false,
        },
    ];
    const THREE: &[Step] = &[
        Step {
            title: "Apagá las cuerdas de los costados",
            detail: "Poné fieltro o cuñas para que suene solo la cuerda del medio.",
            mute: true,
        },
        Step {
            title: "Afiná la cuerda del medio",
            detail: "Tocá la tecla y llevá la barra a la zona verde.",
            mute: false,
        },
        Step {
            title: "Afiná la cuerda izquierda",
            detail: "Liberá la izquierda y afinala contra la del medio hasta que no haya batido.",
            mute: false,
        },
        Step {
            title: "Afiná la cuerda derecha",
            detail: "Liberá la derecha y afinala contra las otras dos hasta que no haya batido.",
            mute: false,
        },
    ];
    match strings {
        1 => ONE,
        2 => TWO,
        _ => THREE,
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
        }
    }

    fn temperament(&self) -> Temperament {
        Temperament::with_a4(self.settings.a4)
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
            Err(e) => self.audio_error = Some(format!("No pude abrir el micrófono: {e}")),
        }
    }

    fn play_reference(&mut self, freq: f32) {
        if self.output.is_none() {
            match AudioOutput::new() {
                Ok(o) => self.output = Some(o),
                Err(e) => {
                    self.audio_error = Some(format!("No pude usar los parlantes: {e}"));
                    return;
                }
            }
        }
        if let Some(out) = &self.output {
            let _ = out.play_sine(freq, 2.0);
        }
    }

    fn start_tuning(&mut self) {
        self.settings.apply_string_layout();
        let stretch = self.settings.stretch.then(StretchCurve::railsback_default);
        self.flow = Some(TuningFlow::new(
            TuningMode::Concert,
            TuningOrder::new(),
            self.temperament(),
            stretch,
        ));
        self.step = 0;
        self.reading = None;
        self.ensure_mic();
        self.screen = Screen::Tuning;
    }

    fn start_free(&mut self) {
        self.reading = None;
        self.ensure_mic();
        self.screen = Screen::Free;
    }

    fn reset_note_state(&mut self) {
        self.step = 0;
        self.reading = None;
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
            self.audio_error = Some(format!("Problema con el micrófono: {w}"));
        }
        match worker.latest_update() {
            Some(PitchUpdate::Detected {
                frequency,
                confidence,
                ..
            }) if confidence > 0.6 => {
                let temperament = Temperament::with_a4(self.settings.a4);
                let (cents, midi) = match (target, &self.flow) {
                    (Some(t), Some(flow)) => (flow.cents_from_target(frequency, t), None),
                    _ => match temperament.nearest_note(frequency) {
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

    // ---------------------------------------------------------------- Inicio

    fn ui_home(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.heading(RichText::new("Piano Tuner Free").size(30.0).strong());
        ui.label("Afinador de piano gratuito. Configurá tu piano y elegí cómo afinar.");
        ui.add_space(14.0);

        egui::Grid::new("settings")
            .num_columns(2)
            .spacing([24.0, 12.0])
            .show(ui, |ui| {
                ui.label(RichText::new("La central (A4)").strong());
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
                    ui.label(
                        RichText::new("Pianos viejos: dejalo donde está el piano hoy (ej. 417).")
                            .weak(),
                    );
                });
                ui.end_row();

                ui.label(RichText::new("Empiezan 2 cuerdas en").strong());
                ui.horizontal(|ui| {
                    note_combo(ui, "bi", &mut self.settings.bichord_from, 22, 107);
                    ui.label(RichText::new("(antes de esta nota: 1 cuerda)").weak());
                });
                ui.end_row();

                ui.label(RichText::new("Empiezan 3 cuerdas en").strong());
                ui.horizontal(|ui| {
                    let min = self.settings.bichord_from;
                    note_combo(ui, "tri", &mut self.settings.trichord_from, min, 108);
                });
                ui.end_row();

                ui.label(RichText::new("Margen de afinado").strong());
                ui.add(
                    egui::Slider::new(&mut self.settings.tolerance, 1.0..=10.0).suffix(" cents"),
                );
                ui.end_row();

                ui.label(RichText::new("Mantener última lectura").strong());
                ui.horizontal(|ui| {
                    ui.add(egui::Slider::new(&mut self.settings.hold_secs, 0.0..=8.0).suffix(" s"));
                    ui.label(RichText::new("Útil para los agudos que se apagan rápido.").weak());
                });
                ui.end_row();

                ui.label(RichText::new("Estiramiento").strong());
                ui.checkbox(
                    &mut self.settings.stretch,
                    "Subir un poco los agudos y bajar los graves (como un afinador profesional)",
                );
                ui.end_row();
            });

        if self.settings.trichord_from < self.settings.bichord_from {
            self.settings.trichord_from = self.settings.bichord_from;
        }
        self.settings.apply_string_layout();

        ui.add_space(14.0);
        ui.label(RichText::new("Cuerdas de tu piano").strong());
        widgets::keyboard(ui, None, &HashSet::new(), string_count);
        ui.horizontal(|ui| {
            for (n, label) in [(1, "1 cuerda"), (2, "2 cuerdas"), (3, "3 cuerdas")] {
                ui.label(RichText::new("■").color(strings_color(n)));
                ui.label(label);
                ui.add_space(10.0);
            }
        });

        ui.add_space(20.0);
        ui.horizontal(|ui| {
            let big = egui::vec2(300.0, 56.0);
            if ui
                .add(
                    egui::Button::new(
                        RichText::new("Afinar el piano (88 notas)")
                            .size(19.0)
                            .strong(),
                    )
                    .min_size(big),
                )
                .clicked()
            {
                self.start_tuning();
            }
            ui.add_space(12.0);
            if ui
                .add(egui::Button::new(RichText::new("Afinador libre").size(19.0)).min_size(big))
                .clicked()
            {
                self.start_free();
            }
        });
        ui.add_space(6.0);
        ui.label(
            RichText::new(
                "El modo de 88 notas te guía nota por nota, empezando por la octava central. \
                 El afinador libre reconoce cualquier nota que toques.",
            )
            .weak(),
        );

        if self.settings != self.saved_settings {
            self.settings.save();
            self.saved_settings = self.settings.clone();
        }
    }

    // ------------------------------------------------------------- Afinacion

    fn ui_tuning(&mut self, ui: &mut egui::Ui) {
        let Some(current) = self.flow.as_ref().and_then(|f| f.current_note()) else {
            ui.add_space(40.0);
            ui.heading("¡Terminaste! Afinaste todas las notas.");
            if ui.button("Volver al inicio").clicked() {
                self.screen = Screen::Home;
            }
            return;
        };
        let steps = steps_for(current.string_count);
        self.step = self.step.min(steps.len() - 1);
        let step = &steps[self.step];
        let note = Note::from_midi(current.midi);
        let name = note.map(|n| n.name).unwrap_or("?");

        // Encabezado
        ui.horizontal(|ui| {
            if ui.button("← Inicio").clicked() {
                self.screen = Screen::Home;
            }
            ui.add_space(10.0);
            ui.label(format!(
                "Nota {} de {}   ·   La = {:.1} Hz",
                current.note_index + 1,
                current.total_notes,
                self.settings.a4
            ));
        });
        ui.add_space(6.0);

        ui.horizontal(|ui| {
            ui.label(RichText::new(&current.display_name).size(72.0).strong());
            ui.add_space(14.0);
            ui.vertical(|ui| {
                ui.add_space(10.0);
                ui.label(RichText::new(solfege(name)).size(24.0));
                ui.label(
                    RichText::new(format!(
                        "{} cuerda{}",
                        current.string_count,
                        if current.string_count > 1 { "s" } else { "" }
                    ))
                    .color(strings_color(current.string_count))
                    .size(18.0),
                );
                ui.label(format!("Objetivo: {:.2} Hz", current.target_freq));
            });
            ui.add_space(30.0);
            if ui.button("🔊 Escuchar la nota").clicked() {
                self.play_reference(current.target_freq);
            }
        });

        ui.add_space(8.0);
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new(format!(
                    "Paso {} de {}: {}",
                    self.step + 1,
                    steps.len(),
                    step.title
                ))
                .size(22.0)
                .strong()
                .color(widgets::BLUE),
            );
            ui.label(RichText::new(step.detail).size(16.0));
        });

        ui.add_space(8.0);
        let shown = self.shown_reading();
        if step.mute {
            ui.label(
                RichText::new("Cuando tengas las cuerdas apagadas, apretá «Listo».").size(16.0),
            );
            widgets::cents_meter(ui, None, self.settings.tolerance, true);
        } else {
            widgets::cents_meter(
                ui,
                shown.map(|(r, _)| r.cents),
                self.settings.tolerance,
                shown.map(|(_, f)| f).unwrap_or(false),
            );
        }
        reading_line(ui, shown, self.settings.tolerance);

        ui.add_space(10.0);
        let mut confirm = false;
        let mut back = false;
        let mut skip = false;
        ui.horizontal(|ui| {
            let size = egui::vec2(170.0, 46.0);
            back = ui
                .add(egui::Button::new("◀ Anterior").min_size(size))
                .clicked();
            skip = ui
                .add(egui::Button::new("Saltar nota ▶").min_size(size))
                .clicked();
            confirm = ui
                .add(
                    egui::Button::new(
                        RichText::new("Listo ✔  (Espacio)")
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

        ui.add_space(12.0);
        ui.label(RichText::new("Tocá una tecla del dibujo para saltar a esa nota.").weak());
        let done: HashSet<u8> = current
            .completed_chromatic_indices
            .iter()
            .map(|i| *i as u8 + 21)
            .collect();
        let clicked = widgets::keyboard(ui, Some(current.midi), &done, string_count);

        // Acciones
        let Some(flow) = self.flow.as_mut() else {
            return;
        };
        if confirm {
            if self.step + 1 < steps.len() {
                self.step += 1;
                self.reading = None;
                flow.rearm_beep_latch();
            } else {
                let cents = self.reading.as_ref().map(|r| r.cents).unwrap_or(0.0);
                flow.confirm_current_note(cents);
                self.reset_note_state();
            }
        } else if back {
            if self.step > 0 {
                self.step -= 1;
                self.reading = None;
            } else if flow.go_back_note() {
                self.reset_note_state();
            }
        } else if skip {
            flow.skip_current_note();
            self.reset_note_state();
        } else if let Some(midi) = clicked {
            if flow.go_to_midi(midi) {
                self.reset_note_state();
            }
        }
    }

    // --------------------------------------------------------- Afinador libre

    fn ui_free(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("← Inicio").clicked() {
                self.screen = Screen::Home;
            }
            ui.add_space(10.0);
            ui.label(format!(
                "Afinador libre   ·   La = {:.1} Hz",
                self.settings.a4
            ));
        });
        ui.add_space(20.0);
        let shown = self.shown_reading();
        let midi = shown.and_then(|(r, _)| r.midi);
        let note = midi.and_then(Note::from_midi);
        ui.vertical_centered(|ui| match note {
            Some(n) => {
                ui.label(RichText::new(n.display_name()).size(96.0).strong());
                ui.label(RichText::new(solfege(n.name)).size(28.0));
            }
            None => {
                ui.label(RichText::new("—").size(96.0).weak());
                ui.label(RichText::new("Tocá una nota").size(28.0).weak());
            }
        });
        ui.add_space(10.0);
        widgets::cents_meter(
            ui,
            shown.map(|(r, _)| r.cents),
            self.settings.tolerance,
            shown.map(|(_, f)| f).unwrap_or(false),
        );
        reading_line(ui, shown, self.settings.tolerance);
        ui.add_space(14.0);
        let done = HashSet::new();
        widgets::keyboard(ui, midi, &done, string_count);
    }
}

fn string_count(midi: u8) -> u8 {
    Note::from_midi(midi).map(|n| n.string_count()).unwrap_or(1)
}

fn reading_line(ui: &mut egui::Ui, shown: Option<(&Reading, bool)>, tolerance: f32) {
    ui.vertical_centered(|ui| match shown {
        Some((r, fresh)) => {
            let ok = r.cents.abs() <= tolerance;
            let text = format!(
                "{:+.1} cents   ·   {:.2} Hz{}{}",
                r.cents,
                r.freq,
                if ok { "   ·   ¡Afinada!" } else { "" },
                if fresh { "" } else { "   (última lectura)" }
            );
            let mut rt = RichText::new(text).size(22.0).strong();
            if ok {
                rt = rt.color(GREEN);
            }
            ui.label(rt);
        }
        None => {
            ui.label(RichText::new("Esperando sonido…").size(22.0).weak());
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
        if let Some(err) = &self.audio_error {
            let err = err.clone();
            egui::TopBottomPanel::top("error").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.colored_label(widgets::RED, err);
                    if ui.button("Reintentar").clicked() {
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
        egui::TopBottomPanel::bottom("footer").show(ctx, |ui| {
            ui.label(
                RichText::new("Piano Tuner Free · open source (MIT) · motor basado en pianito de Axel Stevens")
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
    }
}
