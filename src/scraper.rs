use crate::roms::DiscoveredGame;
use image::imageops::FilterType;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScrapeReport {
    pub downloaded: usize,
    pub skipped: usize,
    pub failed: usize,
    pub details: Vec<String>,
}

fn libretro_repo_name(system_badge: &str) -> Option<&'static str> {
    match system_badge {
        "GBA" => Some("Nintendo_-_Game_Boy_Advance"),
        "GBC" => Some("Nintendo_-_Game_Boy_Color"),
        "GB" => Some("Nintendo_-_Game_Boy"),
        "SNES" => Some("Nintendo_-_Super_Nintendo_Entertainment_System"),
        "NES" => Some("Nintendo_-_Nintendo_Entertainment_System"),
        "MD" => Some("Sega_-_Mega_Drive_-_Genesis"),
        "SEGA" => Some("Sega_-_Master_System_-_Mark_III"),
        "PS1" => Some("Sony_-_PlayStation"),
        "NDS" => Some("Nintendo_-_Nintendo_DS"),
        "N64" => Some("Nintendo_-_Nintendo_64"),
        "PCE" => Some("NEC_-_PC_Engine_-_TurboGrafx_16"),
        _ => None,
    }
}

/// Percent-encodes a single URL path segment for `thumbnails.libretro.com`.
fn encode_path_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for b in s.as_bytes() {
        match *b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char);
            }
            _ => {
                out.push_str(&format!("%{:02X}", b));
            }
        }
    }
    out
}

/// Folds common Latin diacritics to ASCII lowercase for resilient accent-insensitive matching.
fn fold_accents_lower(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        let mapped = match ch {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'À' | 'Á' | 'Â' | 'Ã' | 'Ä' | 'Å' => 'a',
            'è' | 'é' | 'ê' | 'ë' | 'È' | 'É' | 'Ê' | 'Ë' => 'e',
            'ì' | 'í' | 'î' | 'ï' | 'Ì' | 'Í' | 'Î' | 'Ï' => 'i',
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ö' => 'o',
            'ù' | 'ú' | 'û' | 'ü' | 'Ù' | 'Ú' | 'Û' | 'Ü' => 'u',
            'ç' | 'Ç' => 'c',
            'ñ' | 'Ñ' => 'n',
            'ý' | 'ÿ' | 'Ý' => 'y',
            other => other.to_ascii_lowercase(),
        };
        out.push(mapped);
    }
    out
}

/// Strips parenthesized `(...)` and bracketed `[...]` tags (e.g. `(France)`, `(SGB Enhanced)`, `[!]`).
fn strip_bracket_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut paren = 0usize;
    let mut brack = 0usize;
    for ch in s.chars() {
        match ch {
            '(' => paren += 1,
            ')' => paren = paren.saturating_sub(1),
            '[' => brack += 1,
            ']' => brack = brack.saturating_sub(1),
            _ if paren == 0 && brack == 0 => out.push(ch),
            _ => {}
        }
    }
    out
}

/// Extracts normalized title tokens (accent-folded, noise/region words removed) for fuzzy matching.
fn normalized_tokens(raw: &str) -> Vec<String> {
    let stripped = strip_bracket_tags(raw);
    let folded = fold_accents_lower(&stripped);

    // Replace non-alphanumeric separators with spaces (keep apostrophes collapsed so "link's" -> "links")
    let mut cleaned = String::with_capacity(folded.len());
    for ch in folded.chars() {
        if ch.is_ascii_alphanumeric() {
            cleaned.push(ch);
        } else if ch == '\'' || ch == '\u{2019}' {
            // drop apostrophe so "Link's" matches "Links"
        } else {
            cleaned.push(' ');
        }
    }

    const STOP_WORDS: &[&str] = &[
        "france",
        "french",
        "usa",
        "europe",
        "world",
        "japan",
        "germany",
        "spain",
        "italy",
        "uk",
        "en",
        "fr",
        "de",
        "es",
        "it",
        "nl",
        "sv",
        "sgb",
        "cgb",
        "gb",
        "gba",
        "gbc",
        "snes",
        "nes",
        "enhanced",
        "compatible",
        "rev",
        "version",
        "edition",
        "speciale",
        "special",
        "the",
        "and",
        "of",
        "a",
        "la",
        "le",
        "les",
        "de",
        "du",
        "des",
        "v1",
        "v2",
    ];

    cleaned
        .split_whitespace()
        .filter(|w| !STOP_WORDS.contains(w))
        .map(|w| w.to_string())
        .collect()
}

/// Builds normalized query token sets from the game's title, including French->English Pokemon variants.
fn query_token_variants(game: &DiscoveredGame) -> Vec<HashSet<String>> {
    let mut raw_variants = Vec::new();
    let title = game.display_title.trim();
    if !title.is_empty() {
        raw_variants.push(title.to_string());
    }
    if game.default_title != title && !game.default_title.trim().is_empty() {
        raw_variants.push(game.default_title.trim().to_string());
    }

    // Add French -> English Pokemon color mappings so French ROMs also match (USA, Europe) box art
    let mut extra = Vec::new();
    for v in &raw_variants {
        let low = fold_accents_lower(v);
        if low.contains("pokemon") {
            let replacements = [
                ("emeraude", "emerald"),
                ("cristal", "crystal"),
                ("rouge", "red"),
                ("bleue", "blue"),
                ("bleu", "blue"),
                ("jaune", "yellow"),
                ("or", "gold"),
                ("argent", "silver"),
                ("rubis", "ruby"),
                ("saphir", "sapphire"),
                ("rouge feu", "fire red"),
                ("vert feuille", "leaf green"),
            ];
            let mut translated = low.clone();
            for (fr, en) in replacements {
                // Replace whole word boundaries cleanly
                let padded = format!(" {} ", translated.replace(['-', '_', '.', '(', ')'], " "));
                let needle = format!(" {} ", fr);
                if padded.contains(&needle) {
                    translated = padded.replace(&needle, &format!(" {} ", en));
                }
            }
            if translated != low {
                extra.push(translated);
            }
        }
    }
    raw_variants.extend(extra);

    let mut out = Vec::new();
    for v in raw_variants {
        let set: HashSet<String> = normalized_tokens(&v).into_iter().collect();
        if !set.is_empty() && !out.contains(&set) {
            out.push(set);
        }
    }
    out
}

/// Region tie-breaker bonus (higher = preferred when title match score is equal).
fn region_preference(filename: &str, prefer_france: bool) -> f32 {
    let low = filename.to_ascii_lowercase();
    if prefer_france && low.contains("(france)") {
        0.08
    } else if low.contains("(usa, europe)") {
        0.06
    } else if low.contains("(europe)") {
        0.05
    } else if low.contains("(usa)") {
        0.04
    } else if low.contains("(world)") {
        0.03
    } else if low.contains("(france)") {
        0.02
    } else if low.contains("(japan)") {
        -0.05
    } else {
        0.0
    }
}

/// Parses `Named_Boxarts/*.png` filenames out of a GitHub recursive tree API JSON response.
fn parse_github_tree_json(json_str: &str) -> Vec<String> {
    #[derive(Deserialize)]
    struct TreeEntry {
        path: String,
    }
    #[derive(Deserialize)]
    struct TreeResp {
        tree: Vec<TreeEntry>,
    }
    let Ok(parsed) = serde_json::from_str::<TreeResp>(json_str) else {
        return Vec::new();
    };
    parsed
        .tree
        .into_iter()
        .filter_map(|e| {
            e.path
                .strip_prefix("Named_Boxarts/")
                .filter(|name| name.ends_with(".png") && !name.contains('/'))
                .map(|name| name.to_string())
        })
        .collect()
}

/// Finds the best-matching `.png` filename from a Libretro directory index using normalized token overlap.
pub fn find_best_match_in_index<'a>(
    game: &DiscoveredGame,
    index: &'a [String],
) -> Option<&'a str> {
    let queries = query_token_variants(game);
    if queries.is_empty() || index.is_empty() {
        return None;
    }

    let prefer_france = fold_accents_lower(&game.default_title).contains("france")
        || fold_accents_lower(&game.display_title).contains("france");

    let mut best_name: Option<&'a str> = None;
    let mut best_score = 0.0f32;

    for entry in index {
        let stem = entry.strip_suffix(".png").unwrap_or(entry);
        let entry_tokens: HashSet<String> = normalized_tokens(stem).into_iter().collect();
        if entry_tokens.is_empty() {
            continue;
        }

        let mut max_sim = 0.0f32;
        for q in &queries {
            let inter = q.intersection(&entry_tokens).count() as f32;
            if inter == 0.0 {
                continue;
            }
            let union = q.union(&entry_tokens).count() as f32;
            let jaccard = inter / union;
            // Recall of query tokens helps when Libretro adds extra subtitle words like "Special Pikachu Edition"
            let recall = inter / (q.len() as f32);
            let precision = inter / (entry_tokens.len() as f32);
            let sim = jaccard * 0.55 + recall * 0.30 + precision * 0.15;
            if sim > max_sim {
                max_sim = sim;
            }
        }

        // Require a strong title match so we never attach an unrelated game's box art
        if max_sim >= 0.65 {
            let score = max_sim + region_preference(entry, prefer_france);
            if score > best_score {
                best_score = score;
                best_name = Some(entry.as_str());
            }
        }
    }

    best_name
}

/// Fetches and caches the `Named_Boxarts/` filename listing from `libretro-thumbnails` on GitHub.
fn fetch_system_index<'a>(
    repo_name: &'static str,
    cache: &'a mut HashMap<&'static str, Vec<String>>,
) -> &'a [String] {
    if !cache.contains_key(repo_name) {
        let url = format!(
            "https://api.github.com/repos/libretro-thumbnails/{}/git/trees/master?recursive=1",
            repo_name
        );
        let entries = match Command::new("curl")
            .args([
                "-fsSL",
                "--max-time",
                "20",
                "-H",
                "User-Agent: yasumuOS/0.1",
                "-H",
                "Accept: application/vnd.github+json",
                &url,
            ])
            .output()
        {
            Ok(out) if out.status.success() => {
                let body = String::from_utf8_lossy(&out.stdout);
                let list = parse_github_tree_json(&body);
                eprintln!(
                    "[yasumu-scraper] indexed {} box arts from {}",
                    list.len(),
                    repo_name
                );
                list
            }
            Ok(out) => {
                eprintln!(
                    "[yasumu-scraper] index fetch failed for {} (status={}): {}",
                    repo_name,
                    out.status,
                    String::from_utf8_lossy(&out.stderr).trim()
                );
                Vec::new()
            }
            Err(e) => {
                eprintln!("[yasumu-scraper] curl spawn failed for {}: {}", repo_name, e);
                Vec::new()
            }
        };
        cache.insert(repo_name, entries);
    }
    cache.get(repo_name).map(|v| v.as_slice()).unwrap_or(&[])
}

fn download_and_resize_cover(
    repo_name: &str,
    remote_filename: &str,
    tmp_path: &Path,
    dest_path: &Path,
) -> bool {
    let file_enc = encode_path_segment(remote_filename);
    let url = format!(
        "https://raw.githubusercontent.com/libretro-thumbnails/{}/master/Named_Boxarts/{}",
        repo_name, file_enc
    );
    let status = Command::new("curl")
        .args([
            "-fsSL",
            "--max-time",
            "12",
            "-H",
            "User-Agent: yasumuOS/0.1",
            "-o",
            tmp_path.to_string_lossy().as_ref(),
            &url,
        ])
        .status();

    if let Ok(st) = status {
        if st.success() && tmp_path.is_file() {
            if let Ok(img) = image::open(tmp_path) {
                let resized = img.resize(220, 220, FilterType::Triangle);
                if resized.save(dest_path).is_ok() {
                    let _ = fs::remove_file(tmp_path);
                    return true;
                }
            }
            let _ = fs::remove_file(tmp_path);
        }
    }
    false
}

/// Attempts to download and resize box art for a single game into `<rom_dir>/Imgs/<stem>.png`.
pub fn scrape_cover_for_game(game: &DiscoveredGame) -> Result<PathBuf, String> {
    let mut cache = HashMap::new();
    scrape_cover_with_cache(game, &mut cache)
}

fn scrape_cover_with_cache(
    game: &DiscoveredGame,
    index_cache: &mut HashMap<&'static str, Vec<String>>,
) -> Result<PathBuf, String> {
    let repo_name = libretro_repo_name(&game.system_badge)
        .ok_or_else(|| format!("unsupported system {}", game.system_badge))?;

    let rom_path = Path::new(&game.rom_path);
    let rom_dir = rom_path
        .parent()
        .ok_or_else(|| "invalid rom parent dir".to_string())?;
    let stem = rom_path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| "invalid rom filename".to_string())?;

    let imgs_dir = rom_dir.join("Imgs");
    fs::create_dir_all(&imgs_dir).map_err(|e| format!("create Imgs dir: {}", e))?;
    let dest_path = imgs_dir.join(format!("{}.png", stem));
    let tmp_path = imgs_dir.join(format!(".{}.tmp.png", stem));

    // Also try alternate repos for GB/GBC/GBA overlap (e.g. .7z in root ROMS defaults to GBC,
    // while Pokemon Red/Blue/Yellow/Super Mario Land 2 are in GB and Pokemon Emerald is in GBA)
    let mut repos = vec![repo_name];
    if game.system_badge == "GBC" {
        repos.push("Nintendo_-_Game_Boy");
        repos.push("Nintendo_-_Game_Boy_Advance");
    } else if game.system_badge == "GB" {
        repos.push("Nintendo_-_Game_Boy_Color");
    }

    for repo in repos {
        let matched = {
            let idx = fetch_system_index(repo, index_cache);
            find_best_match_in_index(game, idx).map(|s| s.to_string())
        };
        if let Some(remote_png) = matched {
            eprintln!(
                "[yasumu-scraper] matched '{}' -> {}/{}",
                game.display_title, repo, remote_png
            );
            if download_and_resize_cover(repo, &remote_png, &tmp_path, &dest_path) {
                return Ok(dest_path);
            }
        }
    }

    let _ = fs::remove_file(&tmp_path);
    Err(format!("no cover found for {}", game.display_title))
}

/// Scrapes missing covers for all provided games (skips games that already have `box_art_path`).
/// Fetches each Libretro system's `Named_Boxarts/` index once and matches titles fuzzily.
pub fn scrape_missing_covers(games: &[DiscoveredGame]) -> ScrapeReport {
    let mut downloaded = 0usize;
    let mut skipped = 0usize;
    let mut failed = 0usize;
    let mut details = Vec::new();
    let mut index_cache: HashMap<&'static str, Vec<String>> = HashMap::new();

    for game in games {
        if game.box_art_path.is_some() {
            skipped += 1;
            continue;
        }
        match scrape_cover_with_cache(game, &mut index_cache) {
            Ok(_) => {
                downloaded += 1;
                details.push(format!("OK: {}", game.display_title));
            }
            Err(e) => {
                failed += 1;
                details.push(format!("MISS: {}", e));
            }
        }
    }

    ScrapeReport {
        downloaded,
        skipped,
        failed,
        details,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_game(title: &str, badge: &str) -> DiscoveredGame {
        DiscoveredGame {
            id: title.to_string(),
            default_title: title.to_string(),
            display_title: title.to_string(),
            custom_title: None,
            rom_path: format!("/roms/{}/{}.zip", badge, title),
            system_badge: badge.to_string(),
            core_so: "gambatte_libretro.so".to_string(),
            core_override: None,
            allowed: true,
            box_art_path: None,
        }
    }

    #[test]
    fn test_fuzzy_matcher_real_french_and_messy_roms() {
        let index = vec![
            "Legend of Zelda, The - Link's Awakening DX (France) (SGB Enhanced) (GB Compatible).png".to_string(),
            "Legend of Zelda, The - Link's Awakening DX (USA, Europe) (Rev 2) (SGB Enhanced) (GB Compatible).png".to_string(),
            "Pokemon - Version Jaune - Edition Speciale Pikachu (France) (CGB+SGB Enhanced).png".to_string(),
            "Pokemon - Yellow Version - Special Pikachu Edition (USA, Europe) (CGB+SGB Enhanced).png".to_string(),
            "Pokemon - Emerald Version (USA, Europe).png".to_string(),
            "Pokemon - Crystal Version (USA, Europe) (Rev 1).png".to_string(),
            "Super Mario All-Stars + Super Mario World (USA).png".to_string(),
            "Super Mario World (USA).png".to_string(),
        ];

        assert_eq!(
            find_best_match_in_index(&fake_game("Legend of Zelda The - Link's Awakening DX France", "GBC"), &index),
            Some("Legend of Zelda, The - Link's Awakening DX (France) (SGB Enhanced) (GB Compatible).png")
        );
        assert_eq!(
            find_best_match_in_index(&fake_game("Pokemon - Version Jaune - Edition Speciale Pikachu France SGB Enhanced", "GBC"), &index),
            Some("Pokemon - Version Jaune - Edition Speciale Pikachu (France) (CGB+SGB Enhanced).png")
        );
        // French ROM matching English (USA, Europe) when no French box art is in index
        assert_eq!(
            find_best_match_in_index(&fake_game("Pokemon - Version Emeraude France", "GBA"), &index),
            Some("Pokemon - Emerald Version (USA, Europe).png")
        );
        assert_eq!(
            find_best_match_in_index(&fake_game("Pokemon - Version Cristal France", "GBC"), &index),
            Some("Pokemon - Crystal Version (USA, Europe) (Rev 1).png")
        );
        assert_eq!(
            find_best_match_in_index(&fake_game("Super Mario All-Stars and Super Mario World", "SNES"), &index),
            Some("Super Mario All-Stars + Super Mario World (USA).png")
        );
        // Unrelated game must not false-match
        assert_eq!(
            find_best_match_in_index(&fake_game("Metroid Fusion", "GBA"), &index),
            None
        );
    }

    #[test]
    fn test_parse_github_tree_json() {
        let json = r#"{
            "tree": [
                {"path": "README.md"},
                {"path": "Named_Boxarts/Super Mario World (USA).png"},
                {"path": "Named_Boxarts/Pokemon - Emerald Version (USA, Europe).png"},
                {"path": "Named_Snaps/Super Mario World (USA).png"}
            ]
        }"#;
        let parsed = parse_github_tree_json(json);
        assert_eq!(
            parsed,
            vec![
                "Super Mario World (USA).png",
                "Pokemon - Emerald Version (USA, Europe).png"
            ]
        );
    }
}
