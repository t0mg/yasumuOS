use crate::i18n::Language;
use crate::ipc::{send_ipc_request, IpcRequest};
use crate::roms::DiscoveredGame;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn find_retroarch_binary() -> String {
    if let Ok(env_bin) = std::env::var("QUOS_RETROARCH_BIN") {
        if !env_bin.trim().is_empty() {
            return env_bin;
        }
    }
    let candidates = [
        "/usr/bin/retroarch",
        "/opt/muos/share/emulator/retroarch/retroarch",
        "/opt/muos/bin/retroarch",
    ];
    for c in candidates {
        if Path::new(c).is_file() {
            return c.to_string();
        }
    }
    "retroarch".to_string()
}

fn find_retroarch_base_cfg() -> Option<PathBuf> {
    let candidates = [
        "/opt/muos/share/info/config/retroarch.cfg",
        "/run/muos/storage/info/config/retroarch.cfg",
        "/mnt/mmc/MUOS/info/config/retroarch.cfg",
        "/opt/muos/device/control/retroarch.cfg",
        "/opt/muos/share/emulator/retroarch/retroarch.cfg",
        "/mnt/mmc/MUOS/retroarch/retroarch.cfg",
        "/run/muos/storage/retroarch/retroarch.cfg",
        "/root/.config/retroarch/retroarch.cfg",
    ];
    for c in candidates {
        let p = PathBuf::from(c);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

fn find_libretro_core(core_so: &str) -> PathBuf {
    if Path::new(core_so).is_file() {
        return PathBuf::from(core_so);
    }
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Ok(env_dir) = std::env::var("QUOS_CORES_DIR") {
        if !env_dir.trim().is_empty() {
            dirs.push(PathBuf::from(env_dir));
        }
    }
    dirs.extend([
        PathBuf::from("/opt/muos/share/core"),
        PathBuf::from("/run/muos/storage/core"),
        PathBuf::from("/mnt/mmc/MUOS/core"),
        PathBuf::from("/mnt/union/MUOS/core"),
        PathBuf::from("/usr/lib/libretro"),
    ]);

    for dir in &dirs {
        let direct = dir.join(core_so);
        if direct.is_file() {
            return direct;
        }
    }
    // Fallback to /opt/muos/share/core/<core_so>
    PathBuf::from("/opt/muos/share/core").join(core_so)
}

fn find_retroarch_ttf_font() -> Option<PathBuf> {
    let direct = [
        "/opt/muos/share/emulator/retroarch/assets/ozone/bold.ttf",
        "/opt/muos/share/emulator/retroarch/assets/ozone/regular.ttf",
        "/opt/muos/share/emulator/retroarch/assets/xmb/monochrome/font.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
        "/usr/share/fonts/TTF/DejaVuSans-Bold.ttf",
        "/usr/share/fonts/DejaVuSans-Bold.ttf",
    ];
    for p in direct {
        let pb = PathBuf::from(p);
        if pb.is_file() {
            return Some(pb);
        }
    }
    for root in [
        "/opt/muos/share/emulator/retroarch/assets",
        "/usr/share/fonts",
        "/opt/muos/share/theme",
    ] {
        if !Path::new(root).is_dir() {
            continue;
        }
        for entry in walkdir::WalkDir::new(root)
            .max_depth(4)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if entry.path().is_file() {
                if let Some(ext) = entry.path().extension().and_then(|e| e.to_str()) {
                    if ext.eq_ignore_ascii_case("ttf") || ext.eq_ignore_ascii_case("otf") {
                        return Some(entry.path().to_path_buf());
                    }
                }
            }
        }
    }
    None
}

/// Keys stripped from stock RetroArch autoconfig `.cfg` files so in-game menus,
/// save-state hotkeys, fast-forward, and cheats stay locked down on every device.
const LOCKED_AUTOCONFIG_PREFIXES: &[&str] = &[
    "input_menu_toggle",
    "input_enable_hotkey",
    "input_exit_emulator",
    "input_save_state",
    "input_load_state",
    "input_state_slot",
    "input_reset",
    "input_toggle_fast_forward",
    "input_hold_fast_forward",
    "input_rewind",
    "input_pause_toggle",
    "input_fps_toggle",
    "input_screenshot",
    "input_shader",
    "input_cheat",
    "input_disk",
    "input_netplay",
    "input_ai_service",
    "input_volume",
];

fn extract_cfg_value(line: &str) -> Option<String> {
    let (_, rhs) = line.split_once('=')?;
    let v = rhs.trim().trim_matches('"').trim();
    if v.is_empty() || v.eq_ignore_ascii_case("nul") {
        None
    } else {
        Some(v.to_string())
    }
}

/// Sanitizes a single RetroArch autoconfig `.cfg` file:
/// - Preserves all gameplay controls (D-pad, A/B/X/Y, L/R/L2/R2/L3/R3, Select, Start, analog sticks).
/// - Strips all RetroArch menu/hotkey/cheat/state-slot bindings.
/// - Extracts the device's `MENU` / hotkey button and rebinds it to `input_exit_emulator_btn`
///   (paired with `quit_press_twice = "true"` so holding MENU + VOL+/- still works).
fn sanitize_autoconfig_contents(raw: &str) -> (String, Option<String>) {
    let mut out = String::with_capacity(raw.len() + 64);
    let mut menu_btn: Option<String> = None;

    for line in raw.lines() {
        let trimmed = line.trim();
        let key = trimmed
            .split_once('=')
            .map(|(k, _)| k.trim())
            .unwrap_or("");

        if key == "input_enable_hotkey_btn" || (menu_btn.is_none() && key == "input_menu_toggle_btn")
        {
            if let Some(val) = extract_cfg_value(trimmed) {
                menu_btn = Some(val);
            }
        }

        if LOCKED_AUTOCONFIG_PREFIXES.iter().any(|p| key.starts_with(p)) {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }

    if let Some(ref btn) = menu_btn {
        out.push_str(&format!("input_exit_emulator_btn = \"{}\"\n", btn));
    }
    (out, menu_btn)
}

fn extract_menu_btn_from_cfg(cfg_path: &Path) -> Option<String> {
    let contents = fs::read_to_string(cfg_path).ok()?;
    let mut fallback_menu: Option<String> = None;
    for line in contents.lines() {
        let trimmed = line.trim();
        let Some((k, _)) = trimmed.split_once('=') else {
            continue;
        };
        let key = k.trim();
        if key == "input_enable_hotkey_btn" {
            if let Some(val) = extract_cfg_value(trimmed) {
                return Some(val);
            }
        } else if key == "input_menu_toggle_btn" && fallback_menu.is_none() {
            fallback_menu = extract_cfg_value(trimmed);
        }
    }
    fallback_menu
}

fn find_retroarch_autoconfig_dir(base_cfg: Option<&Path>) -> Option<PathBuf> {
    if let Some(cfg_path) = base_cfg {
        if let Ok(contents) = fs::read_to_string(cfg_path) {
            for line in contents.lines() {
                let trimmed = line.trim();
                if let Some((k, _)) = trimmed.split_once('=') {
                    if k.trim() == "joypad_autoconfig_dir" {
                        if let Some(val) = extract_cfg_value(trimmed) {
                            let expanded = if let Some(rest) = val.strip_prefix("~/") {
                                PathBuf::from("/root").join(rest)
                            } else {
                                PathBuf::from(&val)
                            };
                            if expanded.is_dir() {
                                return Some(expanded);
                            }
                        }
                    }
                }
            }
        }
    }
    for candidate in [
        "/opt/muos/share/info/config/autoconfig",
        "/opt/muos/share/emulator/retroarch/autoconfig",
        "/mnt/mmc/MUOS/retroarch/autoconfig",
        "/run/muos/storage/retroarch/autoconfig",
        "/opt/muos/config/retroarch/autoconfig",
    ] {
        let p = PathBuf::from(candidate);
        if p.is_dir() {
            return Some(p);
        }
    }
    None
}

fn prepare_locked_autoconfig_dir(base_cfg: Option<&Path>) -> Option<(PathBuf, Option<String>)> {
    let src_dir = find_retroarch_autoconfig_dir(base_cfg)?;
    let dst_dir = PathBuf::from("/tmp/yasumu-autoconfig");
    let _ = fs::remove_dir_all(&dst_dir);
    fs::create_dir_all(&dst_dir).ok()?;

    let mut count = 0usize;
    let mut detected_menu_btn: Option<String> = None;

    for entry in walkdir::WalkDir::new(&src_dir)
        .max_depth(3)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let is_cfg = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("cfg"))
            .unwrap_or(false);
        if !is_cfg {
            continue;
        }
        let Ok(rel) = path.strip_prefix(&src_dir) else {
            continue;
        };
        let Ok(raw) = fs::read_to_string(path) else {
            continue;
        };
        let (sanitized, menu_btn) = sanitize_autoconfig_contents(&raw);
        if detected_menu_btn.is_none() {
            detected_menu_btn = menu_btn;
        }
        let target = dst_dir.join(rel);
        if let Some(parent) = target.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if fs::write(&target, sanitized).is_ok() {
            count += 1;
        }
    }

    if count == 0 {
        return None;
    }
    eprintln!(
        "[yasumu-launcher] locked autoconfig: {} -> {} ({} files, menu_btn={:?})",
        src_dir.display(),
        dst_dir.display(),
        count,
        detected_menu_btn
    );
    Some((dst_dir, detected_menu_btn))
}

pub(super) fn execute_retroarch_game(socket_path: &Path, game: &DiscoveredGame, lang: Language) {
    let retroarch_bin = find_retroarch_binary();
    let core_path = find_libretro_core(&game.core_so);
    let base_cfg = find_retroarch_base_cfg();
    let override_cfg = std::env::var("YASUMU_RETROARCH_CFG")
        .or_else(|_| std::env::var("QUOS_RETROARCH_CFG"))
        .unwrap_or_else(|_| "/opt/yasumu/retroarch-override.cfg".to_string());

    // Write dynamic RetroArch user_language + discovered TrueType OSD font + per-system font size (+50%)
    let lang_cfg_path = "/tmp/yasumu-lang.cfg";
    let osd_font_size = match game.system_badge.as_str() {
        "GB" | "GBC" => "27.000000",
        "GBA" => "30.000000",
        _ => "42.000000",
    };
    let mut dynamic_cfg = format!(
        "user_language = \"{}\"\nvideo_font_size = \"{}\"\n",
        lang.retroarch_user_language_id(),
        osd_font_size
    );
    if let Some(font_path) = find_retroarch_ttf_font() {
        eprintln!("[yasumu-launcher] RetroArch OSD font: {} (size={})", font_path.display(), osd_font_size);
        dynamic_cfg.push_str(&format!("video_font_path = \"{}\"\n", font_path.display()));
    }
    let autoconfig_result = prepare_locked_autoconfig_dir(base_cfg.as_deref());
    let base_menu_btn = base_cfg.as_deref().and_then(extract_menu_btn_from_cfg);

    if let Some((locked_dir, menu_btn)) = autoconfig_result {
        dynamic_cfg.push_str(&format!(
            "joypad_autoconfig_dir = \"{}\"\n",
            locked_dir.display()
        ));
        let exit_btn = menu_btn
            .or(base_menu_btn)
            .unwrap_or_else(|| "10".to_string());
        dynamic_cfg.push_str(&format!("input_exit_emulator_btn = \"{}\"\n", exit_btn));
    } else if base_cfg.is_some() {
        let exit_btn = base_menu_btn.unwrap_or_else(|| "10".to_string());
        dynamic_cfg.push_str(&format!("input_exit_emulator_btn = \"{}\"\n", exit_btn));
    } else {
        // Safety fallback if neither stock retroarch.cfg nor autoconfig dir was found:
        // bind standard muOS-Keys udev layout so controls can never go dead.
        dynamic_cfg.push_str(
            "input_driver = \"udev\"\n\
             input_joypad_driver = \"udev\"\n\
             input_autodetect_enable = \"false\"\n\
             input_player1_a_btn = \"0\"\n\
             input_player1_b_btn = \"1\"\n\
             input_player1_x_btn = \"2\"\n\
             input_player1_y_btn = \"3\"\n\
             input_player1_l_btn = \"4\"\n\
             input_player1_r_btn = \"5\"\n\
             input_player1_l2_btn = \"6\"\n\
             input_player1_r2_btn = \"7\"\n\
             input_player1_select_btn = \"8\"\n\
             input_player1_start_btn = \"9\"\n\
             input_player1_up_btn = \"h0up\"\n\
             input_player1_down_btn = \"h0down\"\n\
             input_player1_left_btn = \"h0left\"\n\
             input_player1_right_btn = \"h0right\"\n\
             input_exit_emulator_btn = \"10\"\n",
        );
    }
    let _ = fs::write(lang_cfg_path, dynamic_cfg);

    let mut cmd = Command::new(&retroarch_bin);
    if let Some(ref base) = base_cfg {
        cmd.arg("-c").arg(base);
    }
    if Path::new(&override_cfg).is_file() {
        cmd.arg("--appendconfig")
            .arg(format!("{}|{}", override_cfg, lang_cfg_path));
    } else {
        cmd.arg("--appendconfig").arg(lang_cfg_path);
    }
    cmd.arg("-L").arg(&core_path).arg(&game.rom_path);

    eprintln!(
        "[yasumu-launcher] spawning RetroArch: bin={} base_cfg={:?} core={} (exists={}) rom={}",
        retroarch_bin,
        base_cfg,
        core_path.display(),
        core_path.is_file(),
        game.rom_path
    );

    match cmd.spawn() {
        Ok(mut child) => {
            let pid = child.id();
            let _ = send_ipc_request(socket_path, &IpcRequest::NotifyGamePid { pid: Some(pid) });
            match child.wait() {
                Ok(status) => eprintln!("[yasumu-launcher] RetroArch exited with {}", status),
                Err(e) => eprintln!("[yasumu-launcher] RetroArch wait error: {}", e),
            }
            let _ = send_ipc_request(socket_path, &IpcRequest::NotifyGamePid { pid: None });
        }
        Err(e) => {
            eprintln!("[yasumu-launcher] failed to spawn RetroArch ({}): {}", retroarch_bin, e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_autoconfig_preserves_gameplay_and_locks_hotkeys() {
        let sample = r#"
input_driver = "udev"
input_device = "muOS-Keys"
input_a_btn = "0"
input_b_btn = "1"
input_l_x_plus_axis = "+0"
input_l_x_minus_axis = "-0"
input_enable_hotkey_btn = "10"
input_menu_toggle_btn = "2"
input_exit_emulator_btn = "9"
input_save_state_btn = "5"
input_load_state_btn = "4"
input_toggle_fast_forward_btn = "6"
"#;
        let (sanitized, menu_btn) = sanitize_autoconfig_contents(sample);
        assert_eq!(menu_btn.as_deref(), Some("10"));
        assert!(sanitized.contains("input_a_btn = \"0\""));
        assert!(sanitized.contains("input_l_x_plus_axis = \"+0\""));
        assert!(sanitized.contains("input_exit_emulator_btn = \"10\""));
        assert!(!sanitized.contains("input_enable_hotkey_btn"));
        assert!(!sanitized.contains("input_menu_toggle_btn"));
        assert!(!sanitized.contains("input_save_state_btn"));
        assert!(!sanitized.contains("input_load_state_btn"));
        assert!(!sanitized.contains("input_toggle_fast_forward_btn"));
        assert!(!sanitized.contains("input_exit_emulator_btn = \"9\""));
    }
}

