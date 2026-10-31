//! Ajustes del usuario, guardados en %APPDATA%\pianoTunerFree\settings.json.

use std::path::PathBuf;

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    /// Frecuencia del La central (A4) en Hz.
    pub a4: f32,
    /// Primera nota (MIDI) con 2 cuerdas.
    pub bichord_from: u8,
    /// Primera nota (MIDI) con 3 cuerdas.
    pub trichord_from: u8,
    /// Margen en cents para dar una nota por afinada.
    pub tolerance: f32,
    /// Aplicar el estiramiento de Railsback (agudos un poco mas altos,
    /// graves un poco mas bajos), como hacen los afinadores profesionales.
    pub stretch: bool,
    /// Segundos que se mantiene en pantalla la ultima lectura despues de
    /// que la nota se apaga (ayuda con los agudos que duran poco).
    pub hold_secs: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            a4: 440.0,
            bichord_from: 35,  // B1
            trichord_from: 57, // A3
            tolerance: 5.0,
            stretch: true,
            hold_secs: 3.0,
        }
    }
}

impl Settings {
    fn path() -> Option<PathBuf> {
        ProjectDirs::from("", "", "pianoTunerFree").map(|d| d.config_dir().join("settings.json"))
    }

    pub fn load() -> Self {
        Self::path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
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

    /// Aplica la distribucion de cuerdas al motor.
    pub fn apply_string_layout(&self) {
        crate::tuning::set_string_layout(self.bichord_from, self.trichord_from);
    }
}
