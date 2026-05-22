use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Config {
    pub bar: BarConfig,
    pub modules: ModulesConfig,
    pub style: StyleConfig,
    pub log_level: String,
}

#[derive(Debug, Clone)]
pub struct BarConfig {
    pub position: String,    // "top" | "bottom"
    pub height: u32,
    pub multi_monitor: bool, // if true: one bar per monitor; else primary only
    pub click_through: bool, // WS_EX_TRANSPARENT
}

#[derive(Debug, Clone, Default)]
pub struct ModulesConfig {
    pub left: Vec<ModuleEntry>,
    pub center: Vec<ModuleEntry>,
    pub right: Vec<ModuleEntry>,
}

#[derive(Debug, Clone)]
pub struct ModuleEntry {
    pub kind: String,    // "workspaces" | "clock" | "cpu" | etc.
    pub format: String,
    pub interval_ms: u64,
    pub on_click: Option<String>,
    pub on_scroll_up: Option<String>,
    pub on_scroll_down: Option<String>,
    pub extra: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct StyleConfig {
    pub background: String,        // "#1e1e1eee"
    pub foreground: String,        // "#ffffff"
    pub accent: String,            // "#7fc8ff"
    pub font_family: String,       // "Segoe UI"
    pub font_size_pt: f32,
    pub padding_px: u32,
    pub module_spacing_px: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            bar: BarConfig::default(),
            modules: ModulesConfig::default(),
            style: StyleConfig::default(),
            log_level: "info".to_string(),
        }
    }
}

impl Default for BarConfig {
    fn default() -> Self {
        Self {
            position: "top".to_string(),
            height: 32,
            multi_monitor: false,
            click_through: false,
        }
    }
}

impl Default for ModuleEntry {
    fn default() -> Self {
        Self {
            kind: String::new(),
            format: String::new(),
            interval_ms: 1000,
            on_click: None,
            on_scroll_up: None,
            on_scroll_down: None,
            extra: HashMap::new(),
        }
    }
}

impl Default for StyleConfig {
    fn default() -> Self {
        Self {
            background: "#1e1e1eee".to_string(),
            foreground: "#ffffff".to_string(),
            accent: "#7fc8ff".to_string(),
            font_family: "Segoe UI".to_string(),
            font_size_pt: 10.0,
            padding_px: 8,
            module_spacing_px: 12,
        }
    }
}
