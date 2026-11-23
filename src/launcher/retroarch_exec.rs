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

pub(super) fn execute_retroarch_game(socket_path: &Path, game: &DiscoveredGame, lang: Language) {
    let retroarch_bin = find_retroarch_binary();
    let core_path = find_libretro_core(&game.core_so);
    let override_cfg = std::env::var("YASUMU_RETROARCH_CFG")
        .or_else(|_| std::env::var("QUOS_RETROARCH_CFG"))
        .unwrap_or_else(|_| "/opt/yasumu/retroarch-override.cfg".to_string());

    // Write dynamic RetroArch user_language + discovered TrueType OSD font + per-system font size
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
        eprintln!(
            "[yasumu-launcher] RetroArch OSD font: {} (size={})",
            font_path.display(),
            osd_font_size
        );
        dynamic_cfg.push_str(&format!("video_font_path = \"{}\"\n", font_path.display()));
    }
    let _ = fs::write(lang_cfg_path, dynamic_cfg);

    let mut cmd = Command::new(&retroarch_bin);
    if Path::new(&override_cfg).is_file() {
        cmd.arg("--appendconfig")
            .arg(format!("{}|{}", override_cfg, lang_cfg_path));
    } else {
        cmd.arg("--appendconfig").arg(lang_cfg_path);
    }
    cmd.arg("-L").arg(&core_path).arg(&game.rom_path);

    eprintln!(
        "[yasumu-launcher] spawning RetroArch: bin={} core={} (exists={}) rom={}",
        retroarch_bin,
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
