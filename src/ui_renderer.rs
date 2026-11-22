use crate::qrcode::QrCode;
use std::sync::OnceLock;
use tiny_skia::{
    Color, FillRule, FilterQuality, GradientStop, LineCap, Mask, Paint, Path, PathBuilder,
    Pattern, Pixmap, PixmapRef, Point, RadialGradient, Rect, SpreadMode, Stroke,
    Transform,
};
use ttf_parser::{Face, OutlineBuilder};

const FONT_SANS_EXTRABOLD: &[u8] = include_bytes!("../assets/fonts/Sans-ExtraBold.otf");
const FONT_SANS_REGULAR: &[u8] = include_bytes!("../assets/fonts/Sans-Regular.otf");
const FONT_MONO_BOLD: &[u8] = include_bytes!("../assets/fonts/Mono-Bold.otf");
const FONT_MONO_REGULAR: &[u8] = include_bytes!("../assets/fonts/Mono-Regular.otf");

/// Lazily loaded CJK fallback font bytes (Japanese + Simplified Chinese), loaded only when needed.
fn cjk_fallback_face() -> Option<Face<'static>> {
    static BYTES: OnceLock<Option<Vec<u8>>> = OnceLock::new();
    let data = BYTES.get_or_init(|| {
        let candidates = [
            "/opt/yasumu/cjk-fallback.ttf",
            "assets/fonts/CJK-Fallback.ttf",
            "/usr/share/fonts/google-droid-sans-fonts/DroidSansFallbackFull.ttf",
        ];
        for path in candidates {
            if let Ok(b) = std::fs::read(path) {
                return Some(b);
            }
        }
        None
    });
    data.as_deref().and_then(|b| Face::parse(b, 0).ok())
}

fn resolve_glyph(primary: &Face<'static>, ch: char) -> Option<(Face<'static>, ttf_parser::GlyphId)> {
    if let Some(g) = primary.glyph_index(ch) {
        return Some((primary.clone(), g));
    }
    if let Some(cjk) = cjk_fallback_face() {
        if let Some(g) = cjk.glyph_index(ch) {
            return Some((cjk, g));
        }
    }
    primary.glyph_index('?').map(|g| (primary.clone(), g))
}

// Obsidian Handheld Design System Tokens
pub const CANVAS_BASE: (u8, u8, u8) = (9, 11, 16);        // #090B10
pub const SURFACE_MATTE: (u8, u8, u8) = (20, 24, 36);     // #141824
pub const SURFACE_KEYCAP: (u8, u8, u8) = (30, 36, 51);    // #1E2433
pub const KEYCAP_STROKE: (u8, u8, u8) = (48, 56, 76);     // #30384C
pub const HAIRLINE: (u8, u8, u8) = (38, 45, 61);          // #262D3D
pub const TEXT_PRIMARY: (u8, u8, u8) = (243, 244, 246);   // #F3F4F6
pub const TEXT_SECONDARY: (u8, u8, u8) = (209, 213, 219); // #D1D5DB
pub const TEXT_MUTED: (u8, u8, u8) = (156, 163, 175);     // #9CA3AF
pub const TEXT_DIM: (u8, u8, u8) = (107, 114, 128);       // #6B7280

pub const SIGNAL_ORANGE: (u8, u8, u8) = (255, 102, 0);    // #FF6600
pub const NOCTURNE_INDIGO: (u8, u8, u8) = (129, 140, 248);// #818CF8
pub const AMBER_GOLD: (u8, u8, u8) = (251, 191, 36);      // #FBBF24
pub const EMERALD_GREEN: (u8, u8, u8) = (16, 185, 129);   // #10B981
pub const CRIMSON_ROSE: (u8, u8, u8) = (244, 63, 94);     // #F43F5E

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum FontKind {
    SansExtraBold,
    SansRegular,
    MonoBold,
    MonoRegular,
}

struct SkiaOutline {
    pb: PathBuilder,
    scale: f32,
    ox: f32,
    oy: f32,
}

impl OutlineBuilder for SkiaOutline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.pb.move_to(self.ox + x * self.scale, self.oy - y * self.scale);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.pb.line_to(self.ox + x * self.scale, self.oy - y * self.scale);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.pb.quad_to(
            self.ox + x1 * self.scale,
            self.oy - y1 * self.scale,
            self.ox + x * self.scale,
            self.oy - y * self.scale,
        );
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.pb.cubic_to(
            self.ox + x1 * self.scale,
            self.oy - y1 * self.scale,
            self.ox + x2 * self.scale,
            self.oy - y2 * self.scale,
            self.ox + x * self.scale,
            self.oy - y * self.scale,
        );
    }

    fn close(&mut self) {
        self.pb.close();
    }
}

pub struct UiCanvas {
    pub pixmap: Pixmap,
    pub width: u32,
    pub height: u32,
}

impl UiCanvas {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            pixmap: Pixmap::new(width, height).expect("create 640x480 Pixmap"),
            width,
            height,
        }
    }

    fn face(kind: FontKind) -> Face<'static> {
        let bytes = match kind {
            FontKind::SansExtraBold => FONT_SANS_EXTRABOLD,
            FontKind::SansRegular => FONT_SANS_REGULAR,
            FontKind::MonoBold => FONT_MONO_BOLD,
            FontKind::MonoRegular => FONT_MONO_REGULAR,
        };
        Face::parse(bytes, 0).expect("parse embedded OTF font")
    }

    /// Returns straight RGBA8888 pixels (all pixels opaque since base fill is alpha=255).
    pub fn rgba_bytes(&self) -> &[u8] {
        self.pixmap.data()
    }

    /// Fills deep matte obsidian `#090B10` with a smooth full-screen contextual radial ambient glow.
    pub fn draw_ambient_bg(&mut self, accent: (u8, u8, u8), alpha: f32) {
        self.pixmap.fill(Color::from_rgba8(
            CANVAS_BASE.0,
            CANVAS_BASE.1,
            CANVAS_BASE.2,
            255,
        ));

        let a = (alpha.clamp(0.0, 1.0) * 255.0) as u8;
        let mid_a = ((alpha * 0.42).clamp(0.0, 1.0) * 255.0) as u8;
        let grad = RadialGradient::new(
            Point::from_xy(self.width as f32 * 0.5, self.height as f32 * 0.22),
            Point::from_xy(self.width as f32 * 0.5, self.height as f32 * 0.22),
            self.width as f32 * 0.72,
            vec![
                GradientStop::new(0.0, Color::from_rgba8(accent.0, accent.1, accent.2, a)),
                GradientStop::new(0.48, Color::from_rgba8(accent.0, accent.1, accent.2, mid_a)),
                GradientStop::new(1.0, Color::from_rgba8(accent.0, accent.1, accent.2, 0)),
            ],
            SpreadMode::Pad,
            Transform::identity(),
        );

        if let Some(shader) = grad {
            let mut paint = Paint::default();
            paint.shader = shader;
            paint.anti_alias = true;
            if let Some(rect) = Rect::from_xywh(0.0, 0.0, self.width as f32, self.height as f32) {
                self.pixmap
                    .fill_rect(rect, &paint, Transform::identity(), None);
            }
        }
    }

    pub fn fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32, (r, g, b): (u8, u8, u8), a: u8) {
        let Some(rect) = Rect::from_xywh(x, y, w, h) else {
            return;
        };
        let mut paint = Paint::default();
        paint.set_color_rgba8(r, g, b, a);
        paint.anti_alias = true;
        self.pixmap
            .fill_rect(rect, &paint, Transform::identity(), None);
    }

    pub fn fill_rounded_rect(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        radius: f32,
        (r, g, b): (u8, u8, u8),
        a: u8,
    ) {
        let Some(path) = build_rounded_rect_path(x, y, w, h, radius) else {
            return;
        };
        let mut paint = Paint::default();
        paint.set_color_rgba8(r, g, b, a);
        paint.anti_alias = true;
        self.pixmap
            .fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
    }

    pub fn stroke_rounded_rect(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        radius: f32,
        stroke_width: f32,
        (r, g, b): (u8, u8, u8),
        a: u8,
    ) {
        let Some(path) = build_rounded_rect_path(x, y, w, h, radius) else {
            return;
        };
        let mut paint = Paint::default();
        paint.set_color_rgba8(r, g, b, a);
        paint.anti_alias = true;
        let stroke = Stroke {
            width: stroke_width,
            ..Stroke::default()
        };
        self.pixmap
            .stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    }

    pub fn draw_line(
        &mut self,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        width: f32,
        (r, g, b): (u8, u8, u8),
        a: u8,
    ) {
        let mut pb = PathBuilder::new();
        pb.move_to(x1, y1);
        pb.line_to(x2, y2);
        let Some(path) = pb.finish() else {
            return;
        };
        let mut paint = Paint::default();
        paint.set_color_rgba8(r, g, b, a);
        paint.anti_alias = true;
        let stroke = Stroke {
            width,
            line_cap: LineCap::Round,
            ..Stroke::default()
        };
        self.pixmap
            .stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    }

    pub fn fill_triangle(
        &mut self,
        p1: (f32, f32),
        p2: (f32, f32),
        p3: (f32, f32),
        (r, g, b): (u8, u8, u8),
        a: u8,
    ) {
        let mut pb = PathBuilder::new();
        pb.move_to(p1.0, p1.1);
        pb.line_to(p2.0, p2.1);
        pb.line_to(p3.0, p3.1);
        pb.close();
        let Some(path) = pb.finish() else {
            return;
        };
        let mut paint = Paint::default();
        paint.set_color_rgba8(r, g, b, a);
        paint.anti_alias = true;
        self.pixmap
            .fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
    }

    /// Measures the pixel width of `text` in the specified font, pixel size, and letter tracking (in em).
    pub fn measure_text(&self, text: &str, font: FontKind, size_px: f32, tracking_em: f32) -> f32 {
        let primary = Self::face(font);
        let tracking_px = size_px * tracking_em;
        let mut width = 0.0f32;
        let count = text.chars().count();
        for (idx, ch) in text.chars().enumerate() {
            if let Some((face, g)) = resolve_glyph(&primary, ch) {
                let scale = size_px / (face.units_per_em() as f32);
                let adv = face.glyph_hor_advance(g).unwrap_or(0) as f32 * scale;
                width += adv;
                if idx + 1 < count {
                    width += tracking_px;
                }
            }
        }
        width
    }

    /// Draws anti-aliased OpenType vector text with `baseline_y` (with automatic CJK glyph fallback).
    pub fn draw_text(
        &mut self,
        text: &str,
        x: f32,
        baseline_y: f32,
        font: FontKind,
        size_px: f32,
        tracking_em: f32,
        (r, g, b): (u8, u8, u8),
    ) -> f32 {
        let primary = Self::face(font);
        let tracking_px = size_px * tracking_em;
        let mut paint = Paint::default();
        paint.set_color_rgba8(r, g, b, 255);
        paint.anti_alias = true;

        let mut cursor_x = x;
        for ch in text.chars() {
            if let Some((face, gid)) = resolve_glyph(&primary, ch) {
                let scale = size_px / (face.units_per_em() as f32);
                let mut builder = SkiaOutline {
                    pb: PathBuilder::new(),
                    scale,
                    ox: cursor_x,
                    oy: baseline_y,
                };
                if face.outline_glyph(gid, &mut builder).is_some() {
                    if let Some(path) = builder.pb.finish() {
                        self.pixmap.fill_path(
                            &path,
                            &paint,
                            FillRule::Winding,
                            Transform::identity(),
                            None,
                        );
                    }
                }
                let adv = face.glyph_hor_advance(gid).unwrap_or(0) as f32 * scale;
                cursor_x += adv + tracking_px;
            }
        }
        cursor_x
    }

    /// Draws anti-aliased OpenType vector text clipped strictly inside `(clip_x, clip_y, clip_w, clip_h)`.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_text_clipped(
        &mut self,
        text: &str,
        x: f32,
        baseline_y: f32,
        font: FontKind,
        size_px: f32,
        tracking_em: f32,
        (r, g, b): (u8, u8, u8),
        clip_x: f32,
        clip_y: f32,
        clip_w: f32,
        clip_h: f32,
    ) -> f32 {
        let Some(mut mask) = Mask::new(self.width, self.height) else {
            return x;
        };
        let Some(rect) = Rect::from_xywh(clip_x, clip_y, clip_w, clip_h) else {
            return x;
        };
        mask.fill_path(
            &PathBuilder::from_rect(rect),
            FillRule::Winding,
            false,
            Transform::identity(),
        );

        let primary = Self::face(font);
        let tracking_px = size_px * tracking_em;
        let mut paint = Paint::default();
        paint.set_color_rgba8(r, g, b, 255);
        paint.anti_alias = true;

        let min_vis = clip_x - size_px * 1.5;
        let max_vis = clip_x + clip_w + size_px * 0.5;
        let mut cursor_x = x;
        for ch in text.chars() {
            if let Some((face, gid)) = resolve_glyph(&primary, ch) {
                let scale = size_px / (face.units_per_em() as f32);
                let adv = face.glyph_hor_advance(gid).unwrap_or(0) as f32 * scale;
                if cursor_x + adv >= min_vis && cursor_x <= max_vis {
                    let mut builder = SkiaOutline {
                        pb: PathBuilder::new(),
                        scale,
                        ox: cursor_x,
                        oy: baseline_y,
                    };
                    if face.outline_glyph(gid, &mut builder).is_some() {
                        if let Some(path) = builder.pb.finish() {
                            self.pixmap.fill_path(
                                &path,
                                &paint,
                                FillRule::Winding,
                                Transform::identity(),
                                Some(&mask),
                            );
                        }
                    }
                }
                cursor_x += adv + tracking_px;
            }
        }
        cursor_x
    }

    pub fn draw_text_centered(
        &mut self,
        text: &str,
        center_x: f32,
        baseline_y: f32,
        font: FontKind,
        size_px: f32,
        tracking_em: f32,
        color: (u8, u8, u8),
    ) {
        let w = self.measure_text(text, font, size_px, tracking_em);
        self.draw_text(text, center_x - w * 0.5, baseline_y, font, size_px, tracking_em, color);
    }

    /// Truncates `text` with `…` if it exceeds `max_width_px`.
    pub fn truncate_text(
        &self,
        text: &str,
        font: FontKind,
        size_px: f32,
        tracking_em: f32,
        max_width_px: f32,
    ) -> String {
        if self.measure_text(text, font, size_px, tracking_em) <= max_width_px {
            return text.to_string();
        }
        let mut out = String::new();
        for ch in text.chars() {
            let mut candidate = out.clone();
            candidate.push(ch);
            candidate.push('…');
            if self.measure_text(&candidate, font, size_px, tracking_em) > max_width_px {
                break;
            }
            out.push(ch);
        }
        out.push('…');
        out
    }

    /// Renders an RGBA image scaled cleanly inside a rounded rectangle (`radius` px) preserving aspect ratio (cover/contain).
    pub fn draw_rgba_image_rounded(
        &mut self,
        rgba: &[u8],
        img_w: u32,
        img_h: u32,
        dst_x: f32,
        dst_y: f32,
        dst_w: f32,
        dst_h: f32,
        radius: f32,
    ) {
        let Some(src_ref) = PixmapRef::from_bytes(rgba, img_w, img_h) else {
            return;
        };
        let Some(clip_path) = build_rounded_rect_path(dst_x, dst_y, dst_w, dst_h, radius) else {
            return;
        };
        let sx = dst_w / (img_w as f32);
        let sy = dst_h / (img_h as f32);
        let scale = sx.min(sy);
        let draw_w = (img_w as f32) * scale;
        let draw_h = (img_h as f32) * scale;
        let ox = dst_x + (dst_w - draw_w) * 0.5;
        let oy = dst_y + (dst_h - draw_h) * 0.5;

        let mut paint = Paint::default();
        paint.shader = Pattern::new(
            src_ref,
            SpreadMode::Pad,
            FilterQuality::Bilinear,
            1.0,
            Transform::from_scale(scale, scale).post_translate(ox, oy),
        );
        paint.anti_alias = true;

        self.pixmap.fill_path(
            &clip_path,
            &paint,
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }

    /// Draws a crisp QR code inside a rounded white quiet-zone tile.
    pub fn draw_qr_tile(
        &mut self,
        qr: &QrCode,
        center_x: f32,
        center_y: f32,
        module_px: f32,
        corner_radius: f32,
    ) {
        let quiet = 2.0;
        let total = (qr.size as f32) + quiet * 2.0;
        let tile_px = total * module_px;
        let left = (center_x - tile_px * 0.5).round();
        let top = (center_y - tile_px * 0.5).round();

        self.fill_rounded_rect(left, top, tile_px, tile_px, corner_radius, (255, 255, 255), 255);

        for y in 0..qr.size {
            for x in 0..qr.size {
                if qr.get(x, y) {
                    let px = left + (quiet + x as f32) * module_px;
                    let py = top + (quiet + y as f32) * module_px;
                    self.fill_rect(px, py, module_px, module_px, (9, 11, 16), 255);
                }
            }
        }
    }

    /// Measures the pixel width of a hardware control glyph alone (circle for A/B/X/Y,
    /// rectangle for L1/L2/R1/R2, small flat pill/disc for START/SELECT/MENU/VOL/POWER/RESET,
    /// vector triangles for directional arrows).
    fn measure_hardware_key(&self, key: &str) -> f32 {
        match key {
            "A" | "B" | "X" | "Y" => 28.0,
            "UP/DN" => 28.0,
            "L/R" => 34.0,
            "UP" | "DN" | "LEFT" | "RIGHT" => 26.0,
            "L1" | "L2" | "R1" | "R2" | "L" | "R" => {
                (self.measure_text(key, FontKind::MonoBold, 13.0, 0.02) + 16.0).max(36.0)
            }
            "START" | "SELECT" | "MENU" | "VOL" | "VOL+" | "VOL-" | "POWER" | "RESET" => {
                (self.measure_text(key, FontKind::MonoBold, 12.5, 0.04) + 22.0).max(44.0)
            }
            _ => (self.measure_text(key, FontKind::MonoBold, 13.0, 0.02) + 18.0).max(30.0),
        }
    }

    /// Measures a hardware button glyph + action verb pair.
    pub fn measure_keycap_pair(&self, key: &str, verb: &str) -> f32 {
        let kw = self.measure_hardware_key(key);
        let vw = self.measure_text(verb, FontKind::SansRegular, 16.5, 0.01);
        kw + 10.0 + vw
    }

    /// Draws a borderless hardware button glyph + action verb, returning the x position after it.
    pub fn draw_keycap_pair(&mut self, x: f32, center_y: f32, key: &str, verb: &str) -> f32 {
        let key_w = self.measure_hardware_key(key);
        let cx = x + key_w * 0.5;

        match key {
            // Face buttons: circle (28x28, r=14)
            "A" | "B" | "X" | "Y" => {
                let d = 28.0;
                self.fill_rounded_rect(x, center_y - d * 0.5, d, d, d * 0.5, SURFACE_KEYCAP, 255);
                let tw = self.measure_text(key, FontKind::MonoBold, 14.0, 0.0);
                self.draw_text(
                    key,
                    cx - tw * 0.5,
                    center_y + 4.8,
                    FontKind::MonoBold,
                    14.0,
                    0.0,
                    TEXT_PRIMARY,
                );
            }
            // Directional arrows: crisp vector triangles
            "UP/DN" => {
                let h = 28.0;
                self.fill_rounded_rect(x, center_y - h * 0.5, key_w, h, 7.0, SURFACE_KEYCAP, 255);
                // Up triangle
                self.fill_triangle(
                    (cx, center_y - 9.0),
                    (cx - 5.5, center_y - 2.2),
                    (cx + 5.5, center_y - 2.2),
                    TEXT_PRIMARY,
                    255,
                );
                // Down triangle
                self.fill_triangle(
                    (cx, center_y + 9.0),
                    (cx - 5.5, center_y + 2.2),
                    (cx + 5.5, center_y + 2.2),
                    TEXT_PRIMARY,
                    255,
                );
            }
            "L/R" => {
                let h = 26.0;
                self.fill_rounded_rect(x, center_y - h * 0.5, key_w, h, 7.0, SURFACE_KEYCAP, 255);
                // Left triangle
                self.fill_triangle(
                    (cx - 10.0, center_y),
                    (cx - 3.2, center_y - 5.5),
                    (cx - 3.2, center_y + 5.5),
                    TEXT_PRIMARY,
                    255,
                );
                // Right triangle
                self.fill_triangle(
                    (cx + 10.0, center_y),
                    (cx + 3.2, center_y - 5.5),
                    (cx + 3.2, center_y + 5.5),
                    TEXT_PRIMARY,
                    255,
                );
            }
            "UP" | "DN" | "LEFT" | "RIGHT" => {
                let h = 26.0;
                self.fill_rounded_rect(x, center_y - h * 0.5, key_w, h, 7.0, SURFACE_KEYCAP, 255);
                match key {
                    "UP" => self.fill_triangle(
                        (cx, center_y - 5.0),
                        (cx - 6.0, center_y + 3.5),
                        (cx + 6.0, center_y + 3.5),
                        TEXT_PRIMARY,
                        255,
                    ),
                    "DN" => self.fill_triangle(
                        (cx, center_y + 5.0),
                        (cx - 6.0, center_y - 3.5),
                        (cx + 6.0, center_y - 3.5),
                        TEXT_PRIMARY,
                        255,
                    ),
                    "LEFT" => self.fill_triangle(
                        (cx - 5.0, center_y),
                        (cx + 3.5, center_y - 6.0),
                        (cx + 3.5, center_y + 6.0),
                        TEXT_PRIMARY,
                        255,
                    ),
                    _ => self.fill_triangle(
                        (cx + 5.0, center_y),
                        (cx - 3.5, center_y - 6.0),
                        (cx - 3.5, center_y + 6.0),
                        TEXT_PRIMARY,
                        255,
                    ),
                }
            }
            // Shoulder buttons: rounded rectangle
            "L1" | "L2" | "R1" | "R2" | "L" | "R" => {
                let h = 26.0;
                self.fill_rounded_rect(x, center_y - h * 0.5, key_w, h, 5.0, SURFACE_KEYCAP, 255);
                let tw = self.measure_text(key, FontKind::MonoBold, 13.0, 0.02);
                self.draw_text(
                    key,
                    cx - tw * 0.5,
                    center_y + 4.4,
                    FontKind::MonoBold,
                    13.0,
                    0.02,
                    TEXT_PRIMARY,
                );
            }
            // Start / Select / Menu / Volume / Power / Reset: low-profile flat disc-pill
            "START" | "SELECT" | "MENU" | "VOL" | "VOL+" | "VOL-" | "POWER" | "RESET" => {
                let h = 24.0;
                self.fill_rounded_rect(
                    x,
                    center_y - h * 0.5,
                    key_w,
                    h,
                    h * 0.5,
                    SURFACE_KEYCAP,
                    255,
                );
                let tw = self.measure_text(key, FontKind::MonoBold, 12.5, 0.04);
                self.draw_text(
                    key,
                    cx - tw * 0.5,
                    center_y + 4.2,
                    FontKind::MonoBold,
                    12.5,
                    0.04,
                    TEXT_PRIMARY,
                );
            }
            _ => {
                let h = 26.0;
                self.fill_rounded_rect(x, center_y - h * 0.5, key_w, h, 6.0, SURFACE_KEYCAP, 255);
                let tw = self.measure_text(key, FontKind::MonoBold, 13.0, 0.02);
                self.draw_text(
                    key,
                    cx - tw * 0.5,
                    center_y + 4.4,
                    FontKind::MonoBold,
                    13.0,
                    0.02,
                    TEXT_PRIMARY,
                );
            }
        }

        let verb_x = x + key_w + 10.0;
        let end_x = self.draw_text(
            verb,
            verb_x,
            center_y + 5.5,
            FontKind::SansRegular,
            16.5,
            0.01,
            TEXT_SECONDARY,
        );
        end_x
    }

    /// Draws a centered row of hardware keycap + verb pairs along the bottom bar.
    pub fn draw_keycap_row_centered(&mut self, pairs: &[(&str, &str)], center_y: f32, gap: f32) {
        let total_w: f32 = pairs
            .iter()
            .map(|(k, v)| self.measure_keycap_pair(k, v))
            .sum::<f32>()
            + gap * (pairs.len().saturating_sub(1) as f32);
        let mut x = (self.width as f32 - total_w) * 0.5;
        for (k, v) in pairs {
            x = self.draw_keycap_pair(x, center_y, k, v) + gap;
        }
    }

    /// Draws the flush Obsidian top status bar:
    /// Left: `YASUMU` mark
    /// Right: `88%` + minimal rounded battery icon
    pub fn draw_top_bar(
        &mut self,
        remaining_label: Option<(&str, (u8, u8, u8))>,
        battery_percent: u8,
        battery_charging: bool,
    ) {
        let baseline_y = 42.0;
        // Left: flush tracked YASUMU mark
        self.draw_text("YASUMU", 28.0, baseline_y, FontKind::MonoBold, 19.5, 0.18, TEXT_PRIMARY);

        // Optional center label (kept for flexibility; main launcher places huge timer above cover)
        if let Some((label, color)) = remaining_label {
            let text_w = self.measure_text(label, FontKind::MonoBold, 20.0, 0.04);
            let pad_x = 18.0f32;
            let pill_w = pad_x * 2.0 + text_w;
            let pill_h = 34.0f32;
            let pill_x = 320.0 - pill_w * 0.5;
            let pill_y = baseline_y - 24.0;

            self.fill_rounded_rect(pill_x, pill_y, pill_w, pill_h, 17.0, SURFACE_MATTE, 255);
            self.draw_text(
                label,
                pill_x + pad_x,
                baseline_y - 0.5,
                FontKind::MonoBold,
                20.0,
                0.04,
                color,
            );
        }

        // Right: minimal rounded battery icon
        let bat_w = 28.0;
        let bat_h = 14.0;
        let bat_x = 640.0 - 28.0 - bat_w - 3.5;
        let bat_y = baseline_y - 13.5;

        self.stroke_rounded_rect(bat_x, bat_y, bat_w, bat_h, 3.0, 1.6, TEXT_MUTED, 255);
        self.fill_rounded_rect(bat_x + bat_w + 1.0, bat_y + 4.0, 3.0, 6.0, 1.0, TEXT_MUTED, 255);

        let fill_max = bat_w - 5.5;
        let fill_w = (fill_max * (battery_percent as f32 / 100.0)).max(2.5);
        let fill_color = if battery_percent <= 15 {
            CRIMSON_ROSE
        } else if battery_charging {
            EMERALD_GREEN
        } else {
            TEXT_PRIMARY
        };
        self.fill_rounded_rect(bat_x + 2.75, bat_y + 2.75, fill_w, bat_h - 5.5, 1.2, fill_color, 255);

        // Battery percentage text to the left of the battery icon
        let bat_str = if battery_charging {
            format!("{}%+", battery_percent)
        } else {
            format!("{}%", battery_percent)
        };
        let bat_tw = self.measure_text(&bat_str, FontKind::MonoRegular, 16.5, 0.02);
        let bat_tx = bat_x - 10.0 - bat_tw;
        self.draw_text(
            &bat_str,
            bat_tx,
            baseline_y - 0.5,
            FontKind::MonoRegular,
            16.5,
            0.02,
            TEXT_MUTED,
        );
    }

    pub fn save_png(&self, path: &std::path::Path) -> anyhow::Result<()> {
        let img = image::RgbaImage::from_raw(self.width, self.height, self.rgba_bytes().to_vec())
            .ok_or_else(|| anyhow::anyhow!("create RgbaImage from Pixmap"))?;
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        img.save(path)?;
        Ok(())
    }

    /// Saves the canvas as an uncompressed 24-bit RGB `.bmp` (required by U-Boot / muOS `bootlogo.bmp`).
    pub fn save_bmp_24bit(&self, path: &std::path::Path) -> anyhow::Result<()> {
        let rgba = self.rgba_bytes();
        let mut rgb = Vec::with_capacity((self.width * self.height * 3) as usize);
        for px in rgba.chunks_exact(4) {
            rgb.push(px[0]);
            rgb.push(px[1]);
            rgb.push(px[2]);
        }
        let img = image::RgbImage::from_raw(self.width, self.height, rgb)
            .ok_or_else(|| anyhow::anyhow!("create RgbImage from Pixmap"))?;
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        img.save_with_format(path, image::ImageFormat::Bmp)?;
        Ok(())
    }

    /// Renders the Obsidian Handheld boot splash screen (640×480).
    /// For `"yasumuOS"`, styles `yasu` in Signal Orange and `muOS` in Amber/Mustard Gold.
    pub fn render_splash_screen(&mut self, wordmark: &str) {
        self.draw_ambient_bg(SIGNAL_ORANGE, 0.20);
        let cx = (self.width as f32) * 0.5;
        let cy = (self.height as f32) * 0.5;
        let size = 60.0f32;
        let tracking = 0.04f32;

        if wordmark.eq_ignore_ascii_case("yasumuos") {
            let parts: [(&str, (u8, u8, u8)); 2] = [
                ("yasu", SIGNAL_ORANGE),
                ("muOS", AMBER_GOLD),
            ];
            let gap = size * tracking;
            let total_w: f32 = parts
                .iter()
                .map(|(s, _)| self.measure_text(s, FontKind::SansExtraBold, size, tracking))
                .sum::<f32>()
                + gap;
            let mut x = cx - total_w * 0.5;
            for (seg, color) in parts {
                let w = self.measure_text(seg, FontKind::SansExtraBold, size, tracking);
                self.draw_text(
                    seg,
                    x,
                    cy + 16.0,
                    FontKind::SansExtraBold,
                    size,
                    tracking,
                    color,
                );
                x += w + gap;
            }
        } else {
            self.draw_text_centered(
                wordmark,
                cx,
                cy + 16.0,
                FontKind::SansExtraBold,
                size,
                tracking,
                TEXT_PRIMARY,
            );
        }

        self.draw_text_centered(
            crate::APP_VERSION,
            cx,
            (self.height as f32) - 34.0,
            FontKind::MonoRegular,
            13.0,
            0.04,
            TEXT_MUTED,
        );
    }
}

fn build_rounded_rect_path(x: f32, y: f32, w: f32, h: f32, radius: f32) -> Option<Path> {
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let r = radius.min(w * 0.5).min(h * 0.5).max(0.0);
    let k = r * 0.5522848; // Bezier circle approximation
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();
    pb.finish()
}
