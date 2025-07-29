/// Computes reflected Castagnoli CRC32C exactly as frozen in C.10.
#[must_use]
pub fn crc32c(bytes: &[u8]) -> u32 {
    let mut register = 0xFFFF_FFFF_u32;
    for &byte in bytes {
        register ^= u32::from(byte);
        for _ in 0..8 {
            let mask = 0_u32.wrapping_sub(register & 1);
            register = (register >> 1) ^ (0x82F6_3B78 & mask);
        }
    }
    register ^ 0xFFFF_FFFF
}

#[cfg(test)]
mod tests {
    use super::crc32c;

    #[test]
    fn independent_literal_vectors_match() {
        assert_eq!(crc32c(b""), 0x0000_0000);
        assert_eq!(crc32c(b"123456789"), 0xE306_9283);
    }
}
