mod render;
mod retroarch_exec;

use crate::i18n::{strings, Language, Strings};
use crate::ipc::{send_ipc_request, DaemonStatus, IpcRequest, IpcResponse};
use crate::roms::DiscoveredGame;
use crate::sdl2_sys::*;
use crate::state::LockStatus;
use crate::ui_renderer::{FontKind, UiCanvas};
use anyhow::{bail, Context, Result};
use retroarch_exec::execute_retroarch_game;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum InputAction {
    Up,
    Down,
    Left,
    Right,
    Confirm,      // A button / Enter / Z
    Back,         // B button / Backspace / X
    ActionX,      // X button / C key (Exit to muOS from Parent Portal screen)
    ActionY,      // Y button (Force Offline AP from Parent Portal screen)
    ParentSelect, // Select button / Shift / Tab
    QuitApp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiScreen {
    MainOrLock,
    PowerOffConfirm,
    ShuttingDown,
    PinTumbler {
        digits: [u8; 4],
        cursor: usize,
        error_msg: Option<String>,
    },
    ParentPortal,
}

pub struct BoxArtTexture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub struct LauncherApp {
    pub socket_path: PathBuf,
    pub status: DaemonStatus,
    pub screen: UiScreen,
    pub selected_index: usize,
    pub box_art_cache: HashMap<String, Option<BoxArtTexture>>,
    pub ui: UiCanvas,
    pub should_exit: bool,
    pub exit_to_muos: bool,
    pub pending_game: Option<DiscoveredGame>,
    pub marquee_started_at: Instant,
}

impl LauncherApp {
    pub fn new(socket_path: PathBuf) -> Result<Self> {
        let status = Self::fetch_status(&socket_path)?;
        let screen = if status.portal_active {
            UiScreen::ParentPortal
        } else {
            UiScreen::MainOrLock
        };
        Ok(Self {
            socket_path,
            status,
            screen,
            selected_index: 0,
            box_art_cache: HashMap::new(),
            ui: UiCanvas::new(640, 480),
            should_exit: false,
            exit_to_muos: false,
            pending_game: None,
            marquee_started_at: Instant::now(),
        })
    }

    pub fn fetch_status(socket_path: &Path) -> Result<DaemonStatus> {
        match send_ipc_request(socket_path, &IpcRequest::GetStatus)? {
            IpcResponse::Status { status } | IpcResponse::Ok { status } => Ok(status),
            IpcResponse::Error { message } => bail!("daemon error: {}", message),
            other => bail!("unexpected IPC response: {:?}", other),
        }
    }

    pub fn refresh_status(&mut self) {
        if let Ok(st) = Self::fetch_status(&self.socket_path) {
            if st.exit_to_muos_requested {
                self.exit_to_muos = true;
                self.should_exit = true;
            }
            if matches!(self.screen, UiScreen::ParentPortal) && !st.portal_active {
                self.screen = UiScreen::MainOrLock;
            }
            self.status = st;
            let total_entries = self.total_list_items();
            if self.selected_index >= total_entries && total_entries > 0 {
                self.selected_index = total_entries - 1;
            }
        }
    }

    pub fn tr(&self) -> &'static Strings {
        strings(Language::from_code(&self.status.language))
    }

    /// Total items in the vertical list: allowlisted games + Power Off at bottom.
    pub fn total_list_items(&self) -> usize {
        self.status.allowed_games.len() + 1
    }

    pub fn request_power_off(&mut self) {
        self.screen = UiScreen::ShuttingDown;
        let _ = send_ipc_request(&self.socket_path, &IpcRequest::RequestPowerOff);
    }

    pub fn open_parent_flow(&mut self) {
        self.refresh_status();
        eprintln!(
            "[yasumu-launcher] open_parent_flow has_pin={} portal_active={}",
            self.status.has_pin, self.status.portal_active
        );
        if !self.status.has_pin {
            // First-boot flow: skip PIN gate and launch Parent Portal directly.
            // Never fall through to PinTumbler when no PIN has been configured yet.
            match send_ipc_request(&self.socket_path, &IpcRequest::StartPortalFirstBoot) {
                Ok(IpcResponse::Ok { status }) => {
                    self.status = status;
                    self.screen = UiScreen::ParentPortal;
                    eprintln!("[yasumu-launcher] entered ParentPortal (first boot)");
                }
                Ok(IpcResponse::Error { message }) => {
                    eprintln!("[yasumu-launcher] StartPortalFirstBoot error: {}", message);
                }
                Err(e) => {
                    eprintln!("[yasumu-launcher] StartPortalFirstBoot IPC failed: {}", e);
                }
                _ => {}
            }
            return;
        }
        self.screen = UiScreen::PinTumbler {
            digits: [0, 0, 0, 0],
            cursor: 0,
            error_msg: None,
        };
    }

    pub fn handle_action(&mut self, action: InputAction) {
        if action == InputAction::QuitApp {
            self.should_exit = true;
            return;
        }

        match self.screen.clone() {
            UiScreen::MainOrLock => {
                if action == InputAction::ParentSelect {
                    self.open_parent_flow();
                    return;
                }

                if self.status.lock_status != LockStatus::Unlocked {
                    // On any lock screen: A opens PIN/Portal, X opens Power Off confirmation
                    if action == InputAction::Confirm {
                        self.open_parent_flow();
                    } else if action == InputAction::ActionX {
                        self.screen = UiScreen::PowerOffConfirm;
                    }
                    return;
                }

                let count = self.total_list_items();
                match action {
                    InputAction::Up => {
                        if self.selected_index > 0 {
                            self.selected_index -= 1;
                        } else if count > 0 {
                            self.selected_index = count - 1;
                        }
                        self.marquee_started_at = Instant::now();
                    }
                    InputAction::Down => {
                        if self.selected_index + 1 < count {
                            self.selected_index += 1;
                        } else {
                            self.selected_index = 0;
                        }
                        self.marquee_started_at = Instant::now();
                    }
                    InputAction::Confirm => {
                        if self.selected_index == self.status.allowed_games.len() {
                            // Bottom entry: Power Off (opens confirmation dialog)
                            self.screen = UiScreen::PowerOffConfirm;
                        } else if let Some(game) =
                            self.status.allowed_games.get(self.selected_index).cloned()
                        {
                            self.launch_game(&game);
                        }
                    }
                    _ => {}
                }
            }
            UiScreen::PowerOffConfirm => match action {
                InputAction::Confirm => self.request_power_off(),
                InputAction::Back => self.screen = UiScreen::MainOrLock,
                _ => {}
            },
            UiScreen::ShuttingDown => {}
            UiScreen::PinTumbler {
                mut digits,
                mut cursor,
                mut error_msg,
            } => match action {
                InputAction::Up => {
                    digits[cursor] = (digits[cursor] + 1) % 10;
                    error_msg = None;
                    self.screen = UiScreen::PinTumbler {
                        digits,
                        cursor,
                        error_msg,
                    };
                }
                InputAction::Down => {
                    digits[cursor] = (digits[cursor] + 9) % 10;
                    error_msg = None;
                    self.screen = UiScreen::PinTumbler {
                        digits,
                        cursor,
                        error_msg,
                    };
                }
                InputAction::Left => {
                    cursor = if cursor > 0 { cursor - 1 } else { 3 };
                    self.screen = UiScreen::PinTumbler {
                        digits,
                        cursor,
                        error_msg,
                    };
                }
                InputAction::Right => {
                    cursor = (cursor + 1) % 4;
                    self.screen = UiScreen::PinTumbler {
                        digits,
                        cursor,
                        error_msg,
                    };
                }
                InputAction::Back => {
                    self.screen = UiScreen::MainOrLock;
                }
                InputAction::Confirm => {
                    let pin = format!("{}{}{}{}", digits[0], digits[1], digits[2], digits[3]);
                    match send_ipc_request(
                        &self.socket_path,
                        &IpcRequest::VerifyPinAndStartPortal { pin },
                    ) {
                        Ok(IpcResponse::PinResult {
                            unlocked: true,
                            status,
                            ..
                        }) => {
                            self.status = status;
                            self.screen = UiScreen::ParentPortal;
                        }
                        Ok(IpcResponse::PinResult {
                            unlocked: false,
                            lockout_seconds,
                            status,
                        }) => {
                            self.status = status;
                            let tr = self.tr();
                            let msg = if lockout_seconds > 0 {
                                format!("{} {}s.", tr.pin_cooldown_prefix, lockout_seconds)
                            } else {
                                tr.pin_error_sub.to_string()
                            };
                            self.screen = UiScreen::PinTumbler {
                                digits: [0, 0, 0, 0],
                                cursor: 0,
                                error_msg: Some(msg),
                            };
                        }
                        Ok(IpcResponse::Error { message }) => {
                            self.screen = UiScreen::PinTumbler {
                                digits,
                                cursor,
                                error_msg: Some(message),
                            };
                        }
                        _ => {}
                    }
                }
                _ => {}
            },
            UiScreen::ParentPortal => match action {
                InputAction::Back => {
                    if let Ok(IpcResponse::Ok { status }) =
                        send_ipc_request(&self.socket_path, &IpcRequest::StopPortal)
                    {
                        self.status = status;
                    }
                    self.screen = UiScreen::MainOrLock;
                }
                InputAction::ActionY => {
                    if let Ok(IpcResponse::Ok { status }) =
                        send_ipc_request(&self.socket_path, &IpcRequest::ForceOfflineAp)
                    {
                        self.status = status;
                    }
                }
                InputAction::ActionX => {
                    if let Ok(IpcResponse::Ok { status }) =
                        send_ipc_request(&self.socket_path, &IpcRequest::RequestExitToMuos)
                    {
                        self.status = status;
                    }
                    self.exit_to_muos = true;
                    self.should_exit = true;
                }
                _ => {}
            },
        }
    }

    fn launch_game(&mut self, game: &DiscoveredGame) {
        match send_ipc_request(
            &self.socket_path,
            &IpcRequest::PrepareGameLaunch {
                game_id: game.id.clone(),
            },
        ) {
            Ok(IpcResponse::LaunchApproved { game }) => {
                eprintln!(
                    "[yasumu-launcher] approved game launch: {} (core={})",
                    game.rom_path, game.core_so
                );
                self.pending_game = Some(game);
            }
            Ok(IpcResponse::Error { message }) => {
                eprintln!("[yasumu-launcher] launch denied: {}", message);
                self.refresh_status();
            }
            Err(e) => {
                eprintln!("[yasumu-launcher] PrepareGameLaunch IPC error: {}", e);
                self.refresh_status();
            }
            _ => {
                self.refresh_status();
            }
        }
    }

    fn load_box_art(&mut self, path: &str) -> Option<&BoxArtTexture> {
        if !self.box_art_cache.contains_key(path) {
            let loaded = image::open(path).ok().map(|img| {
                let rgba = img.to_rgba8();
                let (w, h) = rgba.dimensions();
                BoxArtTexture {
                    width: w,
                    height: h,
                    rgba: rgba.into_raw(),
                }
            });
            self.box_art_cache.insert(path.to_string(), loaded);
        }
        self.box_art_cache.get(path).and_then(|v| v.as_ref())
    }

    /// Renders the active screen into the 640×480 `UiCanvas` and presents it via SDL2.
    pub fn render(&mut self, canvas: &SdlCanvas) {
        self.render_to_ui();
        canvas.present_rgba(self.ui.rgba_bytes());
    }

    pub fn render_to_ui(&mut self) {
        match self.screen.clone() {
            UiScreen::MainOrLock => {
                if self.status.lock_status == LockStatus::Unlocked {
                    self.render_game_list();
                } else {
                    self.render_lock_screen();
                }
            }
            UiScreen::PowerOffConfirm => {
                self.render_power_off_confirm();
            }
            UiScreen::ShuttingDown => {
                self.render_shutdown_screen();
            }
            UiScreen::PinTumbler {
                digits,
                cursor,
                error_msg,
            } => {
                self.render_pin_tumbler(digits, cursor, error_msg.as_deref());
            }
            UiScreen::ParentPortal => {
                self.render_parent_portal();
            }
        }
    }

    pub fn selected_title_needs_marquee(&self) -> bool {
        if self.screen != UiScreen::MainOrLock || self.status.lock_status != LockStatus::Unlocked {
            return false;
        }
        if let Some(g) = self.status.allowed_games.get(self.selected_index) {
            let max_title_w = 348.0 - 28.0;
            let full_w = self
                .ui
                .measure_text(&g.display_title, FontKind::SansExtraBold, 21.0, 0.01);
            return full_w > max_title_w;
        }
        false
    }

}

pub fn poll_input_action(has_game_controller: bool) -> Option<InputAction> {
    let mut ev = SDL_Event { bytes: [0u8; 64] };
    while unsafe { SDL_PollEvent(&mut ev) } != 0 {
        match ev.event_type() {
            SDL_QUIT => return Some(InputAction::QuitApp),
            SDL_KEYDOWN => {
                if ev.key_repeat() != 0 {
                    continue;
                }
                match ev.keycode() {
                    SDLK_UP => return Some(InputAction::Up),
                    SDLK_DOWN => return Some(InputAction::Down),
                    SDLK_LEFT => return Some(InputAction::Left),
                    SDLK_RIGHT => return Some(InputAction::Right),
                    SDLK_RETURN | SDLK_SPACE | SDLK_Z | SDLK_A => {
                        return Some(InputAction::Confirm)
                    }
                    SDLK_BACKSPACE | SDLK_ESCAPE | SDLK_X | SDLK_B => {
                        return Some(InputAction::Back)
                    }
                    SDLK_C => return Some(InputAction::ActionX),
                    SDLK_LSHIFT | SDLK_RSHIFT | SDLK_TAB => {
                        return Some(InputAction::ParentSelect)
                    }
                    _ => {}
                }
            }
            SDL_CONTROLLERBUTTONDOWN => {
                let btn = ev.controller_button();
                eprintln!("[yasumu-input] controller_btn={}", btn);
                match btn {
                    SDL_CONTROLLER_BUTTON_DPAD_UP => return Some(InputAction::Up),
                    SDL_CONTROLLER_BUTTON_DPAD_DOWN => return Some(InputAction::Down),
                    SDL_CONTROLLER_BUTTON_DPAD_LEFT => return Some(InputAction::Left),
                    SDL_CONTROLLER_BUTTON_DPAD_RIGHT => return Some(InputAction::Right),
                    SDL_CONTROLLER_BUTTON_A => return Some(InputAction::Confirm),
                    SDL_CONTROLLER_BUTTON_B => return Some(InputAction::Back),
                    SDL_CONTROLLER_BUTTON_X => return Some(InputAction::ActionX),
                    SDL_CONTROLLER_BUTTON_Y => return Some(InputAction::ActionY),
                    SDL_CONTROLLER_BUTTON_BACK | SDL_CONTROLLER_BUTTON_GUIDE => {
                        return Some(InputAction::ParentSelect)
                    }
                    _ => {}
                }
            }
            SDL_JOYHATMOTION if !has_game_controller => {
                let val = ev.joy_hat_value();
                if (val & SDL_HAT_UP) != 0 {
                    return Some(InputAction::Up);
                }
                if (val & SDL_HAT_DOWN) != 0 {
                    return Some(InputAction::Down);
                }
                if (val & SDL_HAT_LEFT) != 0 {
                    return Some(InputAction::Left);
                }
                if (val & SDL_HAT_RIGHT) != 0 {
                    return Some(InputAction::Right);
                }
            }
            SDL_JOYBUTTONDOWN if !has_game_controller => {
                let btn = ev.controller_button();
                eprintln!("[yasumu-input] raw_joy_btn={}", btn);
                match btn {
                    2 => return Some(InputAction::Confirm),            // A on muOS-Keys
                    3 => return Some(InputAction::Back),               // B on muOS-Keys
                    4 => return Some(InputAction::ActionX),            // X on muOS-Keys
                    5 => return Some(InputAction::ActionY),            // Y on muOS-Keys
                    10 | 12 => return Some(InputAction::ParentSelect), // SELECT / MENU on muOS-Keys
                    _ => {}
                }
            }
            _ => {}
        }
    }
    None
}

/// Runs `yasumu launcher` either interactively, with `--dump-frame`, or with `--script`.
pub fn run_launcher(
    socket_path: PathBuf,
    dump_frame: Option<PathBuf>,
    script_path: Option<PathBuf>,
) -> Result<()> {
    let mut app = LauncherApp::new(socket_path.clone())?;

    if let Some(script_file) = script_path {
        let canvas = SdlCanvas::new("yasumuOS Launcher", 640, 480)?;
        run_launcher_script(&mut app, &canvas, &script_file)?;
        if let Some(frame_out) = dump_frame {
            app.render(&canvas);
            app.ui.save_png(&frame_out)?;
        }
        return Ok(());
    }

    if let Some(frame_out) = dump_frame {
        let canvas = SdlCanvas::new("yasumuOS Launcher", 640, 480)?;
        app.render(&canvas);
        app.ui.save_png(&frame_out)?;
        return Ok(());
    }

    while !app.should_exit {
        // Scope `canvas` so SDL2 releases DRM/KMS & Mali framebuffer before spawning RetroArch
        {
            let canvas = SdlCanvas::new("yasumuOS Launcher", 640, 480)?;
            let mut frame_counter = 0u32;
            let mut dirty = true;
            while !app.should_exit && app.pending_game.is_none() {
                while let Some(action) = poll_input_action(canvas.has_game_controller) {
                    app.handle_action(action);
                    dirty = true;
                }

                frame_counter = frame_counter.wrapping_add(1);
                if frame_counter % 30 == 0 {
                    app.refresh_status();
                    dirty = true;
                }
                if app.selected_title_needs_marquee() {
                    dirty = true;
                }

                if dirty {
                    app.render(&canvas);
                    dirty = false;
                }
                unsafe {
                    SDL_Delay(33);
                }
            }
        }

        if let Some(game) = app.pending_game.take() {
            let lang = Language::from_code(&app.status.language);
            execute_retroarch_game(&socket_path, &game, lang);
            app.refresh_status();
        }
    }

    if app.exit_to_muos {
        let _ = fs::write("/tmp/yasumu_exit_to_muos", "1\n");
        let stock_mux = Path::new("/opt/muos/extra/muxlaunch");
        if stock_mux.is_file() {
            let _ = Command::new(stock_mux).status();
        }
    }

    Ok(())
}


/// Executes a deterministic UI test script:
/// - `WAIT <ms>`
/// - `PRESS <UP|DOWN|LEFT|RIGHT|A|CONFIRM|B|BACK|X|SELECT|PARENT>`
/// - `SCREENSHOT <path.png|bmp>`
/// - `REFRESH`
fn run_launcher_script(
    app: &mut LauncherApp,
    canvas: &SdlCanvas,
    script_file: &Path,
) -> Result<()> {
    let raw = fs::read_to_string(script_file)
        .with_context(|| format!("read script file {}", script_file.display()))?;

    app.render(canvas);

    for (line_no, raw_line) in raw.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(2, char::is_whitespace);
        let verb = parts.next().unwrap_or("").to_ascii_uppercase();
        let arg = parts.next().unwrap_or("").trim();

        match verb.as_str() {
            "WAIT" => {
                let ms = arg.parse::<u32>().unwrap_or(10).min(2000);
                unsafe {
                    SDL_Delay(ms);
                }
                app.refresh_status();
                app.render(canvas);
            }
            "REFRESH" => {
                app.refresh_status();
                app.render(canvas);
            }
            "PRESS" => {
                let act = match arg.to_ascii_uppercase().as_str() {
                    "UP" => InputAction::Up,
                    "DOWN" => InputAction::Down,
                    "LEFT" => InputAction::Left,
                    "RIGHT" => InputAction::Right,
                    "A" | "CONFIRM" | "ENTER" => InputAction::Confirm,
                    "B" | "BACK" | "CANCEL" => InputAction::Back,
                    "X" => InputAction::ActionX,
                    "Y" => InputAction::ActionY,
                    "SELECT" | "PARENT" => InputAction::ParentSelect,
                    "QUIT" => InputAction::QuitApp,
                    other => bail!("line {}: unknown PRESS button '{}'", line_no + 1, other),
                };
                app.handle_action(act);
                app.render(canvas);
            }
            "SCREENSHOT" => {
                if arg.is_empty() {
                    bail!("line {}: SCREENSHOT requires a file path", line_no + 1);
                }
                app.render(canvas);
                app.ui.save_png(Path::new(arg))?;
            }
            other => bail!("line {}: unknown script command '{}'", line_no + 1, other),
        }
    }
    Ok(())
}
