use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub font: FontConfig,
    pub appearance: AppearanceConfig,
    pub terminal: TerminalConfig,
    pub keybindings: KeybindingsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FontConfig {
    pub family: String,
    pub size: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppearanceConfig {
    pub theme: ThemeMode,
    pub unfocused_pane_opacity: f32,
    pub sidebar_width: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ThemeMode {
    Dark,
    Light,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TerminalConfig {
    pub scrollback_limit: usize,
    pub shell: Option<String>,
    pub working_directory: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct KeybindingsConfig {
    pub new_workspace: String,
    pub close_workspace: String,
    pub next_workspace: String,
    pub prev_workspace: String,
    pub split_right: String,
    pub split_down: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            font: FontConfig::default(),
            appearance: AppearanceConfig::default(),
            terminal: TerminalConfig::default(),
            keybindings: KeybindingsConfig::default(),
        }
    }
}

impl Default for FontConfig {
    fn default() -> Self {
        Self {
            family: "Cascadia Mono".into(),
            size: 16.0,
        }
    }
}

impl Default for AppearanceConfig {
    fn default() -> Self {
        Self {
            theme: ThemeMode::Dark,
            unfocused_pane_opacity: 0.7,
            sidebar_width: 220.0,
        }
    }
}

impl Default for ThemeMode {
    fn default() -> Self {
        Self::Dark
    }
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            scrollback_limit: 10_000,
            shell: None,
            working_directory: None,
        }
    }
}

impl Default for KeybindingsConfig {
    fn default() -> Self {
        Self {
            new_workspace: "Ctrl+Shift+N".into(),
            close_workspace: "Ctrl+Shift+W".into(),
            next_workspace: "Ctrl+Tab".into(),
            prev_workspace: "Ctrl+Shift+Tab".into(),
            split_right: "Ctrl+Shift+D".into(),
            split_down: "Ctrl+Shift+E".into(),
        }
    }
}

impl Config {
    pub fn config_dir() -> PathBuf {
        dirs().join("vibemux")
    }

    pub fn config_path() -> PathBuf {
        Self::config_dir().join("config.toml")
    }

    pub fn load() -> anyhow::Result<Self> {
        let path = Self::config_path();
        if path.exists() {
            let contents = std::fs::read_to_string(&path)?;
            Ok(toml::from_str(&contents)?)
        } else {
            Ok(Self::default())
        }
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let dir = Self::config_dir();
        std::fs::create_dir_all(&dir)?;
        let contents = toml::to_string_pretty(self)?;
        std::fs::write(Self::config_path(), contents)?;
        Ok(())
    }
}

fn dirs() -> PathBuf {
    std::env::var("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs_next::config_dir().unwrap_or_else(|| PathBuf::from("."))
        })
}
