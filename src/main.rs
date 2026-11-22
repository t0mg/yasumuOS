use anyhow::Result;
use clap::{Parser, Subcommand};
use std::net::SocketAddr;
use std::path::PathBuf;
use yasumu::daemon::{run_daemon, DaemonConfig};
use yasumu::launcher::run_launcher;
use yasumu::retroarch::run_mock_retroarch;

#[derive(Parser, Debug)]
#[command(
    name = "yasumu",
    version,
    about = "yasumuOS — Playtime Pacing & Minimalist Overlay for muOS Handhelds"
)]
struct Cli {
    /// Enable local development / CI mode (skips Wi-Fi scripts, uses local ports/paths).
    #[arg(long, global = true, default_value_t = false)]
    dev: bool,

    /// Data directory for signed config.json, usage.json, and socket.
    #[arg(long, global = true, default_value = "/opt/yasumu")]
    data_dir: PathBuf,

    /// Override Unix domain socket path (defaults to `<data_dir>/yasumu.sock` in `--dev` or `/run/yasumu.sock`).
    #[arg(long, global = true)]
    socket: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Run the background parental watchdog and captive portal daemon (`parentald`).
    Daemon {
        /// ROM root directories to scan (comma-separated or repeated).
        #[arg(long, value_delimiter = ',')]
        roms_dir: Vec<PathBuf>,

        /// Optional muOS catalogue root directories for box art discovery.
        #[arg(long, value_delimiter = ',')]
        catalogue_dir: Vec<PathBuf>,

        /// RetroArch UDP command address (`127.0.0.1:55355`).
        #[arg(long, default_value = "127.0.0.1:55355")]
        retroarch_udp: SocketAddr,

        /// HTTP bind address for Parent Portal (`0.0.0.0:80` on hardware, `127.0.0.1:8080` in `--dev`).
        #[arg(long)]
        http_addr: Option<SocketAddr>,

        /// Immediately start the Parent Portal HTTP server on daemon boot (useful for dev/testing).
        #[arg(long, default_value_t = false)]
        start_portal: bool,
    },

    /// Run the 640×480 SDL2 kids' game launcher (`kids-launcher`).
    Launcher {
        /// Render a single frame to `<path.png|bmp>` via `SDL_RenderReadPixels` and exit.
        #[arg(long)]
        dump_frame: Option<PathBuf>,

        /// Execute a deterministic synthetic UI script (`WAIT`, `PRESS`, `SCREENSHOT`) and exit.
        #[arg(long)]
        script: Option<PathBuf>,
    },

    /// Run a mock RetroArch UDP server (`127.0.0.1:55355`) that logs `SHOW_MSG` and exits on `QUIT`.
    MockRetroarch {
        #[arg(long, default_value = "127.0.0.1:55355")]
        bind: SocketAddr,

        #[arg(long)]
        log_file: Option<PathBuf>,
    },

    /// Download missing 200×200 PNG box art covers from Libretro Thumbnails into `<rom_dir>/Imgs/`.
    Scrape {
        /// ROM root directory to scan (e.g., `/run/media/tom/MustardOS/ROMS`).
        roms_dir: PathBuf,
    },

    /// Generate a 640×480 24-bit RGB Obsidian boot splash (`bootlogo.bmp` or `.png`).
    GenerateSplash {
        /// Output image path (`.bmp` for muOS U-Boot splash, or `.png` for preview).
        output: PathBuf,

        /// Centered wordmark text (defaults to `yasumuOS`).
        #[arg(long, default_value = "yasumuOS")]
        wordmark: String,
    },
}

fn resolve_socket_path(cli: &Cli) -> PathBuf {
    if let Some(s) = &cli.socket {
        s.clone()
    } else if cli.dev || cli.data_dir != PathBuf::from("/opt/yasumu") {
        cli.data_dir.join("yasumu.sock")
    } else {
        PathBuf::from("/run/yasumu.sock")
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let socket_path = resolve_socket_path(&cli);

    match cli.command {
        Commands::Daemon {
            roms_dir,
            catalogue_dir,
            retroarch_udp,
            http_addr,
            start_portal,
        } => {
            let rom_roots = if roms_dir.is_empty() {
                if cli.dev {
                    vec![cli.data_dir.join("roms")]
                } else {
                    vec![
                        PathBuf::from("/mnt/union/ROMS"),
                        PathBuf::from("/mnt/mmc/ROMS"),
                        PathBuf::from("/mnt/sdcard/ROMS"),
                    ]
                }
            } else {
                roms_dir
            };

            let portal_bind = http_addr.unwrap_or_else(|| {
                if cli.dev {
                    "127.0.0.1:8080".parse().unwrap()
                } else {
                    "0.0.0.0:80".parse().unwrap()
                }
            });

            let cfg = DaemonConfig {
                data_dir: cli.data_dir,
                socket_path,
                rom_roots,
                catalogue_roots: catalogue_dir,
                retroarch_udp,
                portal_bind,
                dev_mode: cli.dev,
                scan_proc_for_retroarch: !cli.dev,
                start_portal_on_boot: start_portal,
            };

            run_daemon(cfg, None)
        }
        Commands::Launcher { dump_frame, script } => run_launcher(socket_path, dump_frame, script),
        Commands::MockRetroarch { bind, log_file } => {
            run_mock_retroarch(bind, log_file.as_deref())
        }
        Commands::Scrape { roms_dir } => {
            let cfg = yasumu::state::ParentalConfig::default();
            let games = yasumu::roms::scan_roms(&[roms_dir], &cfg, &[]);
            println!("Scanning {} ROM(s) for missing covers...", games.len());
            let report = yasumu::scraper::scrape_missing_covers(&games);
            for line in &report.details {
                println!("  {}", line);
            }
            println!(
                "Summary: {} downloaded, {} already present, {} not found.",
                report.downloaded, report.skipped, report.failed
            );
            Ok(())
        }
        Commands::GenerateSplash { output, wordmark } => {
            let mut canvas = yasumu::ui_renderer::UiCanvas::new(640, 480);
            canvas.render_splash_screen(&wordmark);
            let is_png = output
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case("png"))
                .unwrap_or(false);
            if is_png {
                canvas.save_png(&output)?;
            } else {
                canvas.save_bmp_24bit(&output)?;
            }
            println!("Generated splash screen: {}", output.display());
            Ok(())
        }
    }
}
