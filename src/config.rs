use serde::Deserialize;
use std::{collections::HashMap, fs, path::PathBuf};
use ratatui::style::{Color, Style};

#[derive(Deserialize, Clone)]
pub struct Config {
    pub library: LibraryConfig,
    pub audio: AudioConfig,
    pub ui: UiConfig,
}

#[derive(Deserialize, Clone)]
pub struct LibraryConfig {
    pub music_dir: String,
    pub index_path: String,
}

#[derive(Deserialize, Clone)]
pub struct AudioConfig {
    pub device: String,
}

#[derive(Deserialize, Clone)]
pub struct UiConfig {
    pub fps: u32,
}

impl Config {
    pub fn load() -> Self {
        let home = std::env::var("HOME").expect("home is boom!");
        let config_path = PathBuf::from(home).join(".config/LiPlayerPro/config.toml");
        
        let context = fs::read_to_string(config_path).expect("No found config.toml");

        let mut config: Self = toml::from_str(&context).expect("config format error! 配置文件格式不对喵");
        
        let expanded_index = shellexpand::tilde(&config.library.index_path).to_string();
        config.library.index_path = expanded_index;

        let expanded_music = shellexpand::tilde(&config.library.music_dir).to_string();
        config.library.music_dir = expanded_music;

        config
    }
}

#[derive(serde::Deserialize, Clone, Default)]
pub struct Theme {
    pub colors: HashMap<String, String>,
}

pub struct StyleEngine {
    pub theme: Theme,
}

impl StyleEngine {
    pub fn new() -> Self {
        Self::reload().unwrap_or_else(|_| {
            Self { theme: Theme::default() }
        })
    }

    pub fn reload() -> Result<Self, Box<dyn std::error::Error>> {
        let home = std::env::var("HOME")?;
        let theme_path = std::path::PathBuf::from(home).join(".config/LiPlayerPro/theme.toml");
        if !theme_path.exists() {
            return Ok(Self { theme: Theme::default() });
        }
        let theme_str = fs::read_to_string(theme_path)?;
        let theme: Theme = toml::from_str(&theme_str)?;
        Ok(Self { theme })
    }

    pub fn get(&self, key: &str) -> Style {
        match self.theme.colors.get(key) {
            Some(hex) if hex.len() == 7 && hex.starts_with('#') => {
                let r = u8::from_str_radix(&hex[1..3], 16).unwrap_or(255);
                let g = u8::from_str_radix(&hex[3..5], 16).unwrap_or(255);
                let b = u8::from_str_radix(&hex[5..7], 16).unwrap_or(255);
                Style::default().fg(Color::Rgb(r, g, b))
            }
            _ => Style::default(),
        }
    }
}
