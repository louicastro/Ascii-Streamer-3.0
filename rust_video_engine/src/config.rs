use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorMode {
    Monochrome,
    TrueColor,
    Grayscale,
    Ansi16,
    Ansi256,
}

impl ColorMode {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "truecolor" | "rgb" => ColorMode::TrueColor,
            "grayscale" | "gray" => ColorMode::Grayscale,
            "ansi16" => ColorMode::Ansi16,
            "ansi256" => ColorMode::Ansi256,
            _ => ColorMode::Monochrome,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            ColorMode::Monochrome => "monochrome",
            ColorMode::TrueColor => "truecolor",
            ColorMode::Grayscale => "grayscale",
            ColorMode::Ansi16 => "ansi16",
            ColorMode::Ansi256 => "ansi256",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderMode {
    Normal,
    NegativeSpace,
    InvertedAscii,
    Hybrid,
}

impl RenderMode {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "negative_space" | "negative" => RenderMode::NegativeSpace,
            "inverted_ascii" | "inverted" => RenderMode::InvertedAscii,
            "hybrid" => RenderMode::Hybrid,
            _ => RenderMode::Normal,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            RenderMode::Normal => "normal",
            RenderMode::NegativeSpace => "negative_space",
            RenderMode::InvertedAscii => "inverted_ascii",
            RenderMode::Hybrid => "hybrid",
        }
    }
}

#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub cols: usize,
    pub color_mode: ColorMode,
    pub char_set: String,
    pub invert_chars: bool,

    // Negative Space & Subject Segmentation
    pub render_mode: RenderMode,
    pub neg_space_sensitivity: f32,
    pub neg_space_bg_char_set: String,
    pub neg_space_fg_char_set: String,
    pub neg_space_bg_brightness: f32,

    // AI Structural Perception & Temporal Coherence
    pub enable_edge_detection: bool,
    pub edge_threshold: f32,
    pub edge_weight: f32,
    pub temporal_smoothing: f32,
    pub adaptive_contrast: bool,

    // Image Tuning
    pub brightness: i32, // -100..100
    pub contrast: i32,   // -100..100
    pub gamma: i32,      // -100..100
    pub saturation: i32, // -100..100

    // Aesthetic Phosphor & Styling
    pub phosphor_color: (u8, u8, u8),
    pub bg_color: (u8, u8, u8),
    pub font_size: u32,
}

impl Default for EngineConfig {
    fn default() -> Self {
        EngineConfig {
            cols: 120,
            color_mode: ColorMode::Monochrome,
            char_set: "standard".to_string(),
            invert_chars: false,
            render_mode: RenderMode::Normal,
            neg_space_sensitivity: 0.5,
            neg_space_bg_char_set: "symbols".to_string(),
            neg_space_fg_char_set: "standard".to_string(),
            neg_space_bg_brightness: 1.0,
            enable_edge_detection: true,
            edge_threshold: 0.18,
            edge_weight: 0.65,
            temporal_smoothing: 0.35,
            adaptive_contrast: true,
            brightness: 0,
            contrast: 10,
            gamma: 0,
            saturation: 0,
            phosphor_color: (57, 255, 82), // Matrix green
            bg_color: (0, 0, 0),
            font_size: 12,
        }
    }
}

pub fn get_char_set(name_or_str: &str) -> String {
    match name_or_str {
        "standard" => " .:-=+*#%@".to_string(),
        "extended" => " .'`^\",:;Il!i><~+_-?][}{1)(|\\tfjrxnuvczXYUJCLQ0OZmwqpdbkhao*#MW&8%B@$".to_string(),
        "blocks" => " ░▒▓█".to_string(),
        "minimal" => " .oO0".to_string(),
        "braille" => " ⠁⠂⠃⠄⠈⠊⠋⠌⠐⠓⠔⠘⠛⠜⠠⠨⠰⠸⡀⡈⡐⡘⡠⡨⡰⢀⢁⢂⢄⢈⢐⢠⣀⣤⣶".to_string(),
        "matrix" => " .:=+*#MW&8%B@$".to_string(),
        "letters" => " abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ".to_string(),
        "numbers" => " 0123456789".to_string(),
        "symbols" => " .:+*#%$@&~=^/\\|!?[]{}()<>-".to_string(),
        "alphanumeric" => " 0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ.:+*#%@".to_string(),
        "binary" => " 01".to_string(),
        custom if !custom.is_empty() => custom.to_string(),
        _ => " .:-=+*#%@".to_string(),
    }
}

pub fn get_preset(name: &str) -> EngineConfig {
    match name {
        "matrix" => EngineConfig {
            cols: 140,
            color_mode: ColorMode::Monochrome,
            char_set: "extended".to_string(),
            enable_edge_detection: true,
            edge_threshold: 0.15,
            temporal_smoothing: 0.40,
            phosphor_color: (57, 255, 82),
            contrast: 15,
            brightness: 5,
            ..EngineConfig::default()
        },
        "cyberpunk" => EngineConfig {
            cols: 120,
            color_mode: ColorMode::TrueColor,
            char_set: "extended".to_string(),
            enable_edge_detection: true,
            edge_threshold: 0.18,
            temporal_smoothing: 0.30,
            saturation: 25,
            contrast: 15,
            ..EngineConfig::default()
        },
        "edge_art" => EngineConfig {
            cols: 160,
            color_mode: ColorMode::Monochrome,
            char_set: "extended".to_string(),
            enable_edge_detection: true,
            edge_threshold: 0.10,
            edge_weight: 0.90,
            temporal_smoothing: 0.25,
            phosphor_color: (0, 245, 255), // Cyan
            contrast: 20,
            ..EngineConfig::default()
        },
        "truecolor_cinema" => EngineConfig {
            cols: 100,
            color_mode: ColorMode::TrueColor,
            char_set: "standard".to_string(),
            enable_edge_detection: false,
            temporal_smoothing: 0.20,
            contrast: 5,
            saturation: 10,
            ..EngineConfig::default()
        },
        "retro_amber" => EngineConfig {
            cols: 110,
            color_mode: ColorMode::Monochrome,
            char_set: "standard".to_string(),
            enable_edge_detection: true,
            edge_threshold: 0.20,
            phosphor_color: (255, 187, 0), // Amber
            contrast: 10,
            ..EngineConfig::default()
        },
        "braille_hd" => EngineConfig {
            cols: 160,
            color_mode: ColorMode::Monochrome,
            char_set: "braille".to_string(),
            enable_edge_detection: false,
            temporal_smoothing: 0.25,
            phosphor_color: (240, 240, 240),
            contrast: 20,
            ..EngineConfig::default()
        },
        "negative_space" => EngineConfig {
            cols: 120,
            color_mode: ColorMode::Monochrome,
            render_mode: RenderMode::NegativeSpace,
            char_set: "symbols".to_string(),
            neg_space_bg_char_set: "symbols".to_string(),
            neg_space_fg_char_set: "standard".to_string(),
            neg_space_sensitivity: 0.50,
            neg_space_bg_brightness: 1.20,
            enable_edge_detection: false,
            temporal_smoothing: 0.35,
            phosphor_color: (57, 255, 82),
            contrast: 20,
            ..EngineConfig::default()
        },
        _ => get_preset("matrix"),
    }
}

pub fn get_all_presets() -> BTreeMap<String, EngineConfig> {
    let mut map = BTreeMap::new();
    for name in &["matrix", "cyberpunk", "edge_art", "truecolor_cinema", "retro_amber", "braille_hd", "negative_space"] {
        map.insert(name.to_string(), get_preset(name));
    }
    map
}

pub static ANSI16: [[u8; 3]; 16] = [
    [0, 0, 0], [128, 0, 0], [0, 128, 0], [128, 128, 0],
    [0, 0, 128], [128, 0, 128], [0, 128, 128], [192, 192, 192],
    [128, 128, 128], [255, 0, 0], [0, 255, 0], [255, 255, 0],
    [0, 0, 255], [255, 0, 255], [0, 255, 255], [255, 255, 255],
];

pub fn get_ansi256_palette() -> Vec<[u8; 3]> {
    let mut table = Vec::with_capacity(256);
    // 0-15
    for c in &ANSI16 {
        table.push(*c);
    }
    // 16-231: 6x6x6 color cube
    let levels = [0, 95, 135, 175, 215, 255];
    for &r in &levels {
        for &g in &levels {
            for &b in &levels {
                table.push([r, g, b]);
            }
        }
    }
    // 232-255: grayscale ramp
    for i in 0..24 {
        let v = (8 + i * 10) as u8;
        table.push([v, v, v]);
    }
    table
}
