//! On-demand HTTP server lifecycle, captive-portal wildcard DNS, and Wi-Fi AP scripts.

use super::{build_portal_router, SharedCtx};
use anyhow::Result;
use serde::Deserialize;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use tokio::sync::oneshot;

/// Manages the lifecycle of the on-demand HTTP server, built-in wildcard DNS server (port 53),
/// and Wi-Fi AP (`wpa_supplicant` AP mode + `udhcpd`) on the RG35XXSP.
pub struct PortalHandle {
    pub shutdown_tx: Option<oneshot::Sender<()>>,
    pub dns_stop: Option<Arc<std::sync::atomic::AtomicBool>>,
    pub bind_addr: SocketAddr,
    pub dev_mode: bool,
}

#[derive(Deserialize)]
struct PortalNetStatus {
    mode: String,
    ip: String,
}

impl PortalHandle {
    pub fn start(
        ctx: SharedCtx,
        bind_addr: SocketAddr,
        dev_mode: bool,
        force_ap: bool,
    ) -> Result<Self> {
        let dns_stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        if !dev_mode {
            let (ap_ssid, ap_pass, home_ssid, home_pass) = {
                let mut guard = ctx.lock().unwrap();
                guard.portal_mode = "connecting".to_string();
                guard.portal_url = "http://192.168.4.1".to_string();
                (
                    guard.store.config.wifi_ssid.clone(),
                    guard.store.config.wifi_passphrase.clone(),
                    if force_ap {
                        None
                    } else {
                        guard.store.config.home_wifi_ssid.clone()
                    },
                    if force_ap {
                        None
                    } else {
                        guard.store.config.home_wifi_passphrase.clone()
                    },
                )
            };
            let ctx_bg = ctx.clone();
            let dns_stop_bg = dns_stop.clone();
            // Launch network setup in the background so the launcher responds immediately (<10ms)
            std::thread::spawn(move || {
                start_ap_services(
                    &ap_ssid,
                    &ap_pass,
                    home_ssid.as_deref(),
                    home_pass.as_deref(),
                );
                let parsed = std::fs::read_to_string("/run/yasumu-ap/portal-status.json")
                    .ok()
                    .and_then(|raw| serde_json::from_str::<PortalNetStatus>(&raw).ok());
                let is_ap = {
                    let mut guard = ctx_bg.lock().unwrap();
                    if let Some(st) = parsed {
                        let ap = st.mode == "ap";
                        guard.portal_mode = st.mode;
                        guard.portal_url = format!("http://{}", st.ip);
                        ap
                    } else {
                        guard.portal_mode = "ap".to_string();
                        guard.portal_url = "http://192.168.4.1".to_string();
                        true
                    }
                };
                if is_ap {
                    spawn_wildcard_dns_server([192, 168, 4, 1], dns_stop_bg);
                }
            });
        }

        let std_listener = std::net::TcpListener::bind(bind_addr)?;
        std_listener.set_nonblocking(true)?;
        let actual_addr = std_listener.local_addr()?;

        {
            let mut guard = ctx.lock().unwrap();
            guard.touch_activity();
            if dev_mode {
                guard.portal_mode = "ap".to_string();
                guard.portal_url = format!("http://{}", actual_addr);
            }
        }

        let (tx, rx) = oneshot::channel::<()>();
        let router = build_portal_router(ctx);

        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("build portal tokio runtime");
            rt.block_on(async move {
                let listener =
                    tokio::net::TcpListener::from_std(std_listener).expect("tokio listener");
                let _ = axum::serve(listener, router)
                    .with_graceful_shutdown(async {
                        let _ = rx.await;
                    })
                    .await;
            });
        });

        Ok(Self {
            shutdown_tx: Some(tx),
            dns_stop: Some(dns_stop),
            bind_addr: actual_addr,
            dev_mode,
        })
    }

    pub fn stop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        if let Some(flag) = self.dns_stop.take() {
            flag.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        if !self.dev_mode {
            stop_ap_services();
        }
    }
}

/// Pure-Rust wildcard DNS responder (UDP port 53) returning `target_ip` (192.168.4.1) for all A queries.
/// Only started in Offline AP mode.
fn spawn_wildcard_dns_server(
    target_ip: [u8; 4],
    stop: Arc<std::sync::atomic::AtomicBool>,
) {
    std::thread::spawn(move || {
        let Ok(sock) = std::net::UdpSocket::bind("0.0.0.0:53") else {
            return;
        };
        let _ = sock.set_read_timeout(Some(std::time::Duration::from_millis(250)));
        let mut buf = [0u8; 512];
        while !stop.load(std::sync::atomic::Ordering::Relaxed) {
            let Ok((len, peer)) = sock.recv_from(&mut buf) else {
                continue;
            };
            if let Some(resp) = build_wildcard_dns_response(&buf[..len], target_ip) {
                let _ = sock.send_to(&resp, peer);
            }
        }
    });
}

fn build_wildcard_dns_response(query: &[u8], ip: [u8; 4]) -> Option<Vec<u8>> {
    if query.len() < 12 {
        return None;
    }
    // Parse end of first question (QNAME terminated by 0x00 + 2-byte QTYPE + 2-byte QCLASS)
    let mut pos = 12;
    while pos < query.len() {
        let label_len = query[pos] as usize;
        pos += 1;
        if label_len == 0 {
            break;
        }
        pos += label_len;
    }
    if pos + 4 > query.len() {
        return None;
    }
    let question_end = pos + 4;

    let mut resp = Vec::with_capacity(question_end + 16);
    // Transaction ID
    resp.extend_from_slice(&query[0..2]);
    // Flags: standard response, authoritative, no error (0x8180)
    resp.extend_from_slice(&[0x81, 0x80]);
    // QDCOUNT = 1, ANCOUNT = 1, NSCOUNT = 0, ARCOUNT = 0
    resp.extend_from_slice(&[0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00]);
    // Original Question section
    resp.extend_from_slice(&query[12..question_end]);
    // Answer section: pointer to QNAME at offset 12 (0xC00C), TYPE=A (1), CLASS=IN (1), TTL=60s, RDLENGTH=4, IP
    resp.extend_from_slice(&[
        0xC0, 0x0C, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x3C, 0x00, 0x04, ip[0], ip[1],
        ip[2], ip[3],
    ]);
    Some(resp)
}

impl Drop for PortalHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

fn start_ap_services(
    ssid: &str,
    passphrase: &str,
    home_ssid: Option<&str>,
    home_passphrase: Option<&str>,
) {
    let script = PathBuf::from("/opt/yasumu/ap-up.sh");
    if script.is_file() {
        let mut cmd = Command::new(&script);
        cmd.args([ssid, passphrase]);
        if let Some(hs) = home_ssid {
            if !hs.is_empty() {
                cmd.arg(hs);
                cmd.arg(home_passphrase.unwrap_or(""));
            }
        }
        let _ = cmd.status();
    }
}

fn stop_ap_services() {
    let script = PathBuf::from("/opt/yasumu/ap-down.sh");
    if script.is_file() {
        let _ = Command::new(&script).status();
    }
}
