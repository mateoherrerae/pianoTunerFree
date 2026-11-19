//! Captura por golpe: junta todas las lecturas de un golpe de tecla y da
//! un solo resultado robusto.
//!
//! Las notas agudas duran menos de un segundo. Mostrar la ultima lectura
//! sirve poco (es la cola, cuando el sonido ya se apaga). En cambio, cada
//! golpe se detecta por el salto de volumen, se saltea el ruido del
//! martillo y se toma la mediana de todo lo que se leyo hasta que el
//! sonido se apaga.
//!
//! Es una estructura pura: recibe `now` desde afuera para poder probarla.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Cuanto tiene que subir el volumen respecto del minimo reciente.
const ONSET_RISE_DB: f32 = 9.0;
/// Ventana para ese minimo reciente.
const ONSET_LOOKBACK: Duration = Duration::from_millis(150);
/// Cuanto por encima del ruido de fondo tiene que estar el golpe.
const ONSET_OVER_FLOOR_DB: f32 = 12.0;
/// No hay dos golpes en menos de esto.
const MIN_GAP: Duration = Duration::from_millis(150);
/// Lecturas ignoradas despues del golpe: 40 ms de martillo + 60 ms de la
/// ventana de analisis (cada lectura mira los ultimos 60 ms).
const SKIP_ATTACK: Duration = Duration::from_millis(100);
/// El golpe termina cuando el volumen cae esto debajo del pico...
const END_DROP_DB: f32 = 30.0;
/// ...o a lo sumo despues de este tiempo.
const MAX_STRIKE: Duration = Duration::from_millis(1500);

/// El resultado de un golpe.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StrikeResult {
    /// Mediana de las frecuencias leidas, en Hz (0 si no hubo lecturas).
    pub median_hz: f32,
    /// Dispersion (rango intercuartil) en cents: chica = sonido parejo;
    /// grande = batido, cuerdas falsas o lecturas malas.
    pub iqr_cents: f32,
    /// Cantidad de lecturas (se superponen: no son independientes).
    pub n: usize,
    /// Volumen maximo del golpe, en dBFS.
    pub peak_db: f32,
    /// Cuanto sobresalio el golpe sobre el ruido de fondo, en dB.
    pub over_floor_db: f32,
    /// Cuando fue el golpe.
    pub at: Instant,
}

struct Active {
    onset: Instant,
    peak_db: f32,
    floor_db: f32,
    readings: Vec<f32>,
}

/// Detecta golpes por volumen y acumula las lecturas de cada uno.
pub struct StrikeTracker {
    recent: VecDeque<(Instant, f32)>,
    floor_db: Option<f32>,
    last_onset: Option<Instant>,
    active: Option<Active>,
}

impl Default for StrikeTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl StrikeTracker {
    pub fn new() -> Self {
        Self {
            recent: VecDeque::new(),
            floor_db: None,
            last_onset: None,
            active: None,
        }
    }

    /// Nuevo nivel de volumen. Devuelve el golpe anterior si este nivel lo
    /// cerro (se apago o empezo otro golpe).
    pub fn observe_level(&mut self, now: Instant, db: f32) -> Option<StrikeResult> {
        // Ruido de fondo: baja enseguida, sube muy despacio.
        let floor = self.floor_db.get_or_insert(db);
        if db < *floor {
            *floor = db;
        } else {
            *floor += (db - *floor) * 0.002;
        }
        let floor = *floor;

        while self
            .recent
            .front()
            .is_some_and(|(t, _)| now.duration_since(*t) > ONSET_LOOKBACK)
        {
            self.recent.pop_front();
        }
        // Solo desde el ultimo golpe: si no, mientras una nota se apaga el
        // silencio de antes del golpe la haria "golpear" de nuevo.
        let recent_min = self
            .recent
            .iter()
            .filter(|(t, _)| self.last_onset.is_none_or(|o| *t >= o))
            .map(|(_, d)| *d)
            .fold(f32::INFINITY, f32::min);
        self.recent.push_back((now, db));

        let gap_ok = self
            .last_onset
            .is_none_or(|t| now.duration_since(t) >= MIN_GAP);
        let onset = recent_min.is_finite()
            && db >= recent_min + ONSET_RISE_DB
            && db >= floor + ONSET_OVER_FLOOR_DB
            && gap_ok;

        if onset {
            let done = self.finish();
            self.last_onset = Some(now);
            self.active = Some(Active {
                onset: now,
                peak_db: db,
                floor_db: floor,
                readings: Vec::new(),
            });
            return done;
        }

        let ended = match self.active.as_mut() {
            Some(a) => {
                a.peak_db = a.peak_db.max(db);
                db < a.peak_db - END_DROP_DB || now.duration_since(a.onset) > MAX_STRIKE
            }
            None => false,
        };
        if ended {
            self.finish()
        } else {
            None
        }
    }

    /// Una lectura de altura (frecuencia cruda, sin suavizar).
    pub fn observe_pitch(&mut self, now: Instant, hz: f32) {
        if let Some(a) = self.active.as_mut() {
            if now.duration_since(a.onset) >= SKIP_ATTACK && hz.is_finite() && hz > 0.0 {
                a.readings.push(hz);
            }
        }
    }

    /// Cerrar el golpe en curso (silencio). Devuelve su resultado.
    pub fn finish(&mut self) -> Option<StrikeResult> {
        let a = self.active.take()?;
        let mut v = a.readings;
        v.sort_by(f32::total_cmp);
        let n = v.len();
        let (median_hz, iqr_cents) = if n == 0 {
            (0.0, 0.0)
        } else {
            let q = |p: f32| v[((n - 1) as f32 * p).round() as usize];
            (q(0.5), 1200.0 * (q(0.75) / q(0.25)).log2())
        };
        Some(StrikeResult {
            median_hz,
            iqr_cents,
            n,
            peak_db: a.peak_db,
            over_floor_db: a.peak_db - a.floor_db,
            at: a.onset,
        })
    }

    /// Descartar el golpe en curso (cambio de nota).
    pub fn reset(&mut self) {
        self.active = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Simula un golpe: silencio, ataque y decaimiento, a 100 lecturas/s.
    /// `freq_at(ms)` da la frecuencia leida en cada momento.
    fn strike(
        t: &mut StrikeTracker,
        start: Instant,
        offset_ms: u64,
        peak_db: f32,
        decay_db_per_s: f32,
        freq_at: impl Fn(u64) -> f32,
    ) -> Vec<StrikeResult> {
        let mut out = Vec::new();
        for ms in (0..1200).step_by(10) {
            let now = start + Duration::from_millis(offset_ms + ms);
            let db = (peak_db - decay_db_per_s * ms as f32 / 1000.0).max(-70.0);
            out.extend(t.observe_level(now, db));
            if db > -60.0 {
                t.observe_pitch(now, freq_at(ms));
            }
        }
        out
    }

    fn quiet(t: &mut StrikeTracker, start: Instant, from_ms: u64, to_ms: u64) -> Vec<StrikeResult> {
        (from_ms..to_ms)
            .step_by(10)
            .filter_map(|ms| t.observe_level(start + Duration::from_millis(ms), -70.0))
            .collect()
    }

    #[test]
    fn short_treble_strike_gives_one_frozen_result() {
        let mut t = StrikeTracker::new();
        let s = Instant::now();
        quiet(&mut t, s, 0, 300);
        // C7 que se apaga en ~0,6 s; el ataque lee cualquier cosa.
        let r = strike(&mut t, s, 300, -20.0, 60.0, |ms| {
            if ms < 50 {
                3000.0
            } else {
                2093.0 * 2f32.powf(3.0 / 1200.0)
            }
        });
        assert_eq!(r.len(), 1, "{r:?}");
        let r = r[0];
        let cents = 1200.0 * (r.median_hz / 2093.0).log2();
        assert!((cents - 3.0).abs() < 0.1, "{cents}");
        assert!(r.iqr_cents < 0.5);
        assert!(r.n >= 20, "{}", r.n);
    }

    #[test]
    fn two_strikes_in_a_row_are_kept_apart() {
        let mut t = StrikeTracker::new();
        let s = Instant::now();
        quiet(&mut t, s, 0, 300);
        let mut all = strike(&mut t, s, 300, -20.0, 20.0, |_| 4186.0);
        // Segundo golpe antes de que se apague el primero, con otra altura.
        all.extend(quiet(&mut t, s, 1500, 1510));
        all.extend(strike(&mut t, s, 1510, -10.0, 60.0, |_| 4200.0));
        all.extend(t.finish());
        assert_eq!(all.len(), 2, "{all:?}");
        assert!((all[0].median_hz - 4186.0).abs() < 0.5);
        assert!((all[1].median_hz - 4200.0).abs() < 0.5);
    }

    #[test]
    fn quiet_strike_over_a_quiet_room_is_detected() {
        let mut t = StrikeTracker::new();
        let s = Instant::now();
        quiet(&mut t, s, 0, 500);
        // Golpe debil: -50 dBFS sobre un fondo de -70.
        let r = strike(&mut t, s, 500, -50.0, 40.0, |_| 2093.0);
        let r: Vec<_> = r.into_iter().chain(t.finish()).collect();
        assert_eq!(r.len(), 1);
        assert!(r[0].n > 0);
    }

    #[test]
    fn steady_sound_is_not_a_strike() {
        let mut t = StrikeTracker::new();
        let s = Instant::now();
        for ms in (0..2000).step_by(10) {
            assert!(t
                .observe_level(s + Duration::from_millis(ms), -30.0)
                .is_none());
        }
        assert!(t.finish().is_none());
    }

    #[test]
    fn beating_strike_shows_a_wide_spread() {
        let mut t = StrikeTracker::new();
        let s = Instant::now();
        quiet(&mut t, s, 0, 300);
        let r = strike(&mut t, s, 300, -20.0, 40.0, |ms| {
            2093.0 * 2f32.powf(8.0 * ((ms as f32) / 80.0).sin() / 1200.0)
        });
        let r: Vec<_> = r.into_iter().chain(t.finish()).collect();
        assert!(r[0].iqr_cents > 4.0, "{}", r[0].iqr_cents);
    }
}
