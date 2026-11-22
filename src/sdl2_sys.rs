#![allow(non_camel_case_types, dead_code)]

use anyhow::{bail, Result};
use std::ffi::{c_char, c_int, c_void, CStr, CString};

pub const SDL_INIT_VIDEO: u32 = 0x0000_0020;
pub const SDL_INIT_AUDIO: u32 = 0x0000_0010;
pub const SDL_INIT_JOYSTICK: u32 = 0x0000_0200;
pub const SDL_INIT_GAMECONTROLLER: u32 = 0x0000_2000;

pub const SDL_WINDOWPOS_CENTERED: c_int = 0x2FFF_0000;
pub const SDL_WINDOW_SHOWN: u32 = 0x0000_0004;

pub const SDL_RENDERER_SOFTWARE: u32 = 0x0000_0001;
pub const SDL_RENDERER_ACCELERATED: u32 = 0x0000_0002;
pub const SDL_RENDERER_PRESENTVSYNC: u32 = 0x0000_0004;

// Little-endian ABGR8888 = [R, G, B, A] in memory bytes
pub const SDL_PIXELFORMAT_ABGR8888: u32 = 0x1676_2004;
pub const SDL_TEXTUREACCESS_STATIC: c_int = 0;
pub const SDL_TEXTUREACCESS_STREAMING: c_int = 1;

pub const SDL_QUIT: u32 = 0x100;
pub const SDL_KEYDOWN: u32 = 0x300;
pub const SDL_JOYHATMOTION: u32 = 0x602;
pub const SDL_JOYBUTTONDOWN: u32 = 0x603;
pub const SDL_CONTROLLERBUTTONDOWN: u32 = 0x651;

// SDL_Keycode constants
pub const SDLK_RETURN: i32 = 13;
pub const SDLK_ESCAPE: i32 = 27;
pub const SDLK_BACKSPACE: i32 = 8;
pub const SDLK_TAB: i32 = 9;
pub const SDLK_SPACE: i32 = 32;
pub const SDLK_Z: i32 = b'z' as i32;
pub const SDLK_X: i32 = b'x' as i32;
pub const SDLK_C: i32 = b'c' as i32;
pub const SDLK_A: i32 = b'a' as i32;
pub const SDLK_B: i32 = b'b' as i32;
pub const SDLK_RIGHT: i32 = (1 << 30) | 79;
pub const SDLK_LEFT: i32 = (1 << 30) | 80;
pub const SDLK_DOWN: i32 = (1 << 30) | 81;
pub const SDLK_UP: i32 = (1 << 30) | 82;
pub const SDLK_LSHIFT: i32 = (1 << 30) | 225;
pub const SDLK_RSHIFT: i32 = (1 << 30) | 229;

// SDL_GameControllerButton constants
pub const SDL_CONTROLLER_BUTTON_A: u8 = 0;
pub const SDL_CONTROLLER_BUTTON_B: u8 = 1;
pub const SDL_CONTROLLER_BUTTON_X: u8 = 2;
pub const SDL_CONTROLLER_BUTTON_Y: u8 = 3;
pub const SDL_CONTROLLER_BUTTON_BACK: u8 = 4; // Select
pub const SDL_CONTROLLER_BUTTON_GUIDE: u8 = 5;
pub const SDL_CONTROLLER_BUTTON_START: u8 = 6;
pub const SDL_CONTROLLER_BUTTON_DPAD_UP: u8 = 11;
pub const SDL_CONTROLLER_BUTTON_DPAD_DOWN: u8 = 12;
pub const SDL_CONTROLLER_BUTTON_DPAD_LEFT: u8 = 13;
pub const SDL_CONTROLLER_BUTTON_DPAD_RIGHT: u8 = 14;

pub const SDL_HAT_UP: u8 = 0x01;
pub const SDL_HAT_RIGHT: u8 = 0x02;
pub const SDL_HAT_DOWN: u8 = 0x04;
pub const SDL_HAT_LEFT: u8 = 0x08;

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SDL_Rect {
    pub x: c_int,
    pub y: c_int,
    pub w: c_int,
    pub h: c_int,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct SDL_Event {
    pub bytes: [u8; 64],
}

impl SDL_Event {
    pub fn event_type(&self) -> u32 {
        u32::from_ne_bytes([self.bytes[0], self.bytes[1], self.bytes[2], self.bytes[3]])
    }

    pub fn key_repeat(&self) -> u8 {
        self.bytes[13]
    }

    pub fn keycode(&self) -> i32 {
        i32::from_ne_bytes([
            self.bytes[20],
            self.bytes[21],
            self.bytes[22],
            self.bytes[23],
        ])
    }

    pub fn controller_button(&self) -> u8 {
        self.bytes[12]
    }

    pub fn joy_hat_value(&self) -> u8 {
        self.bytes[13]
    }
}

extern "C" {
    pub fn SDL_Init(flags: u32) -> c_int;
    pub fn SDL_Quit();
    pub fn SDL_GetError() -> *const c_char;
    pub fn SDL_CreateWindow(
        title: *const c_char,
        x: c_int,
        y: c_int,
        w: c_int,
        h: c_int,
        flags: u32,
    ) -> *mut c_void;
    pub fn SDL_DestroyWindow(window: *mut c_void);
    pub fn SDL_CreateRenderer(window: *mut c_void, index: c_int, flags: u32) -> *mut c_void;
    pub fn SDL_DestroyRenderer(renderer: *mut c_void);
    pub fn SDL_RenderSetLogicalSize(renderer: *mut c_void, w: c_int, h: c_int) -> c_int;
    pub fn SDL_SetRenderDrawColor(
        renderer: *mut c_void,
        r: u8,
        g: u8,
        b: u8,
        a: u8,
    ) -> c_int;
    pub fn SDL_RenderClear(renderer: *mut c_void) -> c_int;
    pub fn SDL_RenderFillRect(renderer: *mut c_void, rect: *const SDL_Rect) -> c_int;
    pub fn SDL_RenderDrawRect(renderer: *mut c_void, rect: *const SDL_Rect) -> c_int;
    pub fn SDL_RenderPresent(renderer: *mut c_void);
    pub fn SDL_RenderReadPixels(
        renderer: *mut c_void,
        rect: *const SDL_Rect,
        format: u32,
        pixels: *mut c_void,
        pitch: c_int,
    ) -> c_int;
    pub fn SDL_CreateTexture(
        renderer: *mut c_void,
        format: u32,
        access: c_int,
        w: c_int,
        h: c_int,
    ) -> *mut c_void;
    pub fn SDL_UpdateTexture(
        texture: *mut c_void,
        rect: *const SDL_Rect,
        pixels: *const c_void,
        pitch: c_int,
    ) -> c_int;
    pub fn SDL_RenderCopy(
        renderer: *mut c_void,
        texture: *mut c_void,
        srcrect: *const SDL_Rect,
        dstrect: *const SDL_Rect,
    ) -> c_int;
    pub fn SDL_DestroyTexture(texture: *mut c_void);
    pub fn SDL_PollEvent(event: *mut SDL_Event) -> c_int;
    pub fn SDL_Delay(ms: u32);
    pub fn SDL_NumJoysticks() -> c_int;
    pub fn SDL_IsGameController(joystick_index: c_int) -> c_int;
    pub fn SDL_GameControllerOpen(joystick_index: c_int) -> *mut c_void;
    pub fn SDL_GameControllerAddMapping(mapping_string: *const c_char) -> c_int;
    pub fn SDL_JoystickOpen(device_index: c_int) -> *mut c_void;
}

pub fn sdl_error() -> String {
    unsafe {
        let ptr = SDL_GetError();
        if ptr.is_null() {
            "unknown SDL error".to_string()
        } else {
            CStr::from_ptr(ptr).to_string_lossy().into_owned()
        }
    }
}

/// Safe wrapper around an SDL2 `640×480` window and renderer.
pub struct SdlCanvas {
    pub window: *mut c_void,
    pub renderer: *mut c_void,
    pub frame_texture: *mut c_void,
    pub width: i32,
    pub height: i32,
    pub has_game_controller: bool,
}

fn load_controller_mappings() {
    // Built-in RG35XXSP / MustardOS `muOS-Keys` mapping
    let builtin_mappings = [
        "060000006d750000090700006f730000,muOS-Keys,a:b2,b:b3,x:b4,y:b5,leftshoulder:b6,rightshoulder:b7,lefttrigger:b8,righttrigger:b9,guide:b12,start:b11,back:b10,dpup:h0.1,dpleft:h0.8,dpright:h0.2,dpdown:h0.4,leftx:a0,lefty:a1,platform:Linux,",
    ];
    for m in builtin_mappings {
        if let Ok(cm) = CString::new(m) {
            unsafe {
                let _ = SDL_GameControllerAddMapping(cm.as_ptr());
            }
        }
    }

    // Also load MustardOS's runtime gamecontrollerdb.txt if present
    let db_paths = [
        "/usr/lib/gamecontrollerdb.txt",
        "/opt/muos/device/control/gamecontrollerdb/retro.txt",
        "/opt/muos/share/info/gamecontrollerdb/retro.txt",
    ];
    for p in db_paths {
        if let Ok(contents) = std::fs::read_to_string(p) {
            for line in contents.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    continue;
                }
                if let Ok(cm) = CString::new(trimmed) {
                    unsafe {
                        let _ = SDL_GameControllerAddMapping(cm.as_ptr());
                    }
                }
            }
        }
    }
}

impl SdlCanvas {
    pub fn new(title: &str, width: i32, height: i32) -> Result<Self> {
        unsafe {
            if SDL_Init(SDL_INIT_VIDEO | SDL_INIT_JOYSTICK | SDL_INIT_GAMECONTROLLER) != 0 {
                // Retry with video only in headless environments without udev/input devices
                if SDL_Init(SDL_INIT_VIDEO) != 0 {
                    bail!("SDL_Init failed: {}", sdl_error());
                }
            }

            load_controller_mappings();

            // Open any connected game controllers / joysticks (e.g. RG35XXSP muOS-Keys)
            let mut has_game_controller = false;
            let num_joy = SDL_NumJoysticks();
            eprintln!("[yasumu-launcher] SDL_NumJoysticks={}", num_joy);
            for i in 0..num_joy {
                let is_gc = SDL_IsGameController(i) != 0;
                eprintln!("[yasumu-launcher] joystick[{}] is_game_controller={}", i, is_gc);
                if is_gc {
                    if !SDL_GameControllerOpen(i).is_null() {
                        has_game_controller = true;
                    }
                } else {
                    let _ = SDL_JoystickOpen(i);
                }
            }

            let c_title = CString::new(title).unwrap();
            let window = SDL_CreateWindow(
                c_title.as_ptr(),
                SDL_WINDOWPOS_CENTERED,
                SDL_WINDOWPOS_CENTERED,
                width,
                height,
                SDL_WINDOW_SHOWN,
            );
            if window.is_null() {
                bail!("SDL_CreateWindow failed: {}", sdl_error());
            }

            // Try accelerated renderer first, fall back to software renderer for SDL_VIDEODRIVER=dummy
            let mut renderer = SDL_CreateRenderer(
                window,
                -1,
                SDL_RENDERER_ACCELERATED | SDL_RENDERER_PRESENTVSYNC,
            );
            if renderer.is_null() {
                renderer = SDL_CreateRenderer(window, -1, SDL_RENDERER_SOFTWARE);
            }
            if renderer.is_null() {
                SDL_DestroyWindow(window);
                bail!("SDL_CreateRenderer failed: {}", sdl_error());
            }

            SDL_RenderSetLogicalSize(renderer, width, height);

            let frame_texture = SDL_CreateTexture(
                renderer,
                SDL_PIXELFORMAT_ABGR8888,
                SDL_TEXTUREACCESS_STREAMING,
                width,
                height,
            );

            Ok(Self {
                window,
                renderer,
                frame_texture,
                width,
                height,
                has_game_controller,
            })
        }
    }

    /// Uploads a full `width × height` RGBA8888 software-rendered frame to SDL2 and presents it.
    pub fn present_rgba(&self, rgba: &[u8]) {
        if self.frame_texture.is_null() {
            return;
        }
        unsafe {
            let pitch = self.width * 4;
            if SDL_UpdateTexture(
                self.frame_texture,
                std::ptr::null(),
                rgba.as_ptr() as *const c_void,
                pitch,
            ) == 0
            {
                let _ = SDL_RenderCopy(
                    self.renderer,
                    self.frame_texture,
                    std::ptr::null(),
                    std::ptr::null(),
                );
                SDL_RenderPresent(self.renderer);
            }
        }
    }

    pub fn set_color(&self, r: u8, g: u8, b: u8) {
        unsafe {
            SDL_SetRenderDrawColor(self.renderer, r, g, b, 255);
        }
    }

    pub fn clear(&self, r: u8, g: u8, b: u8) {
        self.set_color(r, g, b);
        unsafe {
            SDL_RenderClear(self.renderer);
        }
    }

    pub fn fill_rect(&self, x: i32, y: i32, w: i32, h: i32, (r, g, b): (u8, u8, u8)) {
        if w <= 0 || h <= 0 {
            return;
        }
        self.set_color(r, g, b);
        let rect = SDL_Rect { x, y, w, h };
        unsafe {
            SDL_RenderFillRect(self.renderer, &rect);
        }
    }

    pub fn draw_rect(&self, x: i32, y: i32, w: i32, h: i32, (r, g, b): (u8, u8, u8)) {
        if w <= 0 || h <= 0 {
            return;
        }
        self.set_color(r, g, b);
        let rect = SDL_Rect { x, y, w, h };
        unsafe {
            SDL_RenderDrawRect(self.renderer, &rect);
        }
    }

    pub fn present(&self) {
        unsafe {
            SDL_RenderPresent(self.renderer);
        }
    }

    /// Reads the rendered 640×480 frame buffer as RGBA8888 bytes (`width * height * 4`).
    pub fn read_rgba_pixels(&self) -> Result<Vec<u8>> {
        let mut buf = vec![0u8; (self.width * self.height * 4) as usize];
        let pitch = self.width * 4;
        let rc = unsafe {
            SDL_RenderReadPixels(
                self.renderer,
                std::ptr::null(),
                SDL_PIXELFORMAT_ABGR8888,
                buf.as_mut_ptr() as *mut c_void,
                pitch,
            )
        };
        if rc != 0 {
            bail!("SDL_RenderReadPixels failed: {}", sdl_error());
        }
        Ok(buf)
    }

    /// Saves the current rendered frame to `.png` or `.bmp`.
    pub fn save_screenshot(&self, path: &std::path::Path) -> Result<()> {
        let pixels = self.read_rgba_pixels()?;
        let img = image::RgbaImage::from_raw(self.width as u32, self.height as u32, pixels)
            .ok_or_else(|| anyhow::anyhow!("create RgbaImage buffer"))?;
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        img.save(path)?;
        Ok(())
    }

    /// Draws an RGBA image buffer at `(dst_x, dst_y, dst_w, dst_h)` using a transient SDL_Texture.
    pub fn draw_rgba_image(
        &self,
        rgba: &[u8],
        img_w: u32,
        img_h: u32,
        dst_x: i32,
        dst_y: i32,
        dst_w: i32,
        dst_h: i32,
    ) {
        unsafe {
            let tex = SDL_CreateTexture(
                self.renderer,
                SDL_PIXELFORMAT_ABGR8888,
                SDL_TEXTUREACCESS_STATIC,
                img_w as c_int,
                img_h as c_int,
            );
            if tex.is_null() {
                return;
            }
            let pitch = (img_w * 4) as c_int;
            if SDL_UpdateTexture(tex, std::ptr::null(), rgba.as_ptr() as *const c_void, pitch) == 0
            {
                let dst = SDL_Rect {
                    x: dst_x,
                    y: dst_y,
                    w: dst_w,
                    h: dst_h,
                };
                let _ = SDL_RenderCopy(self.renderer, tex, std::ptr::null(), &dst);
            }
            SDL_DestroyTexture(tex);
        }
    }

    /// Draws crisp bitmap text using the built-in 8×8 ASCII font at integer scale (`1`, `2`, `3`, etc.).
    pub fn draw_text(
        &self,
        text: &str,
        mut x: i32,
        y: i32,
        scale: i32,
        color: (u8, u8, u8),
    ) {
        self.set_color(color.0, color.1, color.2);
        let scale = scale.max(1);
        for ch in text.chars() {
            let glyph = glyph_8x8(ch);
            for (row_idx, &row_bits) in glyph.iter().enumerate() {
                if row_bits == 0 {
                    continue;
                }
                for col_idx in 0..8 {
                    if ((row_bits >> (7 - col_idx)) & 1) != 0 {
                        let rect = SDL_Rect {
                            x: x + (col_idx * scale),
                            y: y + (row_idx as i32 * scale),
                            w: scale,
                            h: scale,
                        };
                        unsafe {
                            SDL_RenderFillRect(self.renderer, &rect);
                        }
                    }
                }
            }
            x += 8 * scale;
        }
    }

    pub fn draw_text_centered(
        &self,
        text: &str,
        center_x: i32,
        y: i32,
        scale: i32,
        color: (u8, u8, u8),
    ) {
        let width = (text.chars().count() as i32) * 8 * scale.max(1);
        self.draw_text(text, center_x - width / 2, y, scale, color);
    }
}

impl Drop for SdlCanvas {
    fn drop(&mut self) {
        unsafe {
            if !self.frame_texture.is_null() {
                SDL_DestroyTexture(self.frame_texture);
            }
            if !self.renderer.is_null() {
                SDL_DestroyRenderer(self.renderer);
            }
            if !self.window.is_null() {
                SDL_DestroyWindow(self.window);
            }
            SDL_Quit();
        }
    }
}

/// Standard 8×8 pixel font covering printable ASCII `0x20..=0x7E` plus common Latin-1 accented characters.
fn glyph_8x8(ch: char) -> [u8; 8] {
    match ch {
        'é' => [0x0C, 0x18, 0x3C, 0x66, 0x7E, 0x60, 0x3C, 0x00],
        'è' => [0x30, 0x18, 0x3C, 0x66, 0x7E, 0x60, 0x3C, 0x00],
        'ê' => [0x18, 0x24, 0x3C, 0x66, 0x7E, 0x60, 0x3C, 0x00],
        'ë' => [0x24, 0x00, 0x3C, 0x66, 0x7E, 0x60, 0x3C, 0x00],
        'à' => [0x30, 0x18, 0x3C, 0x06, 0x3E, 0x66, 0x3E, 0x00],
        'á' => [0x0C, 0x18, 0x3C, 0x06, 0x3E, 0x66, 0x3E, 0x00],
        'â' => [0x18, 0x24, 0x3C, 0x06, 0x3E, 0x66, 0x3E, 0x00],
        'ä' => [0x24, 0x00, 0x3C, 0x06, 0x3E, 0x66, 0x3E, 0x00],
        'ç' => [0x00, 0x3C, 0x66, 0x60, 0x66, 0x3C, 0x18, 0x30],
        'í' => [0x0C, 0x18, 0x38, 0x18, 0x18, 0x18, 0x3C, 0x00],
        'î' => [0x18, 0x24, 0x38, 0x18, 0x18, 0x18, 0x3C, 0x00],
        'ó' => [0x0C, 0x18, 0x3C, 0x66, 0x66, 0x66, 0x3C, 0x00],
        'ô' => [0x18, 0x24, 0x3C, 0x66, 0x66, 0x66, 0x3C, 0x00],
        'ö' => [0x24, 0x00, 0x3C, 0x66, 0x66, 0x66, 0x3C, 0x00],
        'ù' => [0x30, 0x18, 0x66, 0x66, 0x66, 0x66, 0x3E, 0x00],
        'ú' => [0x0C, 0x18, 0x66, 0x66, 0x66, 0x66, 0x3E, 0x00],
        'û' => [0x18, 0x24, 0x66, 0x66, 0x66, 0x66, 0x3E, 0x00],
        'ü' => [0x24, 0x00, 0x66, 0x66, 0x66, 0x66, 0x3E, 0x00],
        'ñ' => [0x36, 0x6C, 0x00, 0x7C, 0x66, 0x66, 0x66, 0x00],
        'ß' => [0x3C, 0x66, 0x66, 0x7C, 0x66, 0x66, 0x7C, 0x60],
        'É' => [0x0C, 0x18, 0x7E, 0x60, 0x7C, 0x60, 0x7E, 0x00],
        'È' => [0x30, 0x18, 0x7E, 0x60, 0x7C, 0x60, 0x7E, 0x00],
        'Ê' => [0x18, 0x24, 0x7E, 0x60, 0x7C, 0x60, 0x7E, 0x00],
        'À' => [0x30, 0x18, 0x3C, 0x66, 0x7E, 0x66, 0x66, 0x00],
        'Ä' => [0x24, 0x00, 0x3C, 0x66, 0x7E, 0x66, 0x66, 0x00],
        'Ö' => [0x24, 0x00, 0x3C, 0x66, 0x66, 0x66, 0x3C, 0x00],
        'Ü' => [0x24, 0x00, 0x66, 0x66, 0x66, 0x66, 0x3C, 0x00],
        'Ç' => [0x3C, 0x66, 0x60, 0x60, 0x66, 0x3C, 0x18, 0x30],
        'Ñ' => [0x36, 0x6C, 0xC6, 0xE6, 0xF6, 0xDE, 0xCE, 0x00],
        '¡' => [0x18, 0x00, 0x18, 0x18, 0x3C, 0x3C, 0x18, 0x00],
        '¿' => [0x18, 0x00, 0x18, 0x30, 0x60, 0x66, 0x3C, 0x00],
        _ => {
            let idx = if (ch as u32) >= 0x20 && (ch as u32) <= 0x7E {
                (ch as usize) - 0x20
            } else {
                ('?' as usize) - 0x20
            };
            FONT_8X8[idx]
        }
    }
}

const FONT_8X8: [[u8; 8]; 95] = [
    [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00], // ' '
    [0x18, 0x3C, 0x3C, 0x18, 0x18, 0x00, 0x18, 0x00], // '!'
    [0x6C, 0x6C, 0x48, 0x00, 0x00, 0x00, 0x00, 0x00], // '"'
    [0x6C, 0x6C, 0xFE, 0x6C, 0xFE, 0x6C, 0x6C, 0x00], // '#'
    [0x18, 0x3E, 0x60, 0x3C, 0x06, 0x7C, 0x18, 0x00], // '$'
    [0x00, 0xC6, 0xCC, 0x18, 0x30, 0x66, 0xC6, 0x00], // '%'
    [0x38, 0x6C, 0x38, 0x76, 0xDC, 0xCC, 0x76, 0x00], // '&'
    [0x18, 0x18, 0x30, 0x00, 0x00, 0x00, 0x00, 0x00], // '\''
    [0x0C, 0x18, 0x30, 0x30, 0x30, 0x18, 0x0C, 0x00], // '('
    [0x30, 0x18, 0x0C, 0x0C, 0x0C, 0x18, 0x30, 0x00], // ')'
    [0x00, 0x66, 0x3C, 0xFF, 0x3C, 0x66, 0x00, 0x00], // '*'
    [0x00, 0x18, 0x18, 0x7E, 0x18, 0x18, 0x00, 0x00], // '+'
    [0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0x18, 0x30], // ','
    [0x00, 0x00, 0x00, 0x7E, 0x00, 0x00, 0x00, 0x00], // '-'
    [0x00, 0x00, 0x00, 0x00, 0x00, 0x18, 0x18, 0x00], // '.'
    [0x06, 0x0C, 0x18, 0x30, 0x60, 0xC0, 0x80, 0x00], // '/'
    [0x3C, 0x66, 0x6E, 0x76, 0x66, 0x66, 0x3C, 0x00], // '0'
    [0x18, 0x38, 0x18, 0x18, 0x18, 0x18, 0x7E, 0x00], // '1'
    [0x3C, 0x66, 0x06, 0x1C, 0x30, 0x66, 0x7E, 0x00], // '2'
    [0x3C, 0x66, 0x06, 0x1C, 0x06, 0x66, 0x3C, 0x00], // '3'
    [0x1C, 0x3C, 0x6C, 0xCC, 0xFE, 0x0C, 0x1E, 0x00], // '4'
    [0x7E, 0x60, 0x7C, 0x06, 0x06, 0x66, 0x3C, 0x00], // '5'
    [0x1C, 0x30, 0x60, 0x7C, 0x66, 0x66, 0x3C, 0x00], // '6'
    [0x7E, 0x66, 0x06, 0x0C, 0x18, 0x18, 0x18, 0x00], // '7'
    [0x3C, 0x66, 0x66, 0x3C, 0x66, 0x66, 0x3C, 0x00], // '8'
    [0x3C, 0x66, 0x66, 0x3E, 0x06, 0x0C, 0x38, 0x00], // '9'
    [0x00, 0x18, 0x18, 0x00, 0x00, 0x18, 0x18, 0x00], // ':'
    [0x00, 0x18, 0x18, 0x00, 0x00, 0x18, 0x18, 0x30], // ';'
    [0x0C, 0x18, 0x30, 0x60, 0x30, 0x18, 0x0C, 0x00], // '<'
    [0x00, 0x00, 0x7E, 0x00, 0x7E, 0x00, 0x00, 0x00], // '='
    [0x30, 0x18, 0x0C, 0x06, 0x0C, 0x18, 0x30, 0x00], // '>'
    [0x3C, 0x66, 0x06, 0x0C, 0x18, 0x00, 0x18, 0x00], // '?'
    [0x3E, 0x63, 0x7B, 0x7B, 0x7B, 0x60, 0x3E, 0x00], // '@'
    [0x18, 0x3C, 0x66, 0x66, 0x7E, 0x66, 0x66, 0x00], // 'A'
    [0x7C, 0x66, 0x66, 0x7C, 0x66, 0x66, 0x7C, 0x00], // 'B'
    [0x3C, 0x66, 0x60, 0x60, 0x60, 0x66, 0x3C, 0x00], // 'C'
    [0x78, 0x6C, 0x66, 0x66, 0x66, 0x6C, 0x78, 0x00], // 'D'
    [0x7E, 0x60, 0x60, 0x7C, 0x60, 0x60, 0x7E, 0x00], // 'E'
    [0x7E, 0x60, 0x60, 0x7C, 0x60, 0x60, 0x60, 0x00], // 'F'
    [0x3C, 0x66, 0x60, 0x6E, 0x66, 0x66, 0x3E, 0x00], // 'G'
    [0x66, 0x66, 0x66, 0x7E, 0x66, 0x66, 0x66, 0x00], // 'H'
    [0x3C, 0x18, 0x18, 0x18, 0x18, 0x18, 0x3C, 0x00], // 'I'
    [0x1E, 0x0C, 0x0C, 0x0C, 0xCC, 0xCC, 0x78, 0x00], // 'J'
    [0x66, 0x6C, 0x78, 0x70, 0x78, 0x6C, 0x66, 0x00], // 'K'
    [0x60, 0x60, 0x60, 0x60, 0x60, 0x60, 0x7E, 0x00], // 'L'
    [0xC6, 0xEE, 0xFE, 0xD6, 0xC6, 0xC6, 0xC6, 0x00], // 'M'
    [0xC6, 0xE6, 0xF6, 0xDE, 0xCE, 0xC6, 0xC6, 0x00], // 'N'
    [0x3C, 0x66, 0x66, 0x66, 0x66, 0x66, 0x3C, 0x00], // 'O'
    [0x7C, 0x66, 0x66, 0x7C, 0x60, 0x60, 0x60, 0x00], // 'P'
    [0x3C, 0x66, 0x66, 0x66, 0x6A, 0x6C, 0x36, 0x00], // 'Q'
    [0x7C, 0x66, 0x66, 0x7C, 0x6C, 0x66, 0x66, 0x00], // 'R'
    [0x3C, 0x66, 0x60, 0x3C, 0x06, 0x66, 0x3C, 0x00], // 'S'
    [0x7E, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x00], // 'T'
    [0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x3C, 0x00], // 'U'
    [0x66, 0x66, 0x66, 0x66, 0x66, 0x3C, 0x18, 0x00], // 'V'
    [0xC6, 0xC6, 0xC6, 0xD6, 0xFE, 0xEE, 0xC6, 0x00], // 'W'
    [0x66, 0x66, 0x3C, 0x18, 0x3C, 0x66, 0x66, 0x00], // 'X'
    [0x66, 0x66, 0x66, 0x3C, 0x18, 0x18, 0x18, 0x00], // 'Y'
    [0x7E, 0x06, 0x0C, 0x18, 0x30, 0x60, 0x7E, 0x00], // 'Z'
    [0x3C, 0x30, 0x30, 0x30, 0x30, 0x30, 0x3C, 0x00], // '['
    [0xC0, 0x60, 0x30, 0x18, 0x0C, 0x06, 0x02, 0x00], // '\\'
    [0x3C, 0x0C, 0x0C, 0x0C, 0x0C, 0x0C, 0x3C, 0x00], // ']'
    [0x10, 0x38, 0x6C, 0xC6, 0x00, 0x00, 0x00, 0x00], // '^'
    [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF], // '_'
    [0x30, 0x18, 0x0C, 0x00, 0x00, 0x00, 0x00, 0x00], // '`'
    [0x00, 0x00, 0x3C, 0x06, 0x3E, 0x66, 0x3E, 0x00], // 'a'
    [0x60, 0x60, 0x7C, 0x66, 0x66, 0x66, 0x7C, 0x00], // 'b'
    [0x00, 0x00, 0x3C, 0x66, 0x60, 0x66, 0x3C, 0x00], // 'c'
    [0x06, 0x06, 0x3E, 0x66, 0x66, 0x66, 0x3E, 0x00], // 'd'
    [0x00, 0x00, 0x3C, 0x66, 0x7E, 0x60, 0x3C, 0x00], // 'e'
    [0x1C, 0x30, 0x7C, 0x30, 0x30, 0x30, 0x30, 0x00], // 'f'
    [0x00, 0x00, 0x3E, 0x66, 0x66, 0x3E, 0x06, 0x3C], // 'g'
    [0x60, 0x60, 0x7C, 0x66, 0x66, 0x66, 0x66, 0x00], // 'h'
    [0x18, 0x00, 0x38, 0x18, 0x18, 0x18, 0x3C, 0x00], // 'i'
    [0x06, 0x00, 0x0E, 0x06, 0x06, 0x66, 0x66, 0x3C], // 'j'
    [0x60, 0x60, 0x66, 0x6C, 0x78, 0x6C, 0x66, 0x00], // 'k'
    [0x38, 0x18, 0x18, 0x18, 0x18, 0x18, 0x3C, 0x00], // 'l'
    [0x00, 0x00, 0xEC, 0xFE, 0xD6, 0xC6, 0xC6, 0x00], // 'm'
    [0x00, 0x00, 0x7C, 0x66, 0x66, 0x66, 0x66, 0x00], // 'n'
    [0x00, 0x00, 0x3C, 0x66, 0x66, 0x66, 0x3C, 0x00], // 'o'
    [0x00, 0x00, 0x7C, 0x66, 0x66, 0x7C, 0x60, 0x60], // 'p'
    [0x00, 0x00, 0x3E, 0x66, 0x66, 0x3E, 0x06, 0x06], // 'q'
    [0x00, 0x00, 0x7C, 0x66, 0x60, 0x60, 0x60, 0x00], // 'r'
    [0x00, 0x00, 0x3E, 0x60, 0x3C, 0x06, 0x7C, 0x00], // 's'
    [0x30, 0x30, 0x7C, 0x30, 0x30, 0x36, 0x1C, 0x00], // 't'
    [0x00, 0x00, 0x66, 0x66, 0x66, 0x66, 0x3E, 0x00], // 'u'
    [0x00, 0x00, 0x66, 0x66, 0x66, 0x3C, 0x18, 0x00], // 'v'
    [0x00, 0x00, 0xC6, 0xC6, 0xD6, 0xFE, 0x6C, 0x00], // 'w'
    [0x00, 0x00, 0x66, 0x3C, 0x18, 0x3C, 0x66, 0x00], // 'x'
    [0x00, 0x00, 0x66, 0x66, 0x66, 0x3E, 0x06, 0x3C], // 'y'
    [0x00, 0x00, 0x7E, 0x0C, 0x18, 0x30, 0x7E, 0x00], // 'z'
    [0x0E, 0x18, 0x18, 0x70, 0x18, 0x18, 0x0E, 0x00], // '{'
    [0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x00], // '|'
    [0x70, 0x18, 0x18, 0x0E, 0x18, 0x18, 0x70, 0x00], // '}'
    [0x76, 0xDC, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00], // '~'
];
