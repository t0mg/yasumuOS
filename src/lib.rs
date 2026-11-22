pub const APP_VERSION: &str = env!("YASUMU_VERSION");

pub mod crypto;
pub mod daemon;
pub mod i18n;
pub mod ipc;
pub mod launcher;
pub mod portal;
pub mod qrcode;
pub mod retroarch;
pub mod roms;
pub mod scraper;
pub mod sdl2_sys;
pub mod state;
pub mod time_provider;
pub mod ui_renderer;
