//! Persistent TUI settings.

use std::fs;
use std::path::PathBuf;

const HEADER: &str = "tarot-tui-settings-v1";

#[derive(Debug, Clone)]
pub struct Settings {
    pub auto_poll: bool,
    pub auto_save: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            auto_poll: true,
            auto_save: false,
        }
    }
}

pub fn load() -> Settings {
    let Ok(text) = fs::read_to_string(path()) else {
        return Settings::default();
    };
    let mut settings = Settings::default();
    for line in text.lines() {
        if line == HEADER || line.trim().is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key {
            "auto_poll" => settings.auto_poll = parse_bool(value, settings.auto_poll),
            "auto_save" => settings.auto_save = parse_bool(value, settings.auto_save),
            _ => {}
        }
    }
    settings
}

pub fn save(settings: &Settings) -> std::io::Result<()> {
    let file = path();
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(
        file,
        format!(
            "{HEADER}\nauto_poll={}\nauto_save={}\n",
            settings.auto_poll, settings.auto_save
        ),
    )
}

fn parse_bool(value: &str, default: bool) -> bool {
    match value.trim() {
        "true" | "1" | "yes" | "on" => true,
        "false" | "0" | "no" | "off" => false,
        _ => default,
    }
}

fn path() -> PathBuf {
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".config/tarot/tui-settings.conf");
    }
    PathBuf::from("tarot-tui-settings.conf")
}
