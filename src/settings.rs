//! Ajustes del usuario, guardados en %APPDATA%\pianoTunerFree\config\settings.json.

use std::path::PathBuf;

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use crate::tuning::StretchCurve;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum Lang {
    #[default]
    Es,
    En,
}

/// Nivel de ayuda. Cada nivel prende o apaga un grupo de opciones; si el
/// usuario cambia alguna a mano queda en `Custom`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum Level {
    #[default]
    Basic,
    Intermediate,
    Advanced,
    Custom,
}

/// Que estiramiento aplicar a los objetivos.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum StretchSel {
    /// Temperamento igual puro.
    Off,
    /// Curva promedio de Railsback.
    #[default]
    Standard,
    /// Curva medida en este piano (ver "Medir mi piano").
    Measured,
}

/// Datos de un piano.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct PianoConfig {
    pub name: String,
    /// Frecuencia del La central (A4) en Hz.
    pub a4: f32,
    /// Primera nota (MIDI) con 2 cuerdas.
    pub bichord_from: u8,
    /// Primera nota (MIDI) con 3 cuerdas.
    pub trichord_from: u8,
    pub stretch: StretchSel,
    /// Curva medida (88 valores en cents), si se midio el piano.
    pub measured: Option<Vec<f32>>,
    /// Cantidad de teclas: 88, o 85 en muchos pianos viejos (terminan en A7).
    pub keys: u8,
}

impl Default for PianoConfig {
    fn default() -> Self {
        Self {
            name: "Mi piano".into(),
            a4: 440.0,
            bichord_from: 35,  // B1
            trichord_from: 57, // A3
            stretch: StretchSel::Standard,
            measured: None,
            keys: 88,
        }
    }
}

impl PianoConfig {
    /// Nota mas aguda que tiene el piano (MIDI).
    pub fn top_midi(&self) -> u8 {
        (21 + self.keys.clamp(1, 88) - 1).min(108)
    }

    /// La nota existe en este piano.
    pub fn has_key(&self, midi: u8) -> bool {
        (21..=self.top_midi()).contains(&midi)
    }

    /// Curva de estiramiento segun la eleccion.
    pub fn stretch_curve(&self) -> Option<StretchCurve> {
        match self.stretch {
            StretchSel::Off => None,
            StretchSel::Standard => Some(StretchCurve::railsback_default()),
            StretchSel::Measured => match &self.measured {
                Some(v) if v.len() == 88 => {
                    let mut a = [0.0f32; 88];
                    a.copy_from_slice(v);
                    Some(StretchCurve::from_offsets(a))
                }
                _ => Some(StretchCurve::railsback_default()),
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub piano: PianoConfig,
    pub saved_pianos: Vec<PianoConfig>,
    pub lang: Lang,
    pub level: Level,
    /// Mostrar un paso para apagar cuerdas antes de afinar.
    pub remind_mute: bool,
    /// Mostrar explicaciones en cada paso.
    pub show_details: bool,
    /// Avanzar solo cuando la cuerda queda estable en la zona verde.
    pub auto_advance: bool,
    /// Avanzar solo en los unisonos cuando no hay batido.
    pub auto_unison: bool,
    /// Mostrar el medidor de batido en los unisonos.
    pub show_beats: bool,
    /// Mostrar la lupa de -10 a +10 cents.
    pub fine_meter: bool,
    /// Mostrar el estroboscopio.
    pub strobe: bool,
    /// Mostrar intervalos de control con su batido esperado.
    pub show_intervals: bool,
    /// Margen en cents para dar una nota por afinada.
    pub tolerance: f32,
    /// Segundos estable antes de avanzar solo.
    pub auto_secs: f32,
    /// Segundos que se mantiene la ultima lectura (agudos cortos).
    pub hold_secs: f32,
    /// El afinador libre usa el estiramiento del piano.
    pub free_stretch: bool,
    /// Afinar con tira de fieltro en dos pasadas.
    pub strip_mode: bool,
    /// Estado: se esta haciendo la pasada de unisonos.
    pub unison_pass: bool,
    /// Nota de referencia con sonido de piano (si no, tono puro).
    pub ref_piano: bool,
    /// Microfono elegido (None = el predeterminado de Windows).
    pub mic_device: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        let mut s = Self {
            piano: PianoConfig::default(),
            saved_pianos: Vec::new(),
            lang: Lang::Es,
            level: Level::Basic,
            remind_mute: true,
            show_details: true,
            auto_advance: false,
            auto_unison: false,
            show_beats: true,
            fine_meter: true,
            strobe: false,
            show_intervals: false,
            tolerance: 5.0,
            auto_secs: 1.0,
            hold_secs: 3.0,
            ref_piano: true,
            strip_mode: false,
            unison_pass: false,
            free_stretch: true,
            mic_device: None,
        };
        s.apply_level(Level::Basic);
        s
    }
}

impl Settings {
    fn path() -> Option<PathBuf> {
        ProjectDirs::from("", "", "pianoTunerFree").map(|d| d.config_dir().join("settings.json"))
    }

    pub fn load() -> Self {
        let Some(text) = Self::path().and_then(|p| std::fs::read_to_string(p).ok()) else {
            return Self::default();
        };
        let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&text) else {
            return Self::default();
        };
        // Version anterior: los datos del piano estaban sueltos.
        if value.get("piano").is_none() {
            let mut piano = PianoConfig::default();
            if let Some(v) = value.get("a4").and_then(|v| v.as_f64()) {
                piano.a4 = v as f32;
            }
            if let Some(v) = value.get("bichord_from").and_then(|v| v.as_u64()) {
                piano.bichord_from = v as u8;
            }
            if let Some(v) = value.get("trichord_from").and_then(|v| v.as_u64()) {
                piano.trichord_from = v as u8;
            }
            if value.get("stretch").and_then(|v| v.as_bool()) == Some(false) {
                piano.stretch = StretchSel::Off;
            }
            if let (Some(obj), Ok(p)) = (value.as_object_mut(), serde_json::to_value(&piano)) {
                obj.insert("piano".into(), p);
                obj.remove("stretch");
            }
        }
        serde_json::from_value(value).unwrap_or_default()
    }

    pub fn save(&self) {
        if let Some(p) = Self::path() {
            if let Some(dir) = p.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Ok(json) = serde_json::to_string_pretty(self) {
                let _ = std::fs::write(p, json);
            }
        }
    }

    /// Elige el texto en el idioma actual.
    pub fn t(&self, es: &'static str, en: &'static str) -> &'static str {
        match self.lang {
            Lang::Es => es,
            Lang::En => en,
        }
    }

    /// Prende/apaga las opciones de un nivel.
    pub fn apply_level(&mut self, level: Level) {
        self.level = level;
        let (remind, details, auto, unison, strobe, tol) = match level {
            Level::Basic => (true, true, false, false, false, 5.0),
            Level::Intermediate => (false, true, true, false, false, 3.0),
            Level::Advanced => (false, false, true, true, true, 2.0),
            Level::Custom => return,
        };
        self.remind_mute = remind;
        self.show_details = details;
        self.auto_advance = auto;
        self.auto_unison = unison;
        self.show_beats = true;
        self.fine_meter = true;
        self.strobe = strobe;
        self.show_intervals = level == Level::Advanced;
        self.tolerance = tol;
    }

    /// Las opciones que define un nivel (para saber si se toco alguna).
    #[allow(clippy::type_complexity)]
    pub fn assists(&self) -> (bool, bool, bool, bool, bool, bool, bool, bool, f32) {
        (
            self.remind_mute,
            self.show_details,
            self.auto_advance,
            self.auto_unison,
            self.show_beats,
            self.fine_meter,
            self.strobe,
            self.show_intervals,
            self.tolerance,
        )
    }

    /// Aplica la distribucion de cuerdas al motor.
    pub fn apply_string_layout(&self) {
        crate::tuning::set_string_layout(self.piano.bichord_from, self.piano.trichord_from);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_pianos_with_85_keys_end_on_a7() {
        let mut p = PianoConfig::default();
        assert_eq!(p.top_midi(), 108);
        p.keys = 85;
        assert_eq!(p.top_midi(), 105);
        assert!(p.has_key(105));
        assert!(!p.has_key(106));
    }

    #[test]
    fn settings_without_keys_load_as_88() {
        let p: PianoConfig = serde_json::from_str(r#"{"name":"x","a4":417.0}"#).unwrap();
        assert_eq!(p.keys, 88);
    }
}
