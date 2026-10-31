//! Deteccion de batido para afinar unisonos.
//!
//! Dos cuerdas de la misma nota levemente desafinadas suenan con un volumen
//! que sube y baja ("uaua"). La velocidad de esa oscilacion es la diferencia
//! de frecuencia entre las cuerdas. Aca se analiza la curva de volumen desde
//! el ultimo golpe: se le saca la caida natural del sonido y se busca una
//! oscilacion regular.

use std::time::Instant;

/// Resultado del analisis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BeatInfo {
    /// Batidos por segundo, si hay una oscilacion regular.
    pub rate_hz: Option<f32>,
    /// Cuanto sube y baja el volumen (dB, valor RMS del residuo).
    pub depth_db: f32,
    /// No hay batido apreciable.
    pub clean: bool,
    /// Segundos analizados desde el golpe.
    pub analyzed_secs: f32,
}

/// Paso de remuestreo de la curva (s).
const DT: f32 = 0.01;
/// Ignorar el ataque del golpe (s).
const SKIP_ATTACK: f32 = 0.25;
/// Minimo de sonido analizado para opinar (s).
const MIN_SPAN: f32 = 1.0;
/// Maximo analizado (s).
const MAX_SPAN: f32 = 3.0;
/// Por debajo de esto el sonido se considera estable.
const CLEAN_DEPTH_DB: f32 = 0.6;
/// Volumen minimo para analizar (RMS lineal).
const FLOOR: f32 = 2e-4;

/// Analiza la curva `(tiempo, rms)` desde el ultimo golpe.
pub fn analyze(envelope: &[(Instant, f32)]) -> Option<BeatInfo> {
    if envelope.len() < 20 {
        return None;
    }
    let t0 = envelope[0].0;
    let pts: Vec<(f32, f32)> = envelope
        .iter()
        .map(|(t, r)| (t.duration_since(t0).as_secs_f32(), *r))
        .collect();
    analyze_points(&pts)
}

/// Igual que [`analyze`] pero con tiempos en segundos (para tests).
pub fn analyze_points(pts: &[(f32, f32)]) -> Option<BeatInfo> {
    let onset = last_onset(pts)?;
    let start_t = pts[onset].0 + SKIP_ATTACK;
    let end_t = pts.last()?.0;
    let start_t = start_t.max(end_t - MAX_SPAN);
    if end_t - start_t < MIN_SPAN {
        return None;
    }

    // Remuestrear en dB a paso fijo, cortando cuando el sonido se apaga.
    let mut series = Vec::new();
    let mut t = start_t;
    let mut i = onset;
    while t <= end_t {
        while i + 1 < pts.len() && pts[i + 1].0 < t {
            i += 1;
        }
        if i + 1 >= pts.len() {
            break;
        }
        let (ta, ra) = pts[i];
        let (tb, rb) = pts[i + 1];
        let f = if tb > ta {
            ((t - ta) / (tb - ta)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let r = ra + (rb - ra) * f;
        if r < FLOOR {
            break;
        }
        series.push(20.0 * r.log10());
        t += DT;
    }
    let span = series.len() as f32 * DT;
    if span < MIN_SPAN {
        return None;
    }

    let resid = detrend_quadratic(&series);
    let depth = (resid.iter().map(|x| x * x).sum::<f32>() / resid.len() as f32).sqrt();

    // Autocorrelacion para encontrar el periodo del batido.
    let energy: f32 = resid.iter().map(|x| x * x).sum();
    let mut rate = None;
    if energy > 0.0 {
        let min_lag = (0.06 / DT) as usize; // hasta ~16 batidos/s
        let max_lag = ((span / 2.0).min(1.5) / DT) as usize; // desde ~0.7/s
        let corr = |lag: usize| -> f32 {
            let n = resid.len() - lag;
            let s: f32 = (0..n).map(|k| resid[k] * resid[k + lag]).sum();
            s / energy * resid.len() as f32 / n as f32
        };
        // Picos locales; el batido es el primer pico casi tan alto como el
        // mejor (los multiplos del periodo tambien dan picos).
        let mut peaks = Vec::new();
        let mut prev = corr(min_lag.saturating_sub(1).max(1));
        let mut cur = corr(min_lag);
        for lag in min_lag..max_lag.max(min_lag + 1) {
            let next = corr(lag + 1);
            if cur > prev && cur >= next {
                peaks.push((lag, cur));
            }
            prev = cur;
            cur = next;
        }
        let top = peaks.iter().map(|p| p.1).fold(0.0f32, f32::max);
        if let Some(&(lag, _)) = peaks.iter().find(|p| p.1 > 0.35 && p.1 >= 0.85 * top) {
            rate = Some(1.0 / (lag as f32 * DT));
        }
    }

    Some(BeatInfo {
        rate_hz: rate,
        depth_db: depth,
        clean: depth < CLEAN_DEPTH_DB,
        analyzed_secs: span,
    })
}

/// Indice del ultimo golpe: el volumen se duplica respecto de ~50 ms antes.
fn last_onset(pts: &[(f32, f32)]) -> Option<usize> {
    let mut onset = None;
    let mut j = 0;
    for i in 0..pts.len() {
        while pts[i].0 - pts[j].0 > 0.05 {
            j += 1;
        }
        let before = pts[j].1;
        if pts[i].1 > FLOOR * 4.0 && pts[i].1 > before * 3.0 {
            // tomar el comienzo de esta subida
            if onset.is_none_or(|o: usize| pts[i].0 - pts[o].0 > 0.3) {
                onset = Some(i);
            }
        }
    }
    onset
}

/// Resta la mejor parabola (la caida natural del sonido en dB).
fn detrend_quadratic(y: &[f32]) -> Vec<f32> {
    let n = y.len() as f64;
    let xs: Vec<f64> = (0..y.len()).map(|i| i as f64 / n).collect();
    let (s0, mut s1, mut s2, mut s3, mut s4) = (n, 0.0, 0.0, 0.0, 0.0);
    let (mut t0, mut t1, mut t2) = (0.0, 0.0, 0.0);
    for (x, &v) in xs.iter().zip(y) {
        let v = v as f64;
        s1 += x;
        s2 += x * x;
        s3 += x * x * x;
        s4 += x * x * x * x;
        t0 += v;
        t1 += x * v;
        t2 += x * x * v;
    }
    // Resolver [s0 s1 s2; s1 s2 s3; s2 s3 s4] c = [t0 t1 t2]
    let m = [[s0, s1, s2, t0], [s1, s2, s3, t1], [s2, s3, s4, t2]];
    let Some(c) = solve3(m) else {
        let mean = t0 / s0;
        return y.iter().map(|v| (*v as f64 - mean) as f32).collect();
    };
    xs.iter()
        .zip(y)
        .map(|(x, v)| (*v as f64 - (c[0] + c[1] * x + c[2] * x * x)) as f32)
        .collect()
}

fn solve3(mut m: [[f64; 4]; 3]) -> Option<[f64; 3]> {
    for col in 0..3 {
        let piv = (col..3).max_by(|a, b| m[*a][col].abs().total_cmp(&m[*b][col].abs()))?;
        if m[piv][col].abs() < 1e-12 {
            return None;
        }
        m.swap(col, piv);
        for row in 0..3 {
            if row != col {
                let f = m[row][col] / m[col][col];
                for k in col..4 {
                    m[row][k] -= f * m[col][k];
                }
            }
        }
    }
    Some([m[0][3] / m[0][0], m[1][3] / m[1][1], m[2][3] / m[2][2]])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Curva de volumen de un golpe que decae, con batido opcional.
    fn strike(beat_hz: f32, depth: f32, secs: f32) -> Vec<(f32, f32)> {
        let mut v = Vec::new();
        // silencio previo
        for i in 0..30 {
            v.push((i as f32 * 0.01, 1e-5));
        }
        let start = 0.3;
        let mut t = 0.0;
        while t < secs {
            let decay = (-t / 1.5).exp();
            let beat = 1.0 + depth * (2.0 * std::f32::consts::PI * beat_hz * t).cos();
            v.push((start + t, 0.2 * decay * beat.max(0.05)));
            t += 0.01;
        }
        v
    }

    #[test]
    fn detects_beat_rate() {
        for hz in [1.0f32, 2.0, 4.0, 7.0] {
            let info = analyze_points(&strike(hz, 0.4, 3.0)).expect("analysis");
            let rate = info.rate_hz.expect("rate");
            assert!((rate - hz).abs() / hz < 0.15, "expected {hz}, got {rate}");
            assert!(!info.clean);
        }
    }

    #[test]
    fn clean_unison_is_clean() {
        let info = analyze_points(&strike(0.0, 0.0, 3.0)).expect("analysis");
        assert!(info.clean, "{info:?}");
    }

    #[test]
    fn too_short_returns_none() {
        assert!(analyze_points(&strike(2.0, 0.4, 0.8)).is_none());
    }
}
