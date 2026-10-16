use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;

use color_eyre::eyre::{Result, WrapErr, eyre};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct WindowsSettings {
    pub schema_version: u32,
    pub dictation_hotkey: WindowsHotkey,
    pub double_tap_lock: bool,
    pub sound_effect_volume: f32,
    pub transcription: crate::transcription_models::TranscriptionSelection,
    /// Opt-in hosted transcription; off by default.
    pub cloud_transcription: crate::cloud_transcription::CloudTranscriptionSettings,
}

/// A key-containing chord. The hook requires exactly these modifiers, so
/// `Ctrl+Alt+Space` does not also trigger an `Alt+Space` binding.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
pub struct WindowsHotkey {
    pub control: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
    pub key: String,
}

impl Default for WindowsSettings {
    fn default() -> Self {
        Self {
            schema_version: 1,
            dictation_hotkey: WindowsHotkey::default(),
            double_tap_lock: true,
            sound_effect_volume: 0.5,
            transcription: crate::transcription_models::TranscriptionSelection::default(),
            cloud_transcription: Default::default(),
        }
    }
}

impl Default for WindowsHotkey {
    fn default() -> Self {
        Self {
            control: false,
            alt: true,
            shift: false,
            win: false,
            key: "space".into(),
        }
    }
}

impl WindowsSettings {
    pub fn path() -> Result<PathBuf> {
        Ok(crate::app_paths::support_dir()?.join("windows-settings.json"))
    }

    pub fn load() -> Result<Self> {
        let path = Self::path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let settings: Self = serde_json::from_slice(&fs::read(&path)?)
            .wrap_err_with(|| format!("could not parse {}", path.display()))?;
        settings.validate()?;
        Ok(settings)
    }

    fn validate(&self) -> Result<()> {
        self.dictation_hotkey.virtual_key()?;
        crate::transcription_models::validate(&self.transcription)?;
        self.cloud_transcription.validate()?;
        if !(0.0..=1.0).contains(&self.sound_effect_volume) {
            return Err(eyre!("Sound volume must be between zero and one"));
        }
        Ok(())
    }

    pub fn save(&self) -> Result<()> {
        self.validate()?;
        let path = Self::path()?;
        fs::create_dir_all(path.parent().unwrap())?;
        let partial = path.with_extension("json.partial");
        let mut file = File::create(&partial)?;
        serde_json::to_writer_pretty(&mut file, self)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        // `rename` replaces an existing destination atomically on NTFS.
        fs::rename(partial, &path)?;
        Ok(())
    }
}

impl WindowsHotkey {
    pub fn label(&self) -> String {
        let mut parts = Vec::new();
        for (enabled, name) in [
            (self.control, "Ctrl"),
            (self.alt, "Alt"),
            (self.shift, "Shift"),
            (self.win, "Win"),
        ] {
            if enabled {
                parts.push(name.to_string());
            }
        }
        parts.push(if self.key == "space" {
            "Space".into()
        } else {
            self.key.to_ascii_uppercase()
        });
        parts.join("+")
    }

    /// The Win32 virtual-key code of the non-modifier trigger key. Letters and
    /// digits name the key at that position on the active layout's VK map.
    pub fn virtual_key(&self) -> Result<u32> {
        let key = self.key.trim().to_ascii_lowercase();
        match key.as_str() {
            "" => Err(eyre!("Windows hotkeys require a non-modifier key")),
            "space" => Ok(0x20),
            "enter" | "return" => Ok(0x0d),
            "tab" => Ok(0x09),
            "backspace" => Ok(0x08),
            "insert" => Ok(0x2d),
            "pause" => Ok(0x13),
            "scrolllock" => Ok(0x91),
            "capslock" => Ok(0x14),
            key if key.len() == 1 && key.as_bytes()[0].is_ascii_alphanumeric() => {
                Ok(u32::from(key.as_bytes()[0].to_ascii_uppercase()))
            }
            key if key.starts_with('f') => key[1..]
                .parse::<u32>()
                .ok()
                .filter(|number| (1..=24).contains(number))
                // VK_F1 is 0x70 and VK_F1..VK_F24 are contiguous.
                .map(|number| 0x6f + number)
                .ok_or_else(|| eyre!("unsupported Windows function key: {key}")),
            _ => Err(eyre!("unsupported Windows hotkey key: {key}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(key: &str) -> WindowsHotkey {
        WindowsHotkey {
            alt: false,
            key: key.into(),
            ..WindowsHotkey::default()
        }
    }

    #[test]
    fn default_binding_is_alt_space() {
        let hotkey = WindowsSettings::default().dictation_hotkey;
        assert_eq!(hotkey.label(), "Alt+Space");
        assert_eq!(hotkey.virtual_key().unwrap(), 0x20);
    }

    #[test]
    fn keys_map_to_virtual_key_codes() {
        assert_eq!(key("f1").virtual_key().unwrap(), 0x70);
        assert_eq!(key("F24").virtual_key().unwrap(), 0x87);
        assert_eq!(key("d").virtual_key().unwrap(), u32::from(b'D'));
        assert_eq!(key("7").virtual_key().unwrap(), u32::from(b'7'));
        assert_eq!(key("f12").label(), "F12");
        for invalid in ["", "f0", "f25", "ctrl", "é"] {
            assert!(key(invalid).virtual_key().is_err(), "{invalid}");
        }
    }

    #[test]
    fn partial_settings_keep_defaults_and_round_trip() {
        let settings: WindowsSettings =
            serde_json::from_str(r#"{"double_tap_lock":false}"#).unwrap();
        assert!(!settings.double_tap_lock);
        assert_eq!(settings.dictation_hotkey, WindowsHotkey::default());
        assert_eq!(settings.sound_effect_volume, 0.5);
        let saved = serde_json::to_vec(&settings).unwrap();
        assert_eq!(
            serde_json::from_slice::<WindowsSettings>(&saved).unwrap(),
            settings
        );
    }

    #[test]
    fn invalid_sound_volume_is_rejected() {
        for volume in [-0.1, 1.1, f32::NAN] {
            let settings = WindowsSettings {
                sound_effect_volume: volume,
                ..Default::default()
            };
            assert!(settings.validate().is_err());
        }
    }
}
