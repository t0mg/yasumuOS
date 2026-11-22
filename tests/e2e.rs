use yasumu::daemon::{run_daemon, DaemonConfig};
use yasumu::ipc::{send_ipc_request, IpcRequest, IpcResponse};
use yasumu::launcher::run_launcher;
use yasumu::state::LockStatus;
use std::fs;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;
use std::time::Duration;

fn free_local_port() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap()
}

fn raw_http_request(addr: SocketAddr, method: &str, path: &str, body: Option<&str>) -> (u16, String, String) {
    let mut stream = TcpStream::connect(addr).expect("connect to portal HTTP");
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let req = if let Some(b) = body {
        format!(
            "{} {} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            method,
            path,
            addr,
            b.len(),
            b
        )
    } else {
        format!(
            "{} {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
            method, path, addr
        )
    };
    stream.write_all(req.as_bytes()).unwrap();
    let mut resp = String::new();
    stream.read_to_string(&mut resp).unwrap();

    let (headers, body_part) = resp.split_once("\r\n\r\n").unwrap_or((&resp, ""));
    let status_line = headers.lines().next().unwrap_or("");
    let status_code = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(0);
    (status_code, headers.to_string(), body_part.to_string())
}

#[test]
fn test_e2e_portal_api_and_headless_sdl2_launcher_screenshots() {
    std::env::set_var("SDL_VIDEODRIVER", "dummy");
    std::env::set_var("SDL_AUDIODRIVER", "dummy");
    // 2026-11-18 15:00:00 UTC (Wednesday)
    std::env::set_var("MOCK_TIME", "1795014000");

    let tmp = tempfile::tempdir().unwrap();
    let data_dir = tmp.path().join("parental");
    let roms_dir = tmp.path().join("ROMS");
    let gba_dir = roms_dir.join("GBA");
    let imgs_dir = gba_dir.join("Imgs");
    let snes_dir = roms_dir.join("SNES");
    fs::create_dir_all(&imgs_dir).unwrap();
    fs::create_dir_all(&snes_dir).unwrap();

    // Create sample ROMs and a real 64x64 PNG box art
    fs::write(gba_dir.join("Pokemon Emerald (USA).gba"), b"gba-rom").unwrap();
    fs::write(snes_dir.join("Super Mario World (USA) [!].sfc"), b"snes-rom").unwrap();

    let mut box_img = image::RgbaImage::new(64, 64);
    for (x, y, px) in box_img.enumerate_pixels_mut() {
        *px = image::Rgba([(x * 4) as u8, 180, (y * 4) as u8, 255]);
    }
    box_img
        .save(imgs_dir.join("Pokemon Emerald (USA).png"))
        .unwrap();

    let socket_path = data_dir.join("quos.sock");
    let portal_addr = free_local_port();
    let udp_addr = free_local_port();

    let cfg = DaemonConfig {
        data_dir: data_dir.clone(),
        socket_path: socket_path.clone(),
        rom_roots: vec![roms_dir.clone()],
        catalogue_roots: vec![],
        retroarch_udp: udp_addr,
        portal_bind: portal_addr,
        dev_mode: true,
        scan_proc_for_retroarch: false,
        start_portal_on_boot: false,
    };

    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_clone = stop_flag.clone();
    let daemon_thread = thread::spawn(move || {
        run_daemon(cfg, Some(stop_clone)).unwrap();
    });

    // Wait for daemon Unix socket to appear
    for _ in 0..50 {
        if socket_path.exists() {
            break;
        }
        thread::sleep(Duration::from_millis(25));
    }
    assert!(socket_path.exists(), "daemon socket should be created");

    // 1. Start first-boot Parent Portal via IPC
    let resp = send_ipc_request(&socket_path, &IpcRequest::StartPortalFirstBoot).unwrap();
    assert!(matches!(resp, IpcResponse::Ok { .. }));

    // Wait a moment for the portal HTTP thread to bind
    thread::sleep(Duration::from_millis(50));

    // 2. Verify captive-portal redirect (302 Found) on /generate_204 and /hotspot-detect.html
    let (code_204, hdrs_204, _) = raw_http_request(portal_addr, "GET", "/generate_204", None);
    assert_eq!(code_204, 302);
    assert!(hdrs_204.to_ascii_lowercase().contains("location: http://"));

    // 3. Verify GET / serves the self-contained single-file Parent Portal HTML
    let (code_html, _, html_body) = raw_http_request(portal_addr, "GET", "/", None);
    assert_eq!(code_html, 200);
    assert!(html_body.contains("YASUMU"));

    // 4. Sync time, set PIN to "1234", allowlist both games, and set custom schedule via HTTP REST API
    let (code_sync, _, _) = raw_http_request(
        portal_addr,
        "POST",
        "/api/sync-time",
        Some(r#"{"timestamp_ms":1795014000000,"timezone":"UTC"}"#),
    );
    assert_eq!(code_sync, 200);

    let (code_pin, _, _) = raw_http_request(
        portal_addr,
        "POST",
        "/api/pin",
        Some(r#"{"pin":"1234"}"#),
    );
    assert_eq!(code_pin, 200);

    let (code_games, _, _) = raw_http_request(
        portal_addr,
        "POST",
        "/api/games",
        Some(
            r#"{"games":[
                {"id":"GBA/Pokemon Emerald (USA).gba","allowed":true,"custom_title":"Pokemon Emerald","core_override":null},
                {"id":"SNES/Super Mario World (USA) [!].sfc","allowed":true,"custom_title":null,"core_override":null}
            ]}"#,
        ),
    );
    assert_eq!(code_games, 200);

    // Close portal so launcher starts on main game list screen
    let _ = send_ipc_request(&socket_path, &IpcRequest::StopPortal).unwrap();

    // 5. Run headless SDL2 launcher with a deterministic UI script that captures screenshots of:
    //    - Main Split-Pane Game List
    //    - PIN Tumbler screen (entering PIN 1-2-3-4)
    //    - Dual-QR Parent Portal screen
    let shot_main = tmp.path().join("shot_1_main.png");
    let shot_pin = tmp.path().join("shot_2_pin.png");
    let shot_qr = tmp.path().join("shot_3_qr.png");
    let script_path = tmp.path().join("ui_test.script");

    let script_content = format!(
        "SCREENSHOT {}\n\
         PRESS DOWN\n\
         PRESS SELECT\n\
         PRESS UP\n\
         PRESS RIGHT\n\
         PRESS UP\n\
         PRESS UP\n\
         PRESS RIGHT\n\
         PRESS UP\n\
         PRESS UP\n\
         PRESS UP\n\
         PRESS RIGHT\n\
         PRESS UP\n\
         PRESS UP\n\
         PRESS UP\n\
         PRESS UP\n\
         SCREENSHOT {}\n\
         PRESS CONFIRM\n\
         SCREENSHOT {}\n\
         PRESS BACK\n",
        shot_main.display(),
        shot_pin.display(),
        shot_qr.display()
    );
    fs::write(&script_path, script_content).unwrap();

    run_launcher(socket_path.clone(), None, Some(script_path)).unwrap();

    for shot in [&shot_main, &shot_pin, &shot_qr] {
        assert!(shot.is_file(), "screenshot {} should exist", shot.display());
        let img = image::open(shot).unwrap().to_rgba8();
        assert_eq!(img.dimensions(), (640, 480));
        // Verify non-blank rendering (multiple distinct colors present)
        let first_px = *img.get_pixel(0, 0);
        let has_contrast = img.pixels().any(|p| *p != first_px);
        assert!(
            has_contrast,
            "screenshot {} should have rendered UI content",
            shot.display()
        );
    }

    // 6. Test Quota Reached lock screen screenshot via `--dump-frame`
    let mut dummy_game = std::process::Command::new("sleep")
        .arg("10")
        .spawn()
        .unwrap();
    let _ = send_ipc_request(
        &socket_path,
        &IpcRequest::NotifyGamePid {
            pid: Some(dummy_game.id()),
        },
    )
    .unwrap();
    let _ = send_ipc_request(
        &socket_path,
        &IpcRequest::DevAdvanceTime {
            wall_delta_sec: 60 * 60,
            monotonic_delta_sec: 60 * 60,
        },
    )
    .unwrap();
    let _ = dummy_game.wait();
    let _ = send_ipc_request(&socket_path, &IpcRequest::NotifyGamePid { pid: None }).unwrap();

    if let IpcResponse::Status { status } =
        send_ipc_request(&socket_path, &IpcRequest::GetStatus).unwrap()
    {
        assert_eq!(status.lock_status, LockStatus::QuotaReached);
    } else {
        panic!("expected Status response");
    }

    let shot_locked = tmp.path().join("shot_4_locked.png");
    run_launcher(socket_path.clone(), Some(shot_locked.clone()), None).unwrap();
    assert!(shot_locked.is_file());

    stop_flag.store(true, Ordering::Relaxed);
    daemon_thread.join().unwrap();
}
