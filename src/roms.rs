use crate::state::ParentalConfig;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveredGame {
    /// Stable relative ID (`<system_folder>/<filename>`).
    pub id: String,
    /// Absolute path to the ROM file on disk.
    pub rom_path: String,
    /// Auto-cleaned or parent-customized display title.
    pub display_title: String,
    /// Auto-cleaned default title (before custom override).
    pub default_title: String,
    /// Custom title configured by parent, if any.
    pub custom_title: Option<String>,
    /// Short system badge (e.g., `"GBA"`, `"SNES"`, `"NES"`, `"GBC"`, `"PS1"`).
    pub system_badge: String,
    /// Resolved Libretro core filename (e.g., `"mgba_libretro.so"`).
    pub core_so: String,
    /// Custom core override set by parent, if any.
    pub core_override: Option<String>,
    /// Optional absolute path to discovered `.png` box art image.
    pub box_art_path: Option<String>,
    /// Whether this game is allowlisted by the parent.
    pub allowed: bool,
}

const ROM_EXTENSIONS: &[&str] = &[
    "gba", "gb", "gbc", "nes", "fds", "sfc", "smc", "md", "gen", "smd", "gg", "sms", "pce", "chd",
    "cue", "pbp", "iso", "nds", "p8", "zip", "7z", "n64", "z64",
];

/// Cleans a raw ROM filename stem by stripping region/dump tags like `(USA)`, `(En,Fr,De)`, `[!]`, `[b1]`.
pub fn clean_rom_title(stem: &str) -> String {
    let mut out = String::with_capacity(stem.len());
    let mut paren_depth = 0usize;
    let mut bracket_depth = 0usize;

    for ch in stem.chars() {
        match ch {
            '(' => paren_depth += 1,
            ')' => {
                paren_depth = paren_depth.saturating_sub(1);
            }
            '[' => bracket_depth += 1,
            ']' => {
                bracket_depth = bracket_depth.saturating_sub(1);
            }
            _ if paren_depth == 0 && bracket_depth == 0 => {
                out.push(ch);
            }
            _ => {}
        }
    }

    let collapsed = out
        .replace('_', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    if collapsed.is_empty() {
        stem.trim().to_string()
    } else {
        collapsed
    }
}

/// Infers `(system_badge, default_libretro_core)` from parent folder name, filename stem, and file extension.
pub fn infer_system_and_core(folder_name: &str, stem: &str, ext: &str) -> (String, String) {
    let folder_up = folder_name.to_ascii_uppercase();
    let stem_up = stem.to_ascii_uppercase();
    let ext_low = ext.to_ascii_lowercase();

    if folder_up.contains("GBA") || ext_low == "gba" {
        return ("GBA".to_string(), "mgba_libretro.so".to_string());
    }
    if folder_up.contains("GBC") || ext_low == "gbc" {
        return ("GBC".to_string(), "gambatte_libretro.so".to_string());
    }
    if folder_up == "GB" || folder_up.contains("GAME BOY") || ext_low == "gb" {
        return ("GB".to_string(), "gambatte_libretro.so".to_string());
    }
    if folder_up.contains("SNES")
        || folder_up.contains("SFC")
        || ext_low == "sfc"
        || ext_low == "smc"
    {
        return ("SNES".to_string(), "snes9x_libretro.so".to_string());
    }
    if folder_up.contains("NES")
        || folder_up.contains("FC")
        || ext_low == "nes"
        || ext_low == "fds"
    {
        return ("NES".to_string(), "fceumm_libretro.so".to_string());
    }
    if folder_up.contains("MD")
        || folder_up.contains("GENESIS")
        || folder_up.contains("MEGADRIVE")
        || ext_low == "md"
        || ext_low == "gen"
        || ext_low == "smd"
    {
        return ("MD".to_string(), "genesis_plus_gx_libretro.so".to_string());
    }
    if folder_up.contains("SMS")
        || folder_up.contains("GG")
        || ext_low == "sms"
        || ext_low == "gg"
    {
        return ("SEGA".to_string(), "genesis_plus_gx_libretro.so".to_string());
    }
    if folder_up.contains("PS")
        || folder_up.contains("PLAYSTATION")
        || ext_low == "chd"
        || ext_low == "cue"
        || ext_low == "pbp"
    {
        return ("PS1".to_string(), "pcsx_rearmed_libretro.so".to_string());
    }
    if folder_up.contains("NDS") || ext_low == "nds" {
        return ("NDS".to_string(), "melonds_libretro.so".to_string());
    }
    if folder_up.contains("PICO") || ext_low == "p8" {
        return ("PICO8".to_string(), "fake08_libretro.so".to_string());
    }
    if folder_up.contains("N64") || ext_low == "n64" || ext_low == "z64" {
        return ("N64".to_string(), "mupen64plus_next_libretro.so".to_string());
    }
    if folder_up.contains("PCE") || ext_low == "pce" {
        return (
            "PCE".to_string(),
            "mednafen_pce_fast_libretro.so".to_string(),
        );
    }

    // Heuristics for .7z / .zip archives placed directly in ROMS/ without system subfolders
    if stem_up.contains("GB_COMPATIBLE")
        || stem_up.contains("GB COMPATIBLE")
        || stem_up.contains("CGB")
        || stem_up.contains("SGB")
        || stem_up.contains("CRISTAL")
        || stem_up.contains("CRYSTAL")
        || stem_up.contains("MARIO_LAND")
        || stem_up.contains("MARIO LAND")
    {
        return ("GBC".to_string(), "gambatte_libretro.so".to_string());
    }
    if stem_up.contains("EMERAUDE")
        || stem_up.contains("EMERALD")
        || stem_up.contains("RUBIS")
        || stem_up.contains("SAPHIR")
        || stem_up.contains("GBA")
    {
        return ("GBA".to_string(), "mgba_libretro.so".to_string());
    }

    ("RETRO".to_string(), "fbneo_libretro.so".to_string())
}

/// Discovers box art PNG for a ROM file across standard sidecar and muOS catalogue locations.
pub fn find_box_art(rom_path: &Path, stem: &str, extra_catalogue_roots: &[PathBuf]) -> Option<String> {
    let png_name = format!("{}.png", stem);
    if let Some(rom_dir) = rom_path.parent() {
        let candidates = [
            rom_dir.join("Imgs").join(&png_name),
            rom_dir.join("boxart").join(&png_name),
            rom_dir.join("box").join(&png_name),
            rom_dir.join(&png_name),
        ];
        for c in &candidates {
            if c.is_file() {
                return Some(c.to_string_lossy().to_string());
            }
        }
    }

    let mut cat_roots = vec![PathBuf::from("/mnt/mmc/MUOS/info/catalogue")];
    cat_roots.extend_from_slice(extra_catalogue_roots);

    for cat_root in cat_roots {
        if !cat_root.is_dir() {
            continue;
        }
        if let Ok(entries) = std::fs::read_dir(&cat_root) {
            for entry in entries.flatten() {
                let candidate = entry.path().join("box").join(&png_name);
                if candidate.is_file() {
                    return Some(candidate.to_string_lossy().to_string());
                }
            }
        }
    }

    None
}

/// Scans all configured ROM roots and merges with `ParentalConfig` rules.
pub fn scan_roms(
    rom_roots: &[PathBuf],
    config: &ParentalConfig,
    catalogue_roots: &[PathBuf],
) -> Vec<DiscoveredGame> {
    let mut seen_ids = BTreeSet::new();
    let mut games = Vec::new();

    for root in rom_roots {
        if !root.is_dir() {
            continue;
        }
        for entry in WalkDir::new(root)
            .min_depth(1)
            .max_depth(3)
            .follow_links(true)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            // Skip files inside Imgs / boxart / savestates subdirectories
            if let Some(parent_name) = path
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
            {
                let p_low = parent_name.to_ascii_lowercase();
                if matches!(p_low.as_str(), "imgs" | "boxart" | "box" | "saves" | "states") {
                    continue;
                }
            }

            let ext = match path.extension().and_then(|e| e.to_str()) {
                Some(e) => e.to_ascii_lowercase(),
                None => continue,
            };
            if !ROM_EXTENSIONS.contains(&ext.as_str()) {
                continue;
            }

            let rel_path = path.strip_prefix(root).unwrap_or(path);
            let id = rel_path.to_string_lossy().replace('\\', "/");
            if !seen_ids.insert(id.clone()) {
                continue;
            }

            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Unknown");
            let folder_name = path
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                .unwrap_or("");

            let default_title = clean_rom_title(stem);
            let (system_badge, default_core) = infer_system_and_core(folder_name, stem, &ext);
            let rule = config.games.get(&id);

            let custom_title = rule
                .and_then(|r| r.custom_title.clone())
                .filter(|t| !t.trim().is_empty());
            let display_title = custom_title
                .clone()
                .unwrap_or_else(|| default_title.clone());
            let core_override = rule
                .and_then(|r| r.core_override.clone())
                .filter(|c| !c.trim().is_empty());
            let core_so = core_override.clone().unwrap_or(default_core);
            let allowed = rule.map(|r| r.allowed).unwrap_or(false);
            let box_art_path = find_box_art(path, stem, catalogue_roots);

            games.push(DiscoveredGame {
                id,
                rom_path: path.to_string_lossy().to_string(),
                display_title,
                default_title,
                custom_title,
                system_badge,
                core_so,
                core_override,
                box_art_path,
                allowed,
            });
        }
    }

    games.sort_by(|a, b| {
        a.display_title
            .to_ascii_lowercase()
            .cmp(&b.display_title.to_ascii_lowercase())
            .then_with(|| a.id.cmp(&b.id))
    });
    games
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::GameRule;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_clean_rom_title() {
        assert_eq!(
            clean_rom_title("Super Mario World (USA) [!]"),
            "Super Mario World"
        );
        assert_eq!(
            clean_rom_title("Pokemon_-_Emerald_Version_(USA,_Europe)_(Rev_1)"),
            "Pokemon - Emerald Version"
        );
        assert_eq!(clean_rom_title(" Kirby's Adventure (E) "), "Kirby's Adventure");
    }

    #[test]
    fn test_scan_roms_and_box_art() {
        let tmp = tempdir().unwrap();
        let gba_dir = tmp.path().join("GBA");
        let imgs_dir = gba_dir.join("Imgs");
        fs::create_dir_all(&imgs_dir).unwrap();

        let rom_file = gba_dir.join("Mario Kart - Super Circuit (USA).gba");
        fs::write(&rom_file, b"dummy rom").unwrap();
        let box_file = imgs_dir.join("Mario Kart - Super Circuit (USA).png");
        fs::write(&box_file, b"dummy png").unwrap();

        let mut cfg = ParentalConfig::default();
        cfg.games.insert(
            "GBA/Mario Kart - Super Circuit (USA).gba".to_string(),
            GameRule {
                allowed: true,
                custom_title: Some("Mario Kart GBA".to_string()),
                core_override: None,
            },
        );

        let games = scan_roms(&[tmp.path().to_path_buf()], &cfg, &[]);
        assert_eq!(games.len(), 1);
        let g = &games[0];
        assert_eq!(g.display_title, "Mario Kart GBA");
        assert_eq!(g.default_title, "Mario Kart - Super Circuit");
        assert_eq!(g.system_badge, "GBA");
        assert_eq!(g.core_so, "mgba_libretro.so");
        assert!(g.allowed);
        assert_eq!(
            g.box_art_path.as_deref(),
            Some(box_file.to_string_lossy().as_ref())
        );
    }
}
