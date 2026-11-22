use anyhow::{anyhow, bail, Context, Result};
use constant_time_eq::constant_time_eq;
use serde::{de::DeserializeOwned, Deserialize, Serialize};

/// Compiled-in device HMAC secret key for signing `config.json` and `usage.json`.
/// Can be overridden at compile time via `QUOS_HMAC_SECRET`.
const DEVICE_HMAC_KEY: &[u8] = match option_env!("QUOS_HMAC_SECRET") {
    Some(s) => s.as_bytes(),
    None => b"quOS-RG35XXSP-muOS-Parental-HMAC-Secret-Key-v1-9f8e7d6c5b4a3210",
};

/// FIPS 180-4 SHA-256 implementation.
#[derive(Clone)]
pub struct Sha256 {
    state: [u32; 8],
    buffer: [u8; 64],
    buffer_len: usize,
    total_bits: u64,
}

const K256: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

impl Default for Sha256 {
    fn default() -> Self {
        Self::new()
    }
}

impl Sha256 {
    pub fn new() -> Self {
        Self {
            state: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
                0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
            ],
            buffer: [0u8; 64],
            buffer_len: 0,
            total_bits: 0,
        }
    }

    fn compress(state: &mut [u32; 8], block: &[u8; 64]) {
        let mut w = [0u32; 64];
        for (i, chunk) in block.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let mut a = state[0];
        let mut b = state[1];
        let mut c = state[2];
        let mut d = state[3];
        let mut e = state[4];
        let mut f = state[5];
        let mut g = state[6];
        let mut h = state[7];

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K256[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
        state[4] = state[4].wrapping_add(e);
        state[5] = state[5].wrapping_add(f);
        state[6] = state[6].wrapping_add(g);
        state[7] = state[7].wrapping_add(h);
    }

    pub fn update(&mut self, mut data: &[u8]) {
        self.total_bits = self.total_bits.wrapping_add((data.len() as u64) * 8);

        if self.buffer_len > 0 {
            let needed = 64 - self.buffer_len;
            if data.len() >= needed {
                self.buffer[self.buffer_len..64].copy_from_slice(&data[..needed]);
                let block = self.buffer;
                Self::compress(&mut self.state, &block);
                self.buffer_len = 0;
                data = &data[needed..];
            } else {
                self.buffer[self.buffer_len..self.buffer_len + data.len()].copy_from_slice(data);
                self.buffer_len += data.len();
                return;
            }
        }

        while data.len() >= 64 {
            let mut block = [0u8; 64];
            block.copy_from_slice(&data[..64]);
            Self::compress(&mut self.state, &block);
            data = &data[64..];
        }

        if !data.is_empty() {
            self.buffer[..data.len()].copy_from_slice(data);
            self.buffer_len = data.len();
        }
    }

    pub fn finalize(mut self) -> [u8; 32] {
        let bit_len = self.total_bits;
        self.buffer[self.buffer_len] = 0x80;
        self.buffer_len += 1;

        if self.buffer_len > 56 {
            for b in &mut self.buffer[self.buffer_len..64] {
                *b = 0;
            }
            let block = self.buffer;
            Self::compress(&mut self.state, &block);
            self.buffer_len = 0;
        }

        for b in &mut self.buffer[self.buffer_len..56] {
            *b = 0;
        }
        self.buffer[56..64].copy_from_slice(&bit_len.to_be_bytes());
        let block = self.buffer;
        Self::compress(&mut self.state, &block);

        let mut out = [0u8; 32];
        for (i, word) in self.state.iter().enumerate() {
            out[i * 4..(i + 1) * 4].copy_from_slice(&word.to_be_bytes());
        }
        out
    }

    pub fn digest(data: &[u8]) -> [u8; 32] {
        let mut hasher = Self::new();
        hasher.update(data);
        hasher.finalize()
    }
}

/// RFC 2104 HMAC-SHA256 implementation.
pub fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    let mut key_block = [0u8; 64];
    if key.len() > 64 {
        let hashed = Sha256::digest(key);
        key_block[..32].copy_from_slice(&hashed);
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }

    let mut ipad = [0x36u8; 64];
    let mut opad = [0x5cu8; 64];
    for i in 0..64 {
        ipad[i] ^= key_block[i];
        opad[i] ^= key_block[i];
    }

    let mut inner = Sha256::new();
    inner.update(&ipad);
    inner.update(message);
    let inner_hash = inner.finalize();

    let mut outer = Sha256::new();
    outer.update(&opad);
    outer.update(&inner_hash);
    outer.finalize()
}

pub fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

pub fn hex_decode(s: &str) -> Result<Vec<u8>> {
    if s.len() % 2 != 0 {
        bail!("odd hex string length");
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    let bytes = s.as_bytes();
    for i in (0..bytes.len()).step_by(2) {
        let hi = from_hex_digit(bytes[i])?;
        let lo = from_hex_digit(bytes[i + 1])?;
        out.push((hi << 4) | lo);
    }
    Ok(out)
}

fn from_hex_digit(b: u8) -> Result<u8> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => Err(anyhow!("invalid hex character")),
    }
}

/// Signed JSON wrapper persisted on disk for `config.json` and `usage.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedFile {
    pub payload: String,
    pub hmac_sha256: String,
}

pub fn sign_payload<T: Serialize>(value: &T) -> Result<SignedFile> {
    let payload = serde_json::to_string_pretty(value).context("serialize payload")?;
    let mac = hmac_sha256(DEVICE_HMAC_KEY, payload.as_bytes());
    Ok(SignedFile {
        payload,
        hmac_sha256: hex_encode(&mac),
    })
}

pub fn verify_and_deserialize<T: DeserializeOwned>(raw_json: &str) -> Result<T> {
    let signed: SignedFile =
        serde_json::from_str(raw_json).context("parse SignedFile envelope")?;
    let expected_mac = hmac_sha256(DEVICE_HMAC_KEY, signed.payload.as_bytes());
    let actual_mac = hex_decode(&signed.hmac_sha256).context("decode hmac_sha256 hex")?;

    if !constant_time_eq(&expected_mac, &actual_mac) {
        bail!("HMAC-SHA256 signature verification failed (tampering detected)");
    }

    let value: T =
        serde_json::from_str(&signed.payload).context("deserialize verified payload")?;
    Ok(value)
}

/// Generates a random 16-byte hex salt.
pub fn generate_salt() -> String {
    let mut bytes = [0u8; 16];
    for b in &mut bytes {
        *b = fastrand::u8(..);
    }
    hex_encode(&bytes)
}

/// Generates a random 8-character alphanumeric WPA2 passphrase (unambiguous characters).
pub fn generate_wifi_passphrase() -> String {
    const CHARSET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789";
    (0..10)
        .map(|_| {
            let idx = fastrand::usize(..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
}

/// Hashes a 4-digit PIN using 10,000 rounds of iterated HMAC-SHA256 with a random per-PIN salt.
pub fn hash_pin(pin: &str, salt: &str) -> String {
    let mut acc = hmac_sha256(salt.as_bytes(), pin.as_bytes());
    for round in 0..10_000u32 {
        let mut msg = [0u8; 36];
        msg[..32].copy_from_slice(&acc);
        msg[32..36].copy_from_slice(&round.to_be_bytes());
        acc = hmac_sha256(DEVICE_HMAC_KEY, &msg);
    }
    hex_encode(&acc)
}

/// Verifies a candidate PIN against the stored salt and hash in constant time.
pub fn verify_pin(candidate_pin: &str, salt: &str, expected_hash_hex: &str) -> bool {
    let candidate_hash = hash_pin(candidate_pin, salt);
    constant_time_eq(candidate_hash.as_bytes(), expected_hash_hex.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[test]
    fn test_sha256_nist_vectors() {
        // Empty string
        assert_eq!(
            hex_encode(&Sha256::digest(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        // "abc"
        assert_eq!(
            hex_encode(&Sha256::digest(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        // Multi-block NIST vector
        assert_eq!(
            hex_encode(&Sha256::digest(
                b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
            )),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    #[test]
    fn test_hmac_sha256_rfc4231_vectors() {
        // RFC 4231 Test Case 1
        let key1 = [0x0bu8; 20];
        let data1 = b"Hi There";
        assert_eq!(
            hex_encode(&hmac_sha256(&key1, data1)),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );

        // RFC 4231 Test Case 2
        let key2 = b"Jefe";
        let data2 = b"what do ya want for nothing?";
        assert_eq!(
            hex_encode(&hmac_sha256(key2, data2)),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    #[test]
    fn test_signed_file_roundtrip_and_tamper_detection() {
        #[derive(Debug, PartialEq, Serialize, Deserialize)]
        struct Dummy {
            quota: u32,
        }

        let orig = Dummy { quota: 45 };
        let signed = sign_payload(&orig).unwrap();
        let raw = serde_json::to_string(&signed).unwrap();

        let loaded: Dummy = verify_and_deserialize(&raw).unwrap();
        assert_eq!(loaded, orig);

        // Tamper with payload
        let mut tampered = signed.clone();
        tampered.payload = serde_json::to_string_pretty(&Dummy { quota: 999 }).unwrap();
        let tampered_raw = serde_json::to_string(&tampered).unwrap();
        assert!(verify_and_deserialize::<Dummy>(&tampered_raw).is_err());
    }

    #[test]
    fn test_pin_hash_and_verify() {
        let salt = generate_salt();
        let hashed = hash_pin("1234", &salt);
        assert!(verify_pin("1234", &salt, &hashed));
        assert!(!verify_pin("0000", &salt, &hashed));
    }
}
