/// Pure-Rust ISO/IEC 18004 QR Code Generator (Byte Mode, ECC Level L, Versions 1–4).
/// Capable of encoding strings up to 78 bytes (easily covers `WIFI:T:WPA;S:RetroParent;P:...;;` and `http://192.168.4.1`).

#[derive(Debug, Clone)]
pub struct QrCode {
    pub size: usize,
    pub modules: Vec<bool>,
}

struct VersionSpec {
    version: usize,
    size: usize,
    data_codewords: usize,
    ecc_codewords: usize,
    alignment_coord: Option<usize>,
}

const VERSIONS_L: [VersionSpec; 4] = [
    VersionSpec {
        version: 1,
        size: 21,
        data_codewords: 19,
        ecc_codewords: 7,
        alignment_coord: None,
    },
    VersionSpec {
        version: 2,
        size: 25,
        data_codewords: 34,
        ecc_codewords: 10,
        alignment_coord: Some(18),
    },
    VersionSpec {
        version: 3,
        size: 29,
        data_codewords: 55,
        ecc_codewords: 15,
        alignment_coord: Some(22),
    },
    VersionSpec {
        version: 4,
        size: 33,
        data_codewords: 80,
        ecc_codewords: 20,
        alignment_coord: Some(26),
    },
];

impl QrCode {
    pub fn encode_text(text: &str) -> Self {
        Self::encode_bytes(text.as_bytes())
    }

    pub fn encode_bytes(data: &[u8]) -> Self {
        // Choose smallest version (1..=4) that fits data in Byte mode (4-bit mode + 8-bit len + 8*N bits <= data_codewords*8)
        let payload = if data.len() > 78 { &data[..78] } else { data };
        let spec = VERSIONS_L
            .iter()
            .find(|v| payload.len() + 2 <= v.data_codewords)
            .unwrap_or(&VERSIONS_L[3]);

        let data_cw = build_data_codewords(payload, spec.data_codewords);
        let ecc_cw = reed_solomon_compute_remainder(&data_cw, spec.ecc_codewords);

        let mut all_codewords = Vec::with_capacity(spec.data_codewords + spec.ecc_codewords);
        all_codewords.extend_from_slice(&data_cw);
        all_codewords.extend_from_slice(&ecc_cw);

        let size = spec.size;
        let mut modules = vec![false; size * size];
        let mut is_function = vec![false; size * size];

        draw_function_patterns(spec, &mut modules, &mut is_function);
        draw_codewords(size, &all_codewords, &mut modules, &is_function);

        // Apply Mask Pattern 0 ((row + col) % 2 == 0) and draw Format Info for ECC Level L (01), Mask 0 (000)
        apply_mask_0(size, &mut modules, &is_function);
        draw_format_bits(size, 0, &mut modules, &mut is_function);

        let _ = spec.version;
        QrCode { size, modules }
    }

    pub fn get(&self, x: usize, y: usize) -> bool {
        if x < self.size && y < self.size {
            self.modules[y * self.size + x]
        } else {
            false
        }
    }
}

fn build_data_codewords(data: &[u8], capacity: usize) -> Vec<u8> {
    let mut bits: Vec<bool> = Vec::with_capacity(capacity * 8);
    // Mode indicator: Byte mode = 0100
    push_bits(&mut bits, 0b0100, 4);
    // Character count indicator (8 bits for Version 1-9 in Byte mode)
    push_bits(&mut bits, data.len() as u32, 8);
    for &b in data {
        push_bits(&mut bits, b as u32, 8);
    }

    // Terminator: up to 4 zero bits
    let max_bits = capacity * 8;
    let term = (max_bits - bits.len()).min(4);
    push_bits(&mut bits, 0, term);

    // Pad to byte boundary
    while bits.len() % 8 != 0 {
        bits.push(false);
    }

    let mut codewords = Vec::with_capacity(capacity);
    for chunk in bits.chunks_exact(8) {
        let mut byte = 0u8;
        for &bit in chunk {
            byte = (byte << 1) | (bit as u8);
        }
        codewords.push(byte);
    }

    let pad_bytes = [0xECu8, 0x11u8];
    let mut pad_idx = 0;
    while codewords.len() < capacity {
        codewords.push(pad_bytes[pad_idx % 2]);
        pad_idx += 1;
    }
    codewords
}

fn push_bits(bits: &mut Vec<bool>, val: u32, count: usize) {
    for i in (0..count).rev() {
        bits.push(((val >> i) & 1) != 0);
    }
}

fn gf256_mul(mut x: u8, mut y: u8) -> u8 {
    let mut z = 0u8;
    for _ in 0..8 {
        if (y & 1) != 0 {
            z ^= x;
        }
        let hi = (x & 0x80) != 0;
        x <<= 1;
        if hi {
            x ^= 0x1D;
        }
        y >>= 1;
    }
    z
}

fn reed_solomon_generator(degree: usize) -> Vec<u8> {
    let mut result = vec![0u8; degree];
    result[degree - 1] = 1;
    let mut root = 1u8;
    for _ in 0..degree {
        for j in 0..degree {
            result[j] = gf256_mul(result[j], root);
            if j + 1 < degree {
                result[j] ^= result[j + 1];
            }
        }
        root = gf256_mul(root, 0x02);
    }
    result
}

fn reed_solomon_compute_remainder(data: &[u8], degree: usize) -> Vec<u8> {
    let gen = reed_solomon_generator(degree);
    let mut result = vec![0u8; degree];
    for &b in data {
        let factor = b ^ result[0];
        result.remove(0);
        result.push(0);
        for (r, &g) in result.iter_mut().zip(gen.iter()) {
            *r ^= gf256_mul(g, factor);
        }
    }
    result
}

fn set_func_module(
    size: usize,
    x: usize,
    y: usize,
    dark: bool,
    modules: &mut [bool],
    is_function: &mut [bool],
) {
    if x < size && y < size {
        modules[y * size + x] = dark;
        is_function[y * size + x] = true;
    }
}

fn draw_finder_pattern(
    size: usize,
    cx: isize,
    cy: isize,
    modules: &mut [bool],
    is_function: &mut [bool],
) {
    for dy in -4..=4isize {
        for dx in -4..=4isize {
            let xx = cx + dx;
            let yy = cy + dy;
            if xx >= 0 && (xx as usize) < size && yy >= 0 && (yy as usize) < size {
                let dist = dx.abs().max(dy.abs());
                let dark = dist != 2 && dist != 4;
                set_func_module(size, xx as usize, yy as usize, dark, modules, is_function);
            }
        }
    }
}

fn draw_alignment_pattern(
    size: usize,
    cx: usize,
    cy: usize,
    modules: &mut [bool],
    is_function: &mut [bool],
) {
    for dy in -2..=2isize {
        for dx in -2..=2isize {
            let dist = dx.abs().max(dy.abs());
            let xx = (cx as isize + dx) as usize;
            let yy = (cy as isize + dy) as usize;
            set_func_module(size, xx, yy, dist != 1, modules, is_function);
        }
    }
}

fn draw_function_patterns(
    spec: &VersionSpec,
    modules: &mut [bool],
    is_function: &mut [bool],
) {
    let size = spec.size;

    // Timing patterns
    for i in 0..size {
        set_func_module(size, 6, i, i % 2 == 0, modules, is_function);
        set_func_module(size, i, 6, i % 2 == 0, modules, is_function);
    }

    // 3 Finder patterns
    draw_finder_pattern(size, 3, 3, modules, is_function);
    draw_finder_pattern(size, (size - 4) as isize, 3, modules, is_function);
    draw_finder_pattern(size, 3, (size - 4) as isize, modules, is_function);

    // Alignment pattern for Versions 2..=4
    if let Some(coord) = spec.alignment_coord {
        draw_alignment_pattern(size, coord, coord, modules, is_function);
    }

    // Reserve format information areas
    draw_format_bits(size, 0, modules, is_function);
}

fn draw_format_bits(
    size: usize,
    mask: u32,
    modules: &mut [bool],
    is_function: &mut [bool],
) {
    // ECC Level L = 01
    let data = (0b01 << 3) | (mask & 0b111);
    let mut rem = data;
    for _ in 0..10 {
        rem = (rem << 1) ^ ((rem >> 9) * 0x537);
    }
    let bits = ((data << 10) | rem) ^ 0x5412;

    let bit = |i: usize| -> bool { ((bits >> i) & 1) != 0 };

    // Top-left
    for i in 0..=5 {
        set_func_module(size, 8, i, bit(i), modules, is_function);
    }
    set_func_module(size, 8, 7, bit(6), modules, is_function);
    set_func_module(size, 8, 8, bit(7), modules, is_function);
    set_func_module(size, 7, 8, bit(8), modules, is_function);
    for i in 9..15 {
        set_func_module(size, 14 - i, 8, bit(i), modules, is_function);
    }

    // Top-right and bottom-left
    for i in 0..8 {
        set_func_module(size, size - 1 - i, 8, bit(i), modules, is_function);
    }
    for i in 8..15 {
        set_func_module(size, 8, size - 15 + i, bit(i), modules, is_function);
    }
    // Always-dark module
    set_func_module(size, 8, size - 8, true, modules, is_function);
}

fn draw_codewords(
    size: usize,
    codewords: &[u8],
    modules: &mut [bool],
    is_function: &[bool],
) {
    let mut bit_idx = 0usize;
    let total_bits = codewords.len() * 8;

    let mut right = (size - 1) as isize;
    while right >= 1 {
        if right == 6 {
            right = 5;
        }
        for vert in 0..size {
            for j in 0..2 {
                let x = (right - j) as usize;
                let upward = ((right + 1) & 2) == 0;
                let y = if upward { size - 1 - vert } else { vert };
                if !is_function[y * size + x] && bit_idx < total_bits {
                    let cw = codewords[bit_idx / 8];
                    let dark = ((cw >> (7 - (bit_idx % 8))) & 1) != 0;
                    modules[y * size + x] = dark;
                    bit_idx += 1;
                }
            }
        }
        right -= 2;
    }
}

fn apply_mask_0(size: usize, modules: &mut [bool], is_function: &[bool]) {
    for y in 0..size {
        for x in 0..size {
            if !is_function[y * size + x] && (x + y) % 2 == 0 {
                modules[y * size + x] = !modules[y * size + x];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_qr_encode_wifi_and_url() {
        let wifi = QrCode::encode_text("WIFI:T:WPA;S:RetroParent;P:Abc1234567;;");
        assert!(wifi.size == 29 || wifi.size == 33);
        // Top-left finder pattern corner & center are dark
        assert!(wifi.get(0, 0));
        assert!(wifi.get(3, 3));
        assert!(!wifi.get(1, 1));

        let url = QrCode::encode_text("http://192.168.4.1");
        assert_eq!(url.size, 25);
        assert!(url.get(0, 0));
        assert!(url.get(3, 3));
    }
}
