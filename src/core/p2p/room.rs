//! Room codes: 80 random bits plus the host's game port, written in
//! Crockford base32 with a check character, e.g. `7KQ2M9A-XC40PRT-1BZ8V3W`.
//! Everything else (network name, secret, addresses) is derived from them,
//! so a code is all a guest needs.
use sha2::{Digest, Sha256};

use crate::core::error::{AppError, AppResult};

const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const SEED_BYTES: usize = 10;
/// 96 payload bits in 20 characters, then one check character.
const CODE_CHARS: usize = 21;

/// The host's address inside the room's virtual network; guests get theirs
/// from EasyTier's DHCP in the same /24.
pub const HOST_IP: &str = "10.126.126.1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Room {
    seed: [u8; SEED_BYTES],
    /// The port the host's game listens on (its "Open to LAN" port).
    pub game_port: u16,
}

impl Room {
    pub fn new(game_port: u16) -> Self {
        let mut seed = [0u8; SEED_BYTES];
        // The first five bytes of a v4 UUID are all random.
        seed[..5].copy_from_slice(&uuid::Uuid::new_v4().as_bytes()[..5]);
        seed[5..].copy_from_slice(&uuid::Uuid::new_v4().as_bytes()[..5]);
        Self { seed, game_port }
    }

    pub fn code(&self) -> String {
        let mut payload = self.seed.to_vec();
        payload.extend_from_slice(&self.game_port.to_be_bytes());
        let mut chars: Vec<u8> = Vec::with_capacity(CODE_CHARS);
        let (mut buffer, mut bits) = (0u32, 0u32);
        for byte in &payload {
            buffer = buffer << 8 | u32::from(*byte);
            bits += 8;
            while bits >= 5 {
                bits -= 5;
                chars.push(ALPHABET[(buffer >> bits & 31) as usize]);
            }
        }
        if bits > 0 {
            chars.push(ALPHABET[(buffer << (5 - bits) & 31) as usize]);
        }
        chars.push(ALPHABET[check(&payload)]);
        let text = String::from_utf8(chars).unwrap_or_default();
        format!("{}-{}-{}", &text[..7], &text[7..14], &text[14..])
    }

    /// Reads a code as typed or pasted: case, spaces, dashes and the usual
    /// look-alikes (O for 0, I and L for 1) don't matter.
    pub fn parse(code: &str) -> AppResult<Self> {
        let invalid = || {
            AppError::new(
                "room_code",
                crate::core::i18n::pick("房间码不对：应为 21 位字母数字（可带横线），请向房主重新要一次。", "That room code isn't right: it has 21 letters and digits (dashes optional). Ask the host for it again."),
            )
        };
        let mut values = Vec::with_capacity(CODE_CHARS);
        for c in code.chars() {
            let c = match c.to_ascii_uppercase() {
                '-' | ' ' | '\t' | '\n' | '\r' => continue,
                'O' => '0',
                'I' | 'L' => '1',
                other => other,
            };
            let value = ALPHABET.iter().position(|&a| a as char == c).ok_or_else(invalid)?;
            values.push(value as u32);
        }
        if values.len() != CODE_CHARS {
            return Err(invalid());
        }
        let (body, check_char) = values.split_at(CODE_CHARS - 1);
        let mut payload = Vec::with_capacity(SEED_BYTES + 2);
        let (mut buffer, mut bits) = (0u32, 0u32);
        for value in body {
            buffer = buffer << 5 | value;
            bits += 5;
            if bits >= 8 {
                bits -= 8;
                payload.push((buffer >> bits & 0xff) as u8);
            }
        }
        // The four bits after the payload are padding and must be zero.
        if payload.len() != SEED_BYTES + 2 || buffer & ((1 << bits) - 1) != 0 || check(&payload) != check_char[0] as usize {
            return Err(invalid());
        }
        let mut seed = [0u8; SEED_BYTES];
        seed.copy_from_slice(&payload[..SEED_BYTES]);
        let game_port = u16::from_be_bytes([payload[SEED_BYTES], payload[SEED_BYTES + 1]]);
        if game_port == 0 {
            return Err(invalid());
        }
        Ok(Self { seed, game_port })
    }

    /// The EasyTier network name. Public nodes see it, so it reveals nothing
    /// about the secret.
    pub fn network_name(&self) -> String {
        format!("mcl-{}", &derive(b"mc-launcher network", &self.seed)[..20])
    }

    pub fn network_secret(&self) -> String {
        derive(b"mc-launcher secret", &self.seed)
    }
}

fn derive(label: &[u8], seed: &[u8]) -> String {
    let mut hash = Sha256::new();
    hash.update(label);
    hash.update(seed);
    hex::encode(hash.finalize())
}

fn check(payload: &[u8]) -> usize {
    (Sha256::digest(payload)[0] & 31) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_round_trips_and_tolerates_typing() {
        for port in [1, 25565, 54321, 65535] {
            let room = Room::new(port);
            let code = room.code();
            assert_eq!(code.len(), CODE_CHARS + 2);
            assert_eq!(Room::parse(&code).unwrap(), room);
            let sloppy = format!(" {} ", code.to_lowercase().replace('-', " ").replace('0', "o").replace('1', "l"));
            assert_eq!(Room::parse(&sloppy).unwrap(), room);
        }
    }

    #[test]
    fn rejects_typos_and_wrong_lengths() {
        let room = Room { seed: *b"0123456789", game_port: 25565 };
        let code = room.code().replace('-', "");
        let mut caught = 0;
        for i in 0..code.len() {
            let mut chars: Vec<char> = code.chars().collect();
            chars[i] = if chars[i] == 'Z' { 'Y' } else { 'Z' };
            if Room::parse(&chars.into_iter().collect::<String>()).is_err() {
                caught += 1;
            }
        }
        // A 5-bit check catches most single-character slips.
        assert!(caught >= code.len() - 3, "caught {caught}");
        assert!(Room::parse(&code[1..]).is_err());
        assert!(Room::parse("").is_err());
        assert!(Room::parse("ABCDEFG-HIJKLMN-OPQRSTU!").is_err());
    }

    #[test]
    fn credentials_are_stable_and_distinct() {
        let room = Room::parse(&Room::new(25565).code()).unwrap();
        assert_eq!(room.network_name(), room.network_name());
        assert!(room.network_name().starts_with("mcl-"));
        assert_eq!(room.network_secret().len(), 64);
        assert!(!room.network_secret().contains(&room.network_name()[4..]));
        assert_ne!(Room::new(25565).network_name(), room.network_name());
    }
}
