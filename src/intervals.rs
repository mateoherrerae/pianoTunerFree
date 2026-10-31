//! Intervalos de control para el afinador avanzado.
//!
//! Al tocar dos notas juntas, los armonicos que casi coinciden baten. En el
//! temperamento igual cada intervalo tiene un batido esperado: las quintas
//! son un poco angostas y baten lento, las cuartas y terceras son anchas y
//! baten mas rapido. Comparar el batido que se escucha con el esperado es
//! la forma clasica de chequear una nota contra las ya afinadas.

/// Un intervalo de control.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interval {
    MajorThird,
    Fourth,
    Fifth,
    MajorSixth,
    Octave,
}

impl Interval {
    pub const ALL: [Interval; 5] = [
        Interval::MajorThird,
        Interval::Fourth,
        Interval::Fifth,
        Interval::MajorSixth,
        Interval::Octave,
    ];

    /// Semitonos del intervalo.
    pub fn semitones(self) -> u8 {
        match self {
            Interval::MajorThird => 4,
            Interval::Fourth => 5,
            Interval::Fifth => 7,
            Interval::MajorSixth => 9,
            Interval::Octave => 12,
        }
    }

    /// Armonicos que coinciden: (de la nota de abajo, de la de arriba).
    pub fn partials(self) -> (f32, f32) {
        match self {
            Interval::MajorThird => (5.0, 4.0),
            Interval::Fourth => (4.0, 3.0),
            Interval::Fifth => (3.0, 2.0),
            Interval::MajorSixth => (5.0, 3.0),
            Interval::Octave => (2.0, 1.0),
        }
    }

    pub fn name(self, es: bool) -> &'static str {
        match (self, es) {
            (Interval::MajorThird, true) => "3ra mayor",
            (Interval::Fourth, true) => "4ta",
            (Interval::Fifth, true) => "5ta",
            (Interval::MajorSixth, true) => "6ta mayor",
            (Interval::Octave, true) => "8va",
            (Interval::MajorThird, false) => "Major 3rd",
            (Interval::Fourth, false) => "4th",
            (Interval::Fifth, false) => "5th",
            (Interval::MajorSixth, false) => "Major 6th",
            (Interval::Octave, false) => "Octave",
        }
    }
}

/// Batido esperado entre la nota actual y otra ya afinada.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IntervalCheck {
    pub interval: Interval,
    /// La otra nota (MIDI).
    pub other: u8,
    /// La otra nota esta abajo de la actual.
    pub other_below: bool,
    /// Batidos por segundo esperados.
    pub beats: f32,
    /// El intervalo queda mas ancho que el puro.
    pub wide: bool,
}

/// Batido de dos frecuencias para un intervalo: (batidos/s, ancho).
pub fn beat_rate(interval: Interval, lower: f32, upper: f32) -> (f32, bool) {
    let (pl, pu) = interval.partials();
    let diff = pl * lower - pu * upper;
    (diff.abs(), diff < 0.0)
}

/// Intervalos de control posibles para `midi` contra notas ya afinadas.
/// `target` da la frecuencia objetivo de cada nota.
pub fn checks(
    midi: u8,
    is_done: impl Fn(u8) -> bool,
    target: impl Fn(u8) -> f32,
) -> Vec<IntervalCheck> {
    let mut out = Vec::new();
    for interval in Interval::ALL {
        let st = interval.semitones();
        let below = midi.checked_sub(st).filter(|m| *m >= 21);
        let above = Some(midi + st).filter(|m| *m <= 108);
        for (other, other_below) in [(below, true), (above, false)] {
            let Some(other) = other else { continue };
            if !is_done(other) {
                continue;
            }
            let (lo, hi) = if other_below {
                (target(other), target(midi))
            } else {
                (target(midi), target(other))
            };
            let (beats, wide) = beat_rate(interval, lo, hi);
            out.push(IntervalCheck {
                interval,
                other,
                other_below,
                beats,
                wide,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn et(midi: u8) -> f32 {
        440.0 * 2f32.powf((midi as f32 - 69.0) / 12.0)
    }

    #[test]
    fn equal_temperament_textbook_rates() {
        // F3-C4 (quinta) ~0.6/s angosta; F3-A3 (3ra) ~6.9/s ancha
        let (b, wide) = beat_rate(Interval::Fifth, et(53), et(60));
        assert!((b - 0.6).abs() < 0.1, "{b}");
        assert!(!wide);
        let (b, wide) = beat_rate(Interval::MajorThird, et(53), et(57));
        assert!((b - 6.9).abs() < 0.2, "{b}");
        assert!(wide);
        let (b, wide) = beat_rate(Interval::Fourth, et(53), et(58));
        assert!((b - 0.8).abs() < 0.1, "{b}");
        assert!(wide);
        let (b, _) = beat_rate(Interval::Octave, et(57), et(69));
        assert!(b < 1e-3);
    }

    #[test]
    fn only_lists_done_notes() {
        let c = checks(60, |m| m == 53 || m == 48, et);
        let others: Vec<u8> = c.iter().map(|c| c.other).collect();
        assert_eq!(others, vec![53, 48]);
        assert!(c.iter().all(|c| c.other_below));
    }

    #[test]
    fn stays_in_piano_range() {
        let c = checks(21, |_| true, et);
        assert!(c.iter().all(|c| !c.other_below));
        let c = checks(108, |_| true, et);
        assert!(c.iter().all(|c| c.other_below));
    }
}
