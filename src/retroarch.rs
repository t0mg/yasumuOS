use anyhow::{Context, Result};
use std::fs;
use std::net::{SocketAddr, UdpSocket};
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ChimeKind {
    Warn5m,
    Warn1m,
}

/// Synthesizes a clean 44.1 kHz 16-bit mono PCM WAV notification chime in memory.
/// Uses a warm sine + subtle 2nd harmonic with smooth attack and exponential bell decay.
pub fn synthesize_chime_wav(kind: ChimeKind) -> Vec<u8> {
    const SAMPLE_RATE: u32 = 44_100;
    // (frequency_hz, duration_ms)
    let notes: &[(f32, u32)] = match kind {
        ChimeKind::Warn5m => &[(659.25, 160), (880.0, 280)],
        ChimeKind::Warn1m => &[(880.0, 120), (659.25, 120), (880.0, 280)],
    };

    let total_samples: usize = notes
        .iter()
        .map(|(_, ms)| ((SAMPLE_RATE as u64 * (*ms as u64)) / 1000) as usize)
        .sum();
    let data_bytes = (total_samples * 2) as u32;
    let mut wav = Vec::with_capacity(44 + data_bytes as usize);

    // RIFF / WAVE PCM header
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes()); // PCM chunk size
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM format
    wav.extend_from_slice(&1u16.to_le_bytes()); // mono
    wav.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    wav.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes()); // byte rate
    wav.extend_from_slice(&2u16.to_le_bytes()); // block align
    wav.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_bytes.to_le_bytes());

    let two_pi = std::f32::consts::TAU;
    for &(freq, ms) in notes {
        let count = ((SAMPLE_RATE as u64 * (ms as u64)) / 1000) as usize;
        let dur_s = (ms as f32) / 1000.0;
        let attack_s = 0.008f32;
        for i in 0..count {
            let t = (i as f32) / (SAMPLE_RATE as f32);
            let attack = (t / attack_s).min(1.0);
            let decay = (-4.2 * (t / dur_s)).exp();
            let env = attack * decay;
            let wave = (two_pi * freq * t).sin() * 0.82 + (two_pi * freq * 2.0 * t).sin() * 0.18;
            let sample = (wave * env * 0.42 * (i16::MAX as f32))
                .clamp(i16::MIN as f32, i16::MAX as f32) as i16;
            wav.extend_from_slice(&sample.to_le_bytes());
        }
    }

    wav
}

/// Plays a short notification chime asynchronously via PipeWire (`pw-play`) with `paplay`/`aplay` fallback,
/// mixing cleanly on top of RetroArch's active game audio.
pub fn play_warning_chime_async(kind: ChimeKind) {
    std::thread::spawn(move || {
        let path = match kind {
            ChimeKind::Warn5m => "/tmp/yasumu-chime-5m.wav",
            ChimeKind::Warn1m => "/tmp/yasumu-chime-1m.wav",
        };
        if !Path::new(path).is_file() {
            let wav = synthesize_chime_wav(kind);
            if fs::write(path, wav).is_err() {
                return;
            }
        }
        // Try PipeWire first (native mixer on muOS), then PulseAudio bridge, then ALSA default
        for (bin, args) in [
            ("pw-play", vec![path]),
            ("paplay", vec![path]),
            ("aplay", vec!["-q", path]),
        ] {
            if let Ok(st) = Command::new(bin).args(&args).status() {
                if st.success() {
                    return;
                }
            }
        }
    });
}

/// Client for RetroArch's local UDP network command interface (`127.0.0.1:55355`).
#[derive(Debug, Clone)]
pub struct RetroArchController {
    pub udp_target: SocketAddr,
    pub tracked_pid: Arc<Mutex<Option<u32>>>,
    pub scan_proc: bool,
}

impl Default for RetroArchController {
    fn default() -> Self {
        Self::new("127.0.0.1:55355".parse().unwrap(), true)
    }
}

impl RetroArchController {
    pub fn new(udp_target: SocketAddr, scan_proc: bool) -> Self {
        Self {
            udp_target,
            tracked_pid: Arc::new(Mutex::new(None)),
            scan_proc,
        }
    }

    pub fn set_tracked_pid(&self, pid: Option<u32>) {
        *self.tracked_pid.lock().unwrap() = pid;
    }

    /// Checks whether RetroArch (or a tracked game process) is currently alive.
    pub fn is_retroarch_running(&self) -> bool {
        if let Some(pid) = *self.tracked_pid.lock().unwrap() {
            if is_pid_alive(pid) {
                return true;
            } else {
                *self.tracked_pid.lock().unwrap() = None;
            }
        }

        if self.scan_proc {
            return !find_retroarch_pids().is_empty();
        }
        false
    }

    /// Sends raw command text over UDP to RetroArch (`127.0.0.1:55355`).
    pub fn send_udp_command(&self, cmd: &str) -> Result<()> {
        let sock = UdpSocket::bind("127.0.0.1:0").context("bind ephemeral UDP socket")?;
        sock.set_write_timeout(Some(Duration::from_millis(250)))?;
        let payload = if cmd.ends_with('\n') {
            cmd.to_string()
        } else {
            format!("{}\n", cmd)
        };
        sock.send_to(payload.as_bytes(), self.udp_target)
            .with_context(|| format!("send UDP command to {}", self.udp_target))?;
        Ok(())
    }

    /// Displays an in-game OSD notification via RetroArch UDP `SHOW_MSG`.
    pub fn show_message(&self, message: &str) -> Result<()> {
        self.send_udp_command(&format!("SHOW_MSG {}", message))
    }

    /// Requests RetroArch to auto-save and exit cleanly via UDP `QUIT`, with optional `SIGTERM` fallback.
    pub fn quit_retroarch(&self, fallback_sigterm: bool) -> Result<()> {
        let _ = self.send_udp_command("QUIT");
        if fallback_sigterm {
            let pid_opt = *self.tracked_pid.lock().unwrap();
            if let Some(pid) = pid_opt {
                if is_pid_alive(pid) {
                    unsafe {
                        libc::kill(pid as i32, libc::SIGTERM);
                    }
                }
            } else if self.scan_proc {
                for pid in find_retroarch_pids() {
                    unsafe {
                        libc::kill(pid as i32, libc::SIGTERM);
                    }
                }
            }
        }
        Ok(())
    }
}

fn is_pid_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    let proc_path = format!("/proc/{}", pid);
    if !Path::new(&proc_path).exists() {
        return false;
    }
    // Check that it's not a zombie ('Z')
    if let Ok(stat) = fs::read_to_string(format!("/proc/{}/stat", pid)) {
        if let Some(rparen) = stat.rfind(')') {
            let after = stat[rparen + 1..].trim_start();
            if after.starts_with('Z') {
                return false;
            }
        }
    }
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

/// Scans `/proc/*/comm` on Linux for any running `retroarch` process.
pub fn find_retroarch_pids() -> Vec<u32> {
    let mut pids = Vec::new();
    let Ok(entries) = fs::read_dir("/proc") else {
        return pids;
    };
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let Some(name_str) = file_name.to_str() else {
            continue;
        };
        let Ok(pid) = name_str.parse::<u32>() else {
            continue;
        };
        let comm_path = entry.path().join("comm");
        if let Ok(comm) = fs::read_to_string(comm_path) {
            let trimmed = comm.trim();
            if trimmed == "retroarch" || trimmed == "retroarch32" {
                pids.push(pid);
            }
        }
    }
    pids
}

/// Mock RetroArch UDP server for local development and CI tests.
/// Listens on `bind_addr`, appends every received command to `log_file` (if provided) and stdout,
/// and exits cleanly with code 0 when `QUIT` is received.
pub fn run_mock_retroarch(bind_addr: SocketAddr, log_path: Option<&Path>) -> Result<()> {
    let sock = UdpSocket::bind(bind_addr)
        .with_context(|| format!("bind mock RetroArch UDP on {}", bind_addr))?;
    let mut buf = [0u8; 2048];
    loop {
        let (n, _peer) = sock.recv_from(&mut buf)?;
        let msg = String::from_utf8_lossy(&buf[..n]).trim().to_string();
        println!("[mock-retroarch] {}", msg);
        if let Some(path) = log_path {
            use std::io::Write;
            let mut file = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)?;
            writeln!(file, "{}", msg)?;
        }
        if msg == "QUIT" || msg.starts_with("QUIT ") {
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_mock_retroarch_udp_show_msg_and_quit() {
        let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        let addr = sock.local_addr().unwrap();
        drop(sock);

        let tmp = tempfile::tempdir().unwrap();
        let log_file = tmp.path().join("retroarch_udp.log");
        let log_clone = log_file.clone();

        let handle = thread::spawn(move || {
            run_mock_retroarch(addr, Some(&log_clone)).unwrap();
        });

        // Give listener thread a moment to bind
        thread::sleep(Duration::from_millis(30));

        let ctrl = RetroArchController::new(addr, false);
        ctrl.show_message("5 minutes left!").unwrap();
        ctrl.show_message("1 minute left!").unwrap();
        ctrl.quit_retroarch(false).unwrap();

        handle.join().unwrap();

        let contents = fs::read_to_string(&log_file).unwrap();
        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(
            lines,
            vec![
                "SHOW_MSG 5 minutes left!",
                "SHOW_MSG 1 minute left!",
                "QUIT"
            ]
        );
    }

    #[test]
    fn test_synthesize_chime_wav_headers_and_audio() {
        for kind in [ChimeKind::Warn5m, ChimeKind::Warn1m] {
            let wav = synthesize_chime_wav(kind);
            assert!(wav.len() > 44);
            assert_eq!(&wav[0..4], b"RIFF");
            assert_eq!(&wav[8..12], b"WAVE");
            assert_eq!(&wav[36..40], b"data");
            // Ensure non-silent audio samples exist
            let has_nonzero = wav[44..].iter().any(|&b| b != 0);
            assert!(has_nonzero);
        }
    }
}
