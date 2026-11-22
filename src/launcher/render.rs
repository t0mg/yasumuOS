use super::LauncherApp;
use crate::qrcode::QrCode;
use crate::state::LockStatus;
use crate::ui_renderer::{
    FontKind, AMBER_GOLD, CANVAS_BASE, CRIMSON_ROSE, EMERALD_GREEN, HAIRLINE, NOCTURNE_INDIGO,
    SIGNAL_ORANGE, SURFACE_KEYCAP, SURFACE_MATTE, TEXT_MUTED, TEXT_PRIMARY, TEXT_SECONDARY,
};

impl LauncherApp {
    pub(super) fn render_game_list(&mut self) {
        let tr = self.tr();
        // When the quota system is turned off, display nothing so we don't draw attention to it.
        let has_countdown =
            self.status.quota_enabled && self.status.effective_remaining_seconds < u64::MAX;

        self.ui.draw_ambient_bg(SIGNAL_ORANGE, 0.20);
        self.ui
            .draw_top_bar(None, self.status.battery_percent, self.status.battery_charging);

        // Left column: flush vertical game list — larger handheld-friendly typography, no platform clutter
        let total_items = self.total_list_items();
        let visible_rows = 6usize;
        let start_idx = if self.selected_index >= visible_rows {
            self.selected_index + 1 - visible_rows
        } else {
            0
        };

        let list_x = 24.0f32;
        let list_w = 348.0f32;
        let row_h = 48.0f32;
        let row_step = 54.0f32;
        let list_top = 78.0f32;
        let max_title_w = list_w - 28.0;

        for row in 0..visible_rows {
            let item_idx = start_idx + row;
            if item_idx >= total_items {
                break;
            }
            let y = list_top + (row as f32) * row_step;
            let is_selected = item_idx == self.selected_index;
            let baseline = y + 31.0;

            if item_idx < self.status.allowed_games.len() {
                let g = &self.status.allowed_games[item_idx];
                if is_selected {
                    self.ui
                        .fill_rounded_rect(list_x, y, list_w, row_h, 10.0, SIGNAL_ORANGE, 255);
                    let font = FontKind::SansExtraBold;
                    let size = 21.0;
                    let full_w = self.ui.measure_text(&g.display_title, font, size, 0.01);
                    let clip_x = list_x + 14.0;
                    if full_w > max_title_w {
                        // Smooth continuous horizontal marquee with initial 1.2s pause
                        let gap = 56.0f32;
                        let cycle_w = full_w + gap;
                        let speed_px_s = 48.0f32;
                        let hold_s = 1.2f32;
                        let scroll_s = cycle_w / speed_px_s;
                        let period_s = hold_s + scroll_s;
                        let t = self.marquee_started_at.elapsed().as_secs_f32() % period_s;
                        let offset = if t < hold_s {
                            0.0
                        } else {
                            (t - hold_s) * speed_px_s
                        };
                        self.ui.draw_text_clipped(
                            &g.display_title,
                            clip_x - offset,
                            baseline,
                            font,
                            size,
                            0.01,
                            CANVAS_BASE,
                            clip_x,
                            y,
                            max_title_w,
                            row_h,
                        );
                        if offset > 0.0 {
                            self.ui.draw_text_clipped(
                                &g.display_title,
                                clip_x - offset + cycle_w,
                                baseline,
                                font,
                                size,
                                0.01,
                                CANVAS_BASE,
                                clip_x,
                                y,
                                max_title_w,
                                row_h,
                            );
                        }
                    } else {
                        self.ui.draw_text(
                            &g.display_title,
                            clip_x,
                            baseline,
                            font,
                            size,
                            0.01,
                            CANVAS_BASE,
                        );
                    }
                } else {
                    let title = self.ui.truncate_text(
                        &g.display_title,
                        FontKind::SansRegular,
                        20.5,
                        0.01,
                        max_title_w,
                    );
                    self.ui.draw_text(
                        &title,
                        list_x + 14.0,
                        baseline,
                        FontKind::SansRegular,
                        20.5,
                        0.01,
                        TEXT_PRIMARY,
                    );
                }
            } else {
                // Bottom system entry: Power Off
                if is_selected {
                    self.ui
                        .fill_rounded_rect(list_x, y, list_w, row_h, 10.0, SURFACE_KEYCAP, 255);
                    self.ui
                        .fill_rounded_rect(list_x, y + 9.0, 4.0, row_h - 18.0, 2.0, SIGNAL_ORANGE, 255);
                    self.ui.draw_text(
                        tr.power_off_row,
                        list_x + 16.0,
                        baseline,
                        FontKind::SansExtraBold,
                        19.5,
                        0.01,
                        TEXT_PRIMARY,
                    );
                } else {
                    self.ui.draw_text(
                        tr.power_off_row,
                        list_x + 14.0,
                        baseline,
                        FontKind::SansRegular,
                        19.0,
                        0.01,
                        TEXT_MUTED,
                    );
                }
            }
        }

        // Right column: prominent top-right time counter (when active) + clean cover/cartridge below (zero redundant text)
        let right_cx = 506.0f32;
        if has_countdown {
            let rem_mins = (self.status.effective_remaining_seconds + 59) / 60;
            let timer_str = format!("{} min", rem_mins);
            let box_w = 204.0f32;
            let box_h = 72.0f32;
            let box_x = right_cx - box_w * 0.5;
            let box_y = list_top;
            self.ui
                .fill_rounded_rect(box_x, box_y, box_w, box_h, 14.0, SURFACE_MATTE, 255);
            self.ui.draw_text_centered(
                &timer_str,
                right_cx,
                box_y + 49.0,
                FontKind::MonoBold,
                36.0,
                0.03,
                SIGNAL_ORANGE,
            );
        }

        if let Some(selected_game) = self.status.allowed_games.get(self.selected_index).cloned() {
            let (art_size, art_y) = if has_countdown {
                (204.0f32, 170.0f32)
            } else {
                (216.0f32, 126.0f32)
            };
            let art_x = right_cx - art_size * 0.5;

            let mut drew_art = false;
            if let Some(art_path) = &selected_game.box_art_path {
                if let Some(tex) = self.load_box_art(art_path) {
                    let tex_w = tex.width;
                    let tex_h = tex.height;
                    let tex_rgba = tex.rgba.clone();
                    self.ui.draw_rgba_image_rounded(
                        &tex_rgba, tex_w, tex_h, art_x, art_y, art_size, art_size, 14.0,
                    );
                    drew_art = true;
                }
            }

            if !drew_art {
                // Minimal matte cartridge silhouette (shows platform badge once on the cartridge, zero text below)
                let cw = art_size - 12.0;
                let ch = art_size;
                let cx = right_cx - cw * 0.5;
                let cy = art_y;
                self.ui.fill_rounded_rect(cx, cy, cw, ch, 14.0, SURFACE_MATTE, 255);
                self.ui
                    .fill_rounded_rect(cx + 18.0, cy + 24.0, cw - 36.0, ch * 0.52, 8.0, CANVAS_BASE, 255);
                self.ui.draw_text_centered(
                    &selected_game.system_badge,
                    right_cx,
                    cy + 24.0 + ch * 0.31,
                    FontKind::MonoBold,
                    28.0,
                    0.10,
                    TEXT_MUTED,
                );
                for i in 0..3 {
                    let gy = cy + ch - 46.0 + (i as f32) * 11.0;
                    self.ui
                        .draw_line(cx + 46.0, gy, cx + cw - 46.0, gy, 2.2, HAIRLINE, 255);
                }
            }
        } else if self.status.allowed_games.is_empty() {
            let cy = if has_countdown { 250.0 } else { 224.0 };
            self.ui.draw_text_centered(
                tr.no_games_title,
                right_cx,
                cy,
                FontKind::SansExtraBold,
                19.0,
                0.05,
                SIGNAL_ORANGE,
            );
            let (l1, l2) = wrap_title_two_lines(tr.no_games_sub, 20);
            self.ui.draw_text_centered(&l1, right_cx, cy + 34.0, FontKind::SansRegular, 16.5, 0.01, TEXT_MUTED);
            if !l2.is_empty() {
                self.ui.draw_text_centered(&l2, right_cx, cy + 58.0, FontKind::SansRegular, 16.5, 0.01, TEXT_MUTED);
            }
        } else {
            let cy = if has_countdown { 250.0 } else { 224.0 };
            self.ui.draw_text_centered(
                tr.power_off_row,
                right_cx,
                cy,
                FontKind::SansExtraBold,
                22.0,
                0.02,
                TEXT_PRIMARY,
            );
            let (l1, l2) = wrap_title_two_lines(tr.power_off_desc, 22);
            self.ui.draw_text_centered(&l1, right_cx, cy + 34.0, FontKind::SansRegular, 16.5, 0.01, TEXT_MUTED);
            if !l2.is_empty() {
                self.ui.draw_text_centered(&l2, right_cx, cy + 58.0, FontKind::SansRegular, 16.5, 0.01, TEXT_MUTED);
            }
        }

        // Flush bottom hardware button bar
        let action_verb = if self.selected_index < self.status.allowed_games.len() {
            tr.key_play
        } else {
            tr.key_confirm
        };
        let y_bar = 442.0f32;
        self.ui.draw_keycap_pair(28.0, y_bar, "A", action_verb);
        let right_w = self.ui.measure_keycap_pair("SELECT", tr.key_parent);
        self.ui
            .draw_keycap_pair(640.0 - 28.0 - right_w, y_bar, "SELECT", tr.key_parent);
    }

    pub(super) fn render_power_off_confirm(&mut self) {
        let tr = self.tr();
        self.ui.draw_ambient_bg(SIGNAL_ORANGE, 0.18);
        self.ui
            .draw_top_bar(None, self.status.battery_percent, self.status.battery_charging);

        self.ui.draw_text_centered(
            tr.power_off_title,
            320.0,
            210.0,
            FontKind::SansExtraBold,
            42.0,
            0.05,
            SIGNAL_ORANGE,
        );
        self.ui.draw_text_centered(
            tr.power_off_confirm_q,
            320.0,
            258.0,
            FontKind::SansRegular,
            21.0,
            0.01,
            TEXT_PRIMARY,
        );
        let (tip1, tip2) = wrap_title_two_lines(tr.power_off_sleep_tip, 40);
        self.ui.draw_text_centered(
            &tip1,
            320.0,
            296.0,
            FontKind::SansRegular,
            17.0,
            0.01,
            TEXT_MUTED,
        );
        if !tip2.is_empty() {
            self.ui.draw_text_centered(
                &tip2,
                320.0,
                322.0,
                FontKind::SansRegular,
                17.0,
                0.01,
                TEXT_MUTED,
            );
        }

        self.ui.draw_keycap_row_centered(
            &[("A", tr.power_off_row), ("B", tr.key_cancel)],
            440.0,
            44.0,
        );
    }

    pub(super) fn render_shutdown_screen(&mut self) {
        let tr = self.tr();
        // Matches bootlogo.bmp (yasu in Signal Orange + muOS in Amber Gold + 0.20 orange glow)
        self.ui.render_splash_screen("yasumuOS");
        self.ui.draw_text_centered(
            tr.shutting_down_sub,
            320.0,
            296.0,
            FontKind::SansRegular,
            19.0,
            0.02,
            TEXT_MUTED,
        );
    }

    pub(super) fn render_lock_screen(&mut self) {
        let tr = self.tr();
        let (accent, title, subtitle, left_verb) = match &self.status.lock_status {
            LockStatus::QuotaReached => (
                SIGNAL_ORANGE,
                tr.time_up_title.to_string(),
                tr.time_up_sub.to_string(),
                tr.key_parent_access,
            ),
            LockStatus::BedtimeActive { wake_time } => (
                NOCTURNE_INDIGO,
                tr.bedtime_title.to_string(),
                format!("{} {}.", tr.bedtime_sub_prefix, wake_time),
                tr.key_parent_access,
            ),
            LockStatus::ClockSyncRequired | LockStatus::TamperLocked { .. } => (
                AMBER_GOLD,
                tr.clock_sync_title.to_string(),
                tr.clock_sync_sub.to_string(),
                tr.key_set_clock,
            ),
            LockStatus::Unlocked => return,
        };

        // Full-screen obsidian radial ambient glow matching contextual state accent
        self.ui.draw_ambient_bg(accent, 0.22);
        self.ui
            .draw_top_bar(None, self.status.battery_percent, self.status.battery_charging);

        // Centered bold uppercase headline + calm subtitle (zero container boxes or badges)
        let title_size = if title.chars().count() > 14 { 40.0 } else { 46.0 };
        self.ui.draw_text_centered(
            &title,
            320.0,
            230.0,
            FontKind::SansExtraBold,
            title_size,
            0.05,
            accent,
        );
        self.ui.draw_text_centered(
            &subtitle,
            320.0,
            278.0,
            FontKind::SansRegular,
            21.5,
            0.01,
            TEXT_SECONDARY,
        );

        // Flush bottom hardware button prompts: Unlock (A) and Power Off (X)
        self.ui.draw_keycap_row_centered(
            &[("A", left_verb), ("X", tr.power_off_row)],
            440.0,
            40.0,
        );
    }

    pub(super) fn render_pin_tumbler(&mut self, digits: [u8; 4], cursor: usize, error_msg: Option<&str>) {
        let tr = self.tr();
        let is_cooldown = self.status.pin_lockout_remaining_seconds > 0;
        let is_error = is_cooldown || error_msg.is_some();

        let (accent, title) = if is_error {
            (CRIMSON_ROSE, tr.pin_error_title)
        } else {
            (AMBER_GOLD, tr.pin_title)
        };

        self.ui.draw_ambient_bg(accent, 0.18);
        self.ui
            .draw_top_bar(None, self.status.battery_percent, self.status.battery_charging);

        self.ui.draw_text_centered(
            title,
            320.0,
            138.0,
            FontKind::SansExtraBold,
            34.0,
            0.05,
            accent,
        );

        // 4 clean matte rounded tumblers centered horizontally
        let box_w = 78.0f32;
        let box_h = 92.0f32;
        let gap = 20.0f32;
        let total_w = box_w * 4.0 + gap * 3.0;
        let start_x = (640.0 - total_w) * 0.5;
        let box_y = 188.0f32;

        for (i, &digit) in digits.iter().enumerate() {
            let bx = start_x + (i as f32) * (box_w + gap);
            let is_active = i == cursor && !is_error;

            self.ui
                .fill_rounded_rect(bx, box_y, box_w, box_h, 14.0, SURFACE_MATTE, 255);

            if is_error {
                self.ui
                    .stroke_rounded_rect(bx, box_y, box_w, box_h, 14.0, 1.6, CRIMSON_ROSE, 210);
            } else if is_active {
                self.ui
                    .stroke_rounded_rect(bx, box_y, box_w, box_h, 14.0, 2.4, AMBER_GOLD, 255);
                // Crisp vector ▲ and ▼ carets above & below active tumbler
                let cx = bx + box_w * 0.5;
                self.ui.fill_triangle(
                    (cx, box_y - 11.0),
                    (cx - 6.5, box_y - 3.0),
                    (cx + 6.5, box_y - 3.0),
                    AMBER_GOLD,
                    255,
                );
                self.ui.fill_triangle(
                    (cx, box_y + box_h + 11.0),
                    (cx - 6.5, box_y + box_h + 3.0),
                    (cx + 6.5, box_y + box_h + 3.0),
                    AMBER_GOLD,
                    255,
                );
            } else {
                self.ui
                    .stroke_rounded_rect(bx, box_y, box_w, box_h, 14.0, 1.2, HAIRLINE, 255);
            }

            let d_str = digit.to_string();
            let digit_color = if is_error {
                (252, 165, 165)
            } else {
                TEXT_PRIMARY
            };
            self.ui.draw_text_centered(
                &d_str,
                bx + box_w * 0.5,
                box_y + 60.0,
                FontKind::MonoBold,
                42.0,
                0.0,
                digit_color,
            );
        }

        if is_cooldown {
            let msg = format!(
                "{} {}s.",
                tr.pin_cooldown_prefix, self.status.pin_lockout_remaining_seconds
            );
            self.ui.draw_text_centered(
                &msg,
                320.0,
                336.0,
                FontKind::SansRegular,
                18.5,
                0.01,
                CRIMSON_ROSE,
            );
        } else if let Some(err) = error_msg {
            self.ui.draw_text_centered(
                err,
                320.0,
                336.0,
                FontKind::SansRegular,
                18.5,
                0.01,
                CRIMSON_ROSE,
            );
        }

        self.ui.draw_keycap_row_centered(
            &[
                ("UP/DN", tr.key_digit),
                ("L/R", tr.key_move),
                ("A", tr.key_confirm),
                ("B", tr.key_back),
            ],
            440.0,
            16.0,
        );
    }

    pub(super) fn render_parent_portal(&mut self) {
        let tr = self.tr();
        self.ui.draw_ambient_bg(EMERALD_GREEN, 0.16);
        self.ui
            .draw_top_bar(None, self.status.battery_percent, self.status.battery_charging);

        if self.status.portal_mode == "connecting" {
            self.ui.draw_text_centered(
                tr.portal_connecting_title,
                320.0,
                220.0,
                FontKind::SansExtraBold,
                34.0,
                0.05,
                EMERALD_GREEN,
            );
            let sub = if let Some(hs) = &self.status.home_wifi_ssid {
                format!("{} {}", tr.portal_trying_home, hs)
            } else {
                format!("{} {}", tr.portal_starting_ap, self.status.wifi_ssid)
            };
            self.ui.draw_text_centered(
                &sub,
                320.0,
                268.0,
                FontKind::SansRegular,
                20.0,
                0.01,
                TEXT_SECONDARY,
            );
            self.ui.draw_keycap_row_centered(
                &[("B", tr.key_close_portal), ("X", tr.key_exit_muos)],
                440.0,
                28.0,
            );
            return;
        }

        // Bold emerald headline
        self.ui.draw_text_centered(
            tr.portal_title,
            320.0,
            106.0,
            FontKind::SansExtraBold,
            36.0,
            0.05,
            EMERALD_GREEN,
        );

        if self.status.portal_mode == "home_wifi" {
            // Centered Home Wi-Fi LAN IP QR screen (zero redundant frames)
            let heading = self
                .status
                .home_wifi_ssid
                .as_deref()
                .map(|s| format!("{} : {}", tr.portal_home_heading, s))
                .unwrap_or_else(|| tr.portal_home_heading.to_string());
            self.ui.draw_text_centered(
                &heading,
                320.0,
                142.0,
                FontKind::MonoBold,
                16.5,
                0.08,
                EMERALD_GREEN,
            );

            let url_qr = QrCode::encode_text(&self.status.portal_url);
            self.ui.draw_qr_tile(&url_qr, 320.0, 246.0, 5.5, 12.0);

            self.ui.draw_text_centered(
                &self.status.portal_url,
                320.0,
                362.0,
                FontKind::MonoRegular,
                17.5,
                0.02,
                TEXT_PRIMARY,
            );
            self.ui.draw_text_centered(
                tr.portal_clock_sync_note,
                320.0,
                394.0,
                FontKind::SansRegular,
                15.0,
                0.01,
                TEXT_MUTED,
            );

            self.ui.draw_keycap_row_centered(
                &[
                    ("B", tr.key_close_portal),
                    ("Y", tr.key_offline_ap),
                    ("X", tr.key_exit_muos),
                ],
                440.0,
                22.0,
            );
            return;
        }

        // Local Wi-Fi Mode — clean minimal two-column layout with subtle center hairline
        self.ui
            .draw_line(320.0, 146.0, 320.0, 348.0, 1.0, HAIRLINE, 255);

        // Left column: Local Wi-Fi credentials (flush, zero nested box)
        let left_x = 40.0f32;
        self.ui.draw_text(
            tr.portal_ap_heading,
            left_x,
            174.0,
            FontKind::MonoBold,
            16.0,
            0.10,
            EMERALD_GREEN,
        );
        self.ui.draw_text(
            tr.portal_network_label,
            left_x,
            212.0,
            FontKind::SansRegular,
            17.5,
            0.01,
            TEXT_MUTED,
        );
        self.ui.draw_text(
            &self.status.wifi_ssid,
            left_x,
            246.0,
            FontKind::MonoBold,
            24.5,
            0.02,
            TEXT_PRIMARY,
        );
        self.ui.draw_text(
            tr.portal_password_label,
            left_x,
            290.0,
            FontKind::SansRegular,
            17.5,
            0.01,
            TEXT_MUTED,
        );
        self.ui.draw_text(
            &self.status.wifi_passphrase,
            left_x,
            324.0,
            FontKind::MonoBold,
            24.5,
            0.02,
            EMERALD_GREEN,
        );

        // Right column: Crisp scannable Wi-Fi QR tile + fallback URL below
        let right_cx = 474.0f32;
        let wifi_payload = format!(
            "WIFI:T:WPA;S:{};P:{};;",
            self.status.wifi_ssid, self.status.wifi_passphrase
        );
        let wifi_qr = QrCode::encode_text(&wifi_payload);
        let module_px = if wifi_qr.size >= 33 { 4.2 } else { 4.8 };
        self.ui.draw_qr_tile(&wifi_qr, right_cx, 232.0, module_px, 12.0);

        self.ui.draw_text_centered(
            &self.status.portal_url,
            right_cx,
            348.0,
            FontKind::MonoRegular,
            16.5,
            0.01,
            TEXT_MUTED,
        );

        // Calm centered sync note + bottom hardware buttons
        self.ui.draw_text_centered(
            tr.portal_clock_sync_note,
            320.0,
            394.0,
            FontKind::SansRegular,
            15.0,
            0.01,
            TEXT_MUTED,
        );

        self.ui.draw_keycap_row_centered(
            &[("B", tr.key_close_portal), ("X", tr.key_exit_muos)],
            440.0,
            32.0,
        );
    }
}

fn wrap_title_two_lines(title: &str, max_chars: usize) -> (String, String) {
    if title.chars().count() <= max_chars {
        return (title.to_string(), String::new());
    }
    let mut line1 = String::new();
    let mut line2 = String::new();
    for word in title.split_whitespace() {
        let candidate = if line1.is_empty() {
            word.to_string()
        } else {
            format!("{} {}", line1, word)
        };
        if line2.is_empty() && candidate.chars().count() <= max_chars {
            line1 = candidate;
        } else {
            let candidate2 = if line2.is_empty() {
                word.to_string()
            } else {
                format!("{} {}", line2, word)
            };
            line2 = candidate2.chars().take(max_chars).collect();
        }
    }
    if line1.is_empty() {
        let chars: Vec<char> = title.chars().collect();
        line1 = chars.iter().take(max_chars).collect();
        line2 = chars.iter().skip(max_chars).take(max_chars).collect();
    }
    (line1, line2)
}

