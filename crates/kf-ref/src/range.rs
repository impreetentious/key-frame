use kf_spec::V1_ASSETS;

use crate::ReferenceError;

const TOP: u32 = 1 << 24;

pub(crate) struct ReferenceRange<'a> {
    bytes: &'a [u8],
    offset: usize,
    range: u32,
    code: u32,
    contexts: [u16; 144],
}

impl<'a> ReferenceRange<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Result<Self, ReferenceError> {
        Self::with_contexts(bytes, initial_contexts())
    }

    pub(crate) fn with_contexts(
        bytes: &'a [u8],
        contexts: [u16; 144],
    ) -> Result<Self, ReferenceError> {
        let mut decoder = Self {
            bytes,
            offset: 0,
            range: 0xFFFF_FFFF,
            code: 0,
            contexts,
        };
        for _ in 0..5 {
            decoder.code = decoder.code.wrapping_shl(8) | u32::from(decoder.read_byte()?);
        }
        Ok(decoder)
    }

    pub(crate) fn context(&mut self, id: u16) -> Result<bool, ReferenceError> {
        let index = usize::from(id);
        let p1 = *self
            .contexts
            .get(index)
            .ok_or_else(|| self.error("context.id"))?;
        let symbol = self.bin(p1)?;
        let target = if symbol { 4096_i32 } else { 0_i32 };
        let updated = (i32::from(p1) + (target - i32::from(p1)).div_euclid(32)).clamp(1, 4095);
        self.contexts[index] =
            u16::try_from(updated).expect("invariant: reference context is clamped to u16");
        Ok(symbol)
    }

    pub(crate) fn bypass(&mut self) -> Result<bool, ReferenceError> {
        self.bin(2048)
    }

    pub(crate) fn contexts(self) -> [u16; 144] {
        self.contexts
    }

    fn bin(&mut self, p1: u16) -> Result<bool, ReferenceError> {
        let bound = (self.range >> 12) * (4096 - u32::from(p1));
        let symbol = if self.code < bound {
            self.range = bound;
            false
        } else {
            self.code -= bound;
            self.range -= bound;
            true
        };
        while self.range < TOP {
            self.range <<= 8;
            self.code = self.code.wrapping_shl(8) | u32::from(self.read_byte()?);
        }
        Ok(symbol)
    }

    fn read_byte(&mut self) -> Result<u8, ReferenceError> {
        let byte = self
            .bytes
            .get(self.offset)
            .copied()
            .ok_or_else(|| self.error("range.payload"))?;
        self.offset += 1;
        Ok(byte)
    }

    fn error(&self, element: &'static str) -> ReferenceError {
        ReferenceError::new(u32::try_from(self.offset).unwrap_or(u32::MAX), element)
    }
}

fn initial_contexts() -> [u16; 144] {
    let asset = V1_ASSETS
        .iter()
        .find(|asset| asset.name == "contexts.toml")
        .expect("invariant: kf-spec exposes contexts.toml");
    let mut values = Vec::new();
    for line in asset.contents.lines() {
        let Some(body) = line.strip_prefix("initial_p1 = [") else {
            continue;
        };
        let body = body
            .strip_suffix(']')
            .expect("invariant: checked context array has balanced brackets");
        values.extend(body.split(',').map(|value| {
            value
                .trim()
                .parse::<u16>()
                .expect("invariant: checked context initial is u16")
        }));
    }
    values
        .try_into()
        .expect("invariant: checked context asset has 144 initials")
}
