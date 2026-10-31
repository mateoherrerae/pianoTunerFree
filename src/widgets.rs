//! Dibujos propios: medidor de cents y teclado de 88 teclas.

use std::collections::HashMap;

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};

pub const GREEN: Color32 = Color32::from_rgb(46, 170, 90);
pub const BLUE: Color32 = Color32::from_rgb(60, 130, 230);
pub const ORANGE: Color32 = Color32::from_rgb(230, 140, 40);
pub const RED: Color32 = Color32::from_rgb(220, 70, 60);

/// Colores para las zonas de 1, 2 y 3 cuerdas.
pub fn strings_color(strings: u8) -> Color32 {
    match strings {
        1 => Color32::from_rgb(170, 120, 200),
        2 => Color32::from_rgb(230, 170, 60),
        _ => Color32::from_rgb(70, 160, 200),
    }
}

/// Nombre en solfeo (Do, Re, Mi...) para un nombre ingles (C, C#, D...).
pub fn solfege(name: &str) -> String {
    let (base, rest) = name.split_at(1);
    let s = match base {
        "C" => "Do",
        "D" => "Re",
        "E" => "Mi",
        "F" => "Fa",
        "G" => "Sol",
        "A" => "La",
        "B" => "Si",
        _ => base,
    };
    format!("{s}{rest}")
}

fn is_black(midi: u8) -> bool {
    matches!(midi % 12, 1 | 3 | 6 | 8 | 10)
}

/// Medidor horizontal de cents (-50 a +50) con la zona afinada en verde.
/// `cents = None` muestra el medidor vacio. `fresh = false` lo atenua
/// (lectura retenida despues de que la nota se apago).
pub fn cents_meter(
    ui: &mut egui::Ui,
    cents: Option<f32>,
    tolerance: f32,
    fresh: bool,
    labels: (&str, &str),
) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 110.0), Sense::hover());
    let p = ui.painter_at(rect);
    let visuals = ui.visuals();
    let bg = visuals.extreme_bg_color;
    let fg = visuals.text_color();
    let range = 50.0;

    let bar = Rect::from_min_max(
        Pos2::new(rect.left() + 20.0, rect.top() + 30.0),
        Pos2::new(rect.right() - 20.0, rect.top() + 80.0),
    );
    p.rect_filled(bar, 8.0, bg);
    let x_of = |c: f32| bar.center().x + (c.clamp(-range, range) / range) * bar.width() / 2.0;

    // Zona afinada
    let zone = Rect::from_min_max(
        Pos2::new(x_of(-tolerance), bar.top()),
        Pos2::new(x_of(tolerance), bar.bottom()),
    );
    p.rect_filled(zone, 4.0, GREEN.gamma_multiply(0.35));

    // Marcas
    for c in [-50, -40, -30, -20, -10, 0, 10, 20, 30, 40, 50] {
        let x = x_of(c as f32);
        let h = if c == 0 { 0.0 } else { 14.0 };
        p.line_segment(
            [Pos2::new(x, bar.top() + h), Pos2::new(x, bar.bottom() - h)],
            Stroke::new(if c == 0 { 2.0_f32 } else { 1.0 }, fg.gamma_multiply(0.5)),
        );
        p.text(
            Pos2::new(x, bar.bottom() + 12.0),
            Align2::CENTER_CENTER,
            if c > 0 {
                format!("+{c}")
            } else {
                c.to_string()
            },
            FontId::proportional(12.0),
            fg.gamma_multiply(0.6),
        );
    }
    p.text(
        Pos2::new(bar.left(), rect.top() + 12.0),
        Align2::LEFT_CENTER,
        labels.0,
        FontId::proportional(13.0),
        fg.gamma_multiply(0.6),
    );
    p.text(
        Pos2::new(bar.right(), rect.top() + 12.0),
        Align2::RIGHT_CENTER,
        labels.1,
        FontId::proportional(13.0),
        fg.gamma_multiply(0.6),
    );

    if let Some(c) = cents {
        let color = if c.abs() <= tolerance {
            GREEN
        } else if c.abs() <= 15.0 {
            ORANGE
        } else {
            RED
        };
        let color = if fresh {
            color
        } else {
            color.gamma_multiply(0.45)
        };
        let x = x_of(c);
        p.rect_filled(
            Rect::from_center_size(
                Pos2::new(x, bar.center().y),
                Vec2::new(8.0, bar.height() + 16.0),
            ),
            3.0,
            color,
        );
    }
}

/// Teclado de 88 teclas. Pinta la tecla actual, las ya afinadas y una
/// franja abajo con la cantidad de cuerdas. Devuelve la tecla clickeada.
pub fn keyboard(
    ui: &mut egui::Ui,
    current: Option<u8>,
    done: &HashMap<u8, bool>,
    string_count: impl Fn(u8) -> u8,
) -> Option<u8> {
    let width = ui.available_width();
    let white_w = width / 52.0;
    let height = (white_w * 5.0).clamp(70.0, 130.0);
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height + 12.0), Sense::click());
    let p = ui.painter_at(rect);

    // Posicion de cada tecla
    let mut whites = Vec::new();
    let mut blacks = Vec::new();
    let mut wi = 0.0;
    for midi in 21u8..=108 {
        if is_black(midi) {
            let x = rect.left() + wi * white_w - white_w * 0.32;
            blacks.push((
                midi,
                Rect::from_min_size(
                    Pos2::new(x, rect.top()),
                    Vec2::new(white_w * 0.64, height * 0.62),
                ),
            ));
        } else {
            let x = rect.left() + wi * white_w;
            whites.push((
                midi,
                Rect::from_min_size(Pos2::new(x, rect.top()), Vec2::new(white_w, height)),
            ));
            wi += 1.0;
        }
    }

    let fill = |midi: u8, black: bool| {
        if Some(midi) == current {
            return BLUE;
        }
        match done.get(&midi) {
            Some(true) if black => Color32::from_rgb(25, 110, 55),
            Some(true) => Color32::from_rgb(150, 220, 170),
            Some(false) if black => Color32::from_rgb(150, 80, 10),
            Some(false) => Color32::from_rgb(245, 195, 130),
            None if black => Color32::from_gray(25),
            None => Color32::from_gray(245),
        }
    };

    for (midi, r) in &whites {
        p.rect_filled(r.shrink(0.5), 2.0, fill(*midi, false));
        p.line_segment(
            [r.right_top(), r.right_bottom()],
            Stroke::new(1.0_f32, Color32::from_gray(120)),
        );
        // franja de cuerdas
        let band = Rect::from_min_size(
            Pos2::new(r.left(), r.bottom() + 3.0),
            Vec2::new(r.width(), 7.0),
        );
        p.rect_filled(band, 0.0, strings_color(string_count(*midi)));
        if midi % 12 == 0 && white_w > 11.0 {
            p.text(
                Pos2::new(r.center().x, r.bottom() - 8.0),
                Align2::CENTER_CENTER,
                format!("C{}", midi / 12 - 1),
                FontId::proportional(9.0),
                Color32::from_gray(90),
            );
        }
    }
    for (midi, r) in &blacks {
        p.rect_filled(*r, 2.0, fill(*midi, true));
    }

    if response.clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            if let Some((m, _)) = blacks.iter().find(|(_, r)| r.contains(pos)) {
                return Some(*m);
            }
            if let Some((m, _)) = whites
                .iter()
                .find(|(_, r)| r.left() <= pos.x && pos.x < r.right())
            {
                return Some(*m);
            }
        }
    }
    None
}

/// Curva de volumen (dB) de los ultimos segundos: en un unisono
/// desafinado se ve ondular ("uaua").
pub fn envelope_plot(ui: &mut egui::Ui, pts: &[(f32, f32)], color: Color32) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 70.0), Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 6.0, ui.visuals().extreme_bg_color);
    if pts.len() < 2 {
        return;
    }
    let t_end = pts.last().map(|x| x.0).unwrap_or(0.0);
    let t_start = t_end - 3.0;
    let (lo, hi) = (-70.0f32, 0.0f32);
    let line: Vec<Pos2> = pts
        .iter()
        .filter(|(t, _)| *t >= t_start)
        .map(|(t, db)| {
            let x = rect.left() + (t - t_start) / 3.0 * rect.width();
            let y = rect.bottom() - ((db.clamp(lo, hi) - lo) / (hi - lo)) * rect.height();
            Pos2::new(x, y)
        })
        .collect();
    p.add(egui::Shape::line(line, Stroke::new(2.0_f32, color)));
}

/// Lupa: medidor de -10 a +10 cents para el ajuste fino.
pub fn fine_meter(ui: &mut egui::Ui, cents: Option<f32>, tolerance: f32, fresh: bool) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 46.0), Sense::hover());
    let p = ui.painter_at(rect);
    let fg = ui.visuals().text_color();
    let range = 10.0;
    let bar = Rect::from_min_max(
        Pos2::new(rect.left() + 20.0, rect.top() + 4.0),
        Pos2::new(rect.right() - 20.0, rect.top() + 26.0),
    );
    p.rect_filled(bar, 5.0, ui.visuals().extreme_bg_color);
    let x_of = |c: f32| bar.center().x + (c.clamp(-range, range) / range) * bar.width() / 2.0;
    p.rect_filled(
        Rect::from_min_max(
            Pos2::new(x_of(-tolerance), bar.top()),
            Pos2::new(x_of(tolerance), bar.bottom()),
        ),
        3.0,
        GREEN.gamma_multiply(0.3),
    );
    for c in -10..=10 {
        let x = x_of(c as f32);
        let major = c % 5 == 0;
        let h = if c == 0 {
            0.0
        } else if major {
            4.0
        } else {
            8.0
        };
        p.line_segment(
            [Pos2::new(x, bar.top() + h), Pos2::new(x, bar.bottom() - h)],
            Stroke::new(if c == 0 { 2.0_f32 } else { 1.0 }, fg.gamma_multiply(0.45)),
        );
        if major {
            p.text(
                Pos2::new(x, bar.bottom() + 9.0),
                Align2::CENTER_CENTER,
                if c > 0 {
                    format!("+{c}")
                } else {
                    c.to_string()
                },
                FontId::proportional(11.0),
                fg.gamma_multiply(0.55),
            );
        }
    }
    if let Some(c) = cents {
        let color = if c.abs() <= tolerance { GREEN } else { ORANGE };
        let color = if fresh {
            color
        } else {
            color.gamma_multiply(0.45)
        };
        let x = x_of(c);
        if c.abs() > range {
            // fuera de la lupa: flecha en el borde
            let dir = c.signum();
            let tip = Pos2::new(x + dir * 2.0, bar.center().y);
            let back = x - dir * 12.0;
            p.add(egui::Shape::convex_polygon(
                vec![
                    tip,
                    Pos2::new(back, bar.top() + 2.0),
                    Pos2::new(back, bar.bottom() - 2.0),
                ],
                color,
                Stroke::NONE,
            ));
        } else {
            p.circle_filled(Pos2::new(x, bar.center().y), 8.0, color);
        }
    }
}

/// Estroboscopio: franjas que corren a la derecha si la nota esta alta y a
/// la izquierda si esta baja; quietas cuando esta afinada. `phase` es el
/// desplazamiento acumulado en pixeles.
pub fn strobe(ui: &mut egui::Ui, phase: f32, active: bool, in_tune: bool) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 44.0), Sense::hover());
    let p = ui.painter_at(rect);
    let bar = rect.shrink2(Vec2::new(20.0, 2.0));
    p.rect_filled(bar, 6.0, ui.visuals().extreme_bg_color);
    if !active {
        return;
    }
    let color = if in_tune { GREEN } else { BLUE };
    // Dos filas: la de abajo va al doble (como el 2do armonico).
    let h = bar.height() / 2.0;
    for (row, mult, period) in [(0.0f32, 1.0f32, 28.0f32), (1.0, 2.0, 14.0)] {
        let top = bar.top() + row * h;
        let off = (phase * mult).rem_euclid(period * 2.0);
        let mut x = bar.left() - period * 2.0 + off;
        while x < bar.right() {
            let r = Rect::from_min_max(
                Pos2::new(x.max(bar.left()), top + 2.0),
                Pos2::new((x + period).min(bar.right()), top + h - 2.0),
            );
            if r.width() > 0.0 {
                p.rect_filled(r, 0.0, color.gamma_multiply(0.8));
            }
            x += period * 2.0;
        }
    }
}

/// Grafico del desvio final de cada nota, alineado con el teclado de 88.
/// Barras arriba = alta, abajo = baja; verde dentro del margen.
pub fn deviation_chart(ui: &mut egui::Ui, cents: &HashMap<u8, f32>, tolerance: f32) {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 140.0), Sense::hover());
    let p = ui.painter_at(rect);
    let fg = ui.visuals().text_color();
    p.rect_filled(rect, 6.0, ui.visuals().extreme_bg_color);
    let range = 20.0f32;
    let mid = rect.center().y;
    let y_of = |c: f32| mid - (c.clamp(-range, range) / range) * (rect.height() / 2.0 - 8.0);
    // zona del margen y lineas guia
    p.rect_filled(
        Rect::from_min_max(
            Pos2::new(rect.left(), y_of(tolerance)),
            Pos2::new(rect.right(), y_of(-tolerance)),
        ),
        0.0,
        GREEN.gamma_multiply(0.15),
    );
    for c in [-20.0f32, -10.0, 0.0, 10.0, 20.0] {
        let y = y_of(c);
        p.line_segment(
            [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
            Stroke::new(if c == 0.0 { 1.5_f32 } else { 0.5 }, fg.gamma_multiply(0.3)),
        );
        p.text(
            Pos2::new(rect.left() + 4.0, y),
            Align2::LEFT_CENTER,
            format!("{c:+.0}"),
            FontId::proportional(10.0),
            fg.gamma_multiply(0.5),
        );
    }
    let slot = rect.width() / 88.0;
    for (&midi, &c) in cents {
        if !(21..=108).contains(&midi) {
            continue;
        }
        let x = rect.left() + (midi - 21) as f32 * slot + slot / 2.0;
        let color = if c.abs() <= tolerance {
            GREEN
        } else if c.abs() <= 15.0 {
            ORANGE
        } else {
            RED
        };
        let (a, b) = (y_of(0.0), y_of(c));
        p.rect_filled(
            Rect::from_min_max(
                Pos2::new(x - slot * 0.35, a.min(b)),
                Pos2::new(x + slot * 0.35, a.max(b).max(a.min(b) + 1.5)),
            ),
            1.0,
            color,
        );
    }
    if let Some(pos) = response.hover_pos() {
        let midi = 21 + ((pos.x - rect.left()) / slot).floor().clamp(0.0, 87.0) as u8;
        if let Some(c) = cents.get(&midi) {
            let name = crate_note_name(midi);
            response.on_hover_text(format!("{name}: {c:+.1} cents"));
        }
    }
}

fn crate_note_name(midi: u8) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    format!("{}{}", NAMES[(midi % 12) as usize], midi as i32 / 12 - 1)
}
