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

fn extract_cfg_value(line: &str) -> Option<String> {
    let (_, rhs) = line.split_once('=')?;
    let v = rhs.trim().trim_matches('"').trim();
    if v.is_empty() || v.eq_ignore_ascii_case("nul") {
        None
    } else {
        Some(v.to_string())
    }
}

/// Returns true if an `input_*` RetroArch config key is a safe gameplay/driver setting
/// (never a hotkey like FPS, slow-motion, rewind, menu toggle, save/load state, etc.).
fn is_allowed_input_key(key: &str) -> bool {
    if !key.starts_with("input_") {
        return true;
    }
    key.starts_with("input_player1_")
        || matches!(
            key,
            "input_driver"
                | "input_joypad_driver"
                | "input_max_users"
                | "input_axis_threshold"
                | "input_analog_deadzone"
                | "input_analog_sensitivity"
                | "input_poll_type_behavior"
                | "input_remap_binds_enable"
                | "input_auto_game_focus"
        )
}

/// Sanitizes a stock muOS `retroarch.cfg` into a locked-down base config:
/// - Preserves all video/audio/directory settings and all `input_player1_*` gameplay controls
///   (D-pad, face buttons, shoulders, L2/R2, Select, Start, and analog sticks).
/// - Strips every other `input_*` hotkey/menu/slow-motion/FPS/rewind/cheat binding so nothing
///   can fire without a hotkey modifier.
/// - Extracts the device's configured `MENU` button (`input_enable_hotkey_btn` / `input_menu_toggle_btn`)
///   so we can bind it to double-tap exit (`input_exit_emulator_btn`).
fn sanitize_base_cfg_contents(raw: &str) -> (String, Option<String>, bool) {
    let mut out = String::with_capacity(raw.len());
    let mut menu_btn: Option<String> = None;
    let mut fallback_menu_btn: Option<String> = None;
    let mut has_player1_binds = false;

    for line in raw.lines() {
        let trimmed = line.trim();
        let key = trimmed
            .split_once('=')
            .map(|(k, _)| k.trim())
            .unwrap_or("");

        if key == "input_enable_hotkey_btn" {
            if let Some(val) = extract_cfg_value(trimmed) {
                menu_btn = Some(val);
            }
        } else if key == "input_menu_toggle_btn" && fallback_menu_btn.is_none() {
            fallback_menu_btn = extract_cfg_value(trimmed);
        }

        if key == "input_player1_a_btn" && extract_cfg_value(trimmed).is_some() {
            has_player1_binds = true;
        }

        if !is_allowed_input_key(key) {
            if !key.is_empty() {
                out.push_str(&format!("{} = \"nul\"\n", key));
            }
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }

    (out, menu_btn.or(fallback_menu_btn), has_player1_binds)
}

/// Returns true if an `input_*` key inside a RetroArch autoconfig `.cfg` file is a
/// device identity or RetroPad gameplay control (never a hotkey/menu/cheat/state binding).
fn is_allowed_autoconfig_key(key: &str) -> bool {
    if !key.starts_with("input_") {
        return true;
    }
    matches!(
        key,
        "input_driver" | "input_device" | "input_vendor_id" | "input_product_id"
    ) || [
        "input_a_",
        "input_b_",
        "input_x_",
        "input_y_",
        "input_l_",
        "input_r_",
        "input_l2_",
        "input_r2_",
        "input_l3_",
        "input_r3_",
        "input_select_",
        "input_start_",
        "input_up_",
        "input_down_",
        "input_left_",
        "input_right_",
    ]
    .iter()
    .any(|p| key.starts_with(p))
}

fn sanitize_autoconfig_contents(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for line in raw.lines() {
        let trimmed = line.trim();
        let key = trimmed
            .split_once('=')
            .map(|(k, _)| k.trim())
            .unwrap_or("");
        if !is_allowed_autoconfig_key(key) {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
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
    ] {
        let p = PathBuf::from(candidate);
        if p.is_dir() {
            return Some(p);
        }
    }
    None
}

fn prepare_locked_autoconfig_dir(base_cfg: Option<&Path>) -> Option<PathBuf> {
    let src_dir = find_retroarch_autoconfig_dir(base_cfg)?;
    let dst_dir = PathBuf::from("/tmp/yasumu-autoconfig");
    let _ = fs::remove_dir_all(&dst_dir);
    fs::create_dir_all(&dst_dir).ok()?;

    let mut count = 0usize;
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
        // Only sanitize built-in handheld profiles (fast, skips 900+ desktop USB controllers)
        let fname_lower = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let is_handheld = fname_lower.contains("muos")
            || fname_lower.contains("anbernic")
            || fname_lower.contains("trimui")
            || fname_lower.contains("miyoo")
            || fname_lower.contains("retrogame")
            || fname_lower.contains("rg35")
            || fname_lower.contains("rg40")
            || fname_lower.contains("rg28")
            || fname_lower.contains("cube");
        if !is_handheld {
            continue;
        }
        let Ok(rel) = path.strip_prefix(&src_dir) else {
            continue;
        };
        let Ok(raw) = fs::read_to_string(path) else {
            continue;
        };
        let target = dst_dir.join(rel);
        if let Some(parent) = target.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if fs::write(&target, sanitize_autoconfig_contents(&raw)).is_ok() {
            count += 1;
        }
    }

    if count == 0 {
        return None;
    }
    eprintln!(
        "[yasumu-launcher] locked handheld autoconfigs: {} -> {} ({} files)",
        src_dir.display(),
        dst_dir.display(),
        count
    );
    Some(dst_dir)
}

pub(super) fn execute_retroarch_game(socket_path: &Path, game: &DiscoveredGame, lang: Language) {
    let retroarch_bin = find_retroarch_binary();
    let core_path = find_libretro_core(&game.core_so);
    let base_cfg = find_retroarch_base_cfg();
    let override_cfg = std::env::var("YASUMU_RETROARCH_CFG")
        .or_else(|_| std::env::var("QUOS_RETROARCH_CFG"))
        .unwrap_or_else(|_| "/opt/yasumu/retroarch-override.cfg".to_string());

    // Write dynamic RetroArch user_language + discovered TrueType OSD font + readable font size
    let lang_cfg_path = "/tmp/yasumu-lang.cfg";
    let osd_font_size = match game.system_badge.as_str() {
        "GB" | "GBC" | "GBA" => "20.000000",
        _ => "22.000000",
    };
    let mut dynamic_cfg = format!(
        "user_language = \"{}\"\nvideo_font_size = \"{}\"\n",
        lang.retroarch_user_language_id(),
        osd_font_size
    );
    if let Some(font_path) = find_retroarch_ttf_font() {
        eprintln!(
            "[yasumu-launcher] RetroArch OSD font: {} (size={})",
            font_path.display(),
            osd_font_size
        );
        dynamic_cfg.push_str(&format!("video_font_path = \"{}\"\n", font_path.display()));
    }

    let sanitized_base_path = PathBuf::from("/tmp/yasumu-base.cfg");
    let mut effective_base_cfg: Option<PathBuf> = None;
    let mut has_player1_binds = false;
    let mut exit_btn = "10".to_string();

    if let Some(ref base) = base_cfg {
        if let Ok(raw) = fs::read_to_string(base) {
            let (sanitized, menu_btn, p1_ok) = sanitize_base_cfg_contents(&raw);
            if let Some(btn) = menu_btn {
                exit_btn = btn;
            }
            has_player1_binds = p1_ok;
            if fs::write(&sanitized_base_path, sanitized).is_ok() {
                effective_base_cfg = Some(sanitized_base_path.clone());
            }
            eprintln!(
                "[yasumu-launcher] sanitized base_cfg={} (p1_binds={}, menu_btn={})",
                base.display(),
                has_player1_binds,
                exit_btn
            );
        }
    }

    let locked_autoconfig = prepare_locked_autoconfig_dir(base_cfg.as_deref());
    if let Some(ref locked_dir) = locked_autoconfig {
        dynamic_cfg.push_str(&format!(
            "joypad_autoconfig_dir = \"{}\"\n",
            locked_dir.display()
        ));
    }

    if !has_player1_binds && locked_autoconfig.is_none() {
        // Guaranteed fallback only if neither stock retroarch.cfg nor handheld autoconfigs were found
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
             input_player1_right_btn = \"h0right\"\n",
        );
    }
    dynamic_cfg.push_str(&format!("input_exit_emulator_btn = \"{}\"\n", exit_btn));
    let _ = fs::write(lang_cfg_path, dynamic_cfg);

    let mut cmd = Command::new(&retroarch_bin);
    if let Some(ref base) = effective_base_cfg {
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
        effective_base_cfg,
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
            eprintln!(
                "[yasumu-launcher] failed to spawn RetroArch ({}): {}",
                retroarch_bin, e
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_base_cfg_preserves_gameplay_and_strips_all_hotkeys() {
        let sample = r#"
video_driver = "gl"
input_driver = "udev"
input_joypad_driver = "udev"
input_player1_a_btn = "0"
input_player1_b_btn = "1"
input_player1_l_x_plus_axis = "+0"
input_player1_l_x_minus_axis = "-0"
input_enable_hotkey_btn = "10"
input_menu_toggle_btn = "2"
input_fps_toggle_btn = "3"
input_hold_slowmotion_btn = "7"
input_toggle_slowmotion_btn = "6"
input_save_state_btn = "5"
input_load_state_btn = "4"
input_toggle_fast_forward_btn = "6"
input_rewind_btn = "4"
"#;
        let (sanitized, menu_btn, has_p1) = sanitize_base_cfg_contents(sample);
        assert!(has_p1);
        assert_eq!(menu_btn.as_deref(), Some("10"));
        assert!(sanitized.contains("video_driver = \"gl\""));
        assert!(sanitized.contains("input_driver = \"udev\""));
        assert!(sanitized.contains("input_player1_a_btn = \"0\""));
        assert!(sanitized.contains("input_player1_l_x_plus_axis = \"+0\""));
        assert!(sanitized.contains("input_enable_hotkey_btn = \"nul\""));
        assert!(sanitized.contains("input_menu_toggle_btn = \"nul\""));
        assert!(sanitized.contains("input_fps_toggle_btn = \"nul\""));
        assert!(sanitized.contains("input_hold_slowmotion_btn = \"nul\""));
        assert!(sanitized.contains("input_toggle_slowmotion_btn = \"nul\""));
        assert!(sanitized.contains("input_save_state_btn = \"nul\""));
        assert!(sanitized.contains("input_load_state_btn = \"nul\""));
        assert!(sanitized.contains("input_toggle_fast_forward_btn = \"nul\""));
        assert!(sanitized.contains("input_rewind_btn = \"nul\""));
    }
}

