use std::sync::OnceLock;

use kf_spec::V1_ASSETS;

use crate::{ReferenceError, crc::crc32c, reader::ByteReader};

const FRAME_HEADER_SIZE: usize = 24;

/// The payload bounds this scanner admits, read from the frozen constants.
///
/// Both were literals here — `16 * 1024 * 1024` and a bare `5` — matching the
/// literals the production packet layer carries. That made the two decoders
/// agree about which packets are structurally admissible because two people
/// typed the same numbers, rather than because both read the specification, and
/// nothing anywhere compared them: `crates/kf-bitstream/tests/normative_limits.rs`
/// drives the production constructor with the declared values, and there was no
/// counterpart on this side. A vector at either boundary would have caught a
/// drift, and no vector reaches them — the upper one is sixteen megabytes.
///
/// Reading the same asset is not sharing an implementation. This parses it with
/// this crate's own reader, into this crate's own constants, and disagreement
/// with the specification remains the only way the two decoders can disagree
/// while both looking correct.
struct PayloadBounds {
    minimum: u32,
    maximum: u32,
}

fn payload_bounds() -> &'static PayloadBounds {
    static BOUNDS: OnceLock<PayloadBounds> = OnceLock::new();
    BOUNDS.get_or_init(|| PayloadBounds {
        minimum: declared("decoder_initial_bytes"),
        maximum: declared("max_payload_bytes"),
    })
}

fn declared(key: &str) -> u32 {
    let asset = V1_ASSETS
        .iter()
        .find(|asset| asset.name == "constants.toml")
        .expect("invariant: kf-spec exposes constants.toml");
    asset
        .contents
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once('=')?;
            (name.trim() == key).then(|| value.trim().parse::<u32>().ok())?
        })
        .expect("invariant: checked constants declare the packet payload bounds")
}

pub(crate) struct RefPacket<'a> {
    pub payload: &'a [u8],
    pub frame_index: u32,
    pub qp: u8,
    pub key: bool,
    pub golden_refresh: bool,
    pub consumed: usize,
}

/// Header facts a recovering decoder still needs when the payload is unusable.
#[derive(Clone, Copy)]
pub(crate) struct RefHeaderFacts {
    pub frame_index: u32,
    pub key: bool,
}

/// One outcome of advancing the independent cursor.
pub(crate) enum RefScanEvent<'a> {
    Packet(RefPacket<'a>),
    /// Header validated, payload CRC failed; the declared extent was consumed.
    PayloadCorrupt(RefHeaderFacts),
    /// Header validated, payload runs past the end of the region.
    Truncated(RefHeaderFacts),
    End,
}

/// Independent packet cursor. Failed sync or header candidates advance one byte.
pub(crate) struct PacketCursor<'a> {
    bytes: &'a [u8],
    offset: usize,
    last_seen_index: Option<u32>,
}

impl<'a> PacketCursor<'a> {
    pub(crate) const fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            offset: 0,
            last_seen_index: None,
        }
    }

    pub(crate) fn next_packet(&mut self) -> Result<Option<RefPacket<'a>>, ReferenceError> {
        while self.offset + 4 <= self.bytes.len() {
            if &self.bytes[self.offset..self.offset + 4] != b"KFP1" {
                self.offset += 1;
                continue;
            }
            let remaining = &self.bytes[self.offset..];
            let header = match read_header(remaining) {
                Ok(header) => header,
                Err(_) => {
                    self.offset += 1;
                    continue;
                }
            };
            if self
                .last_seen_index
                .is_some_and(|previous| header.frame_index <= previous)
            {
                self.offset += 1;
                continue;
            }
            let extent = FRAME_HEADER_SIZE
                + usize::try_from(header.payload_len)
                    .map_err(|_| ReferenceError::new(4, "packet.payload_len"))?;
            self.last_seen_index = Some(header.frame_index);
            if remaining.len() < extent {
                self.offset = self.bytes.len();
                return Err(ReferenceError::new(
                    u32::try_from(self.bytes.len()).unwrap_or(u32::MAX),
                    "packet.payload",
                ));
            }
            match read_packet(remaining) {
                Ok(packet) => {
                    self.offset += packet.consumed;
                    return Ok(Some(packet));
                }
                Err(error) if error.element == "packet.payload_crc" => {
                    self.offset += extent;
                    return Err(error);
                }
                Err(_) => {
                    self.offset += 1;
                }
            }
        }
        Ok(None)
    }
}

impl<'a> PacketCursor<'a> {
    /// Advances to the next outcome, distinguishing damage that can be stepped
    /// over from damage that ends the region. Byte advancement is identical to
    /// `next_packet`, so the two agree on where every packet sits.
    pub(crate) fn next_event(&mut self) -> RefScanEvent<'a> {
        while self.offset + 4 <= self.bytes.len() {
            if &self.bytes[self.offset..self.offset + 4] != b"KFP1" {
                self.offset += 1;
                continue;
            }
            let remaining = &self.bytes[self.offset..];
            let Ok(header) = read_header(remaining) else {
                self.offset += 1;
                continue;
            };
            if self
                .last_seen_index
                .is_some_and(|previous| header.frame_index <= previous)
            {
                self.offset += 1;
                continue;
            }
            let Ok(payload_len) = usize::try_from(header.payload_len) else {
                self.offset += 1;
                continue;
            };
            let extent = FRAME_HEADER_SIZE + payload_len;
            self.last_seen_index = Some(header.frame_index);
            let facts = RefHeaderFacts {
                frame_index: header.frame_index,
                key: header.key,
            };
            if remaining.len() < extent {
                self.offset = self.bytes.len();
                return RefScanEvent::Truncated(facts);
            }
            match read_packet(remaining) {
                Ok(packet) => {
                    self.offset += packet.consumed;
                    return RefScanEvent::Packet(packet);
                }
                Err(error) if error.element == "packet.payload_crc" => {
                    self.offset += extent;
                    return RefScanEvent::PayloadCorrupt(facts);
                }
                Err(_) => {
                    self.offset += 1;
                }
            }
        }
        RefScanEvent::End
    }
}

#[derive(Debug)]
struct PacketHeader {
    payload_len: u32,
    frame_index: u32,
    key: bool,
}

fn read_header(bytes: &[u8]) -> Result<PacketHeader, ReferenceError> {
    if bytes.len() < FRAME_HEADER_SIZE {
        return Err(ReferenceError::new(
            u32::try_from(bytes.len()).unwrap_or(u32::MAX),
            "packet.header",
        ));
    }
    if &bytes[..4] != b"KFP1" {
        return Err(ReferenceError::new(0, "packet.sync"));
    }
    let payload_len = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    let bounds = payload_bounds();
    if !(bounds.minimum..=bounds.maximum).contains(&payload_len) {
        return Err(ReferenceError::new(4, "packet.payload_len"));
    }
    let flag_bits = bytes[12];
    let key = flag_bits & 1 != 0;
    let golden_refresh = flag_bits & 2 != 0;
    let show = flag_bits & 4 != 0;
    if flag_bits & 0xf8 != 0 || !show || (key && !golden_refresh) {
        return Err(ReferenceError::new(12, "packet.flags"));
    }
    if bytes[13] > 63 || u16::from_le_bytes([bytes[14], bytes[15]]) != 0 {
        return Err(ReferenceError::new(13, "packet.qp_or_reserved"));
    }
    let header_crc = u32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    if crc32c(&bytes[4..16]) != header_crc {
        return Err(ReferenceError::new(16, "packet.header_crc"));
    }
    Ok(PacketHeader {
        payload_len,
        frame_index: u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
        key,
    })
}

fn read_packet(bytes: &[u8]) -> Result<RefPacket<'_>, ReferenceError> {
    let mut reader = ByteReader::new(bytes);
    if reader.bytes(4, "packet.sync")? != b"KFP1" {
        return Err(ReferenceError::new(0, "packet.sync"));
    }
    let payload_len = reader.u32("packet.payload_len")?;
    let bounds = payload_bounds();
    if !(bounds.minimum..=bounds.maximum).contains(&payload_len) {
        return Err(ReferenceError::new(4, "packet.payload_len"));
    }
    let frame_index = reader.u32("packet.frame_index")?;
    let flag_bits = reader.u8("packet.flags")?;
    let key = flag_bits & 1 != 0;
    let golden_refresh = flag_bits & 2 != 0;
    let show = flag_bits & 4 != 0;
    if flag_bits & 0xf8 != 0 || !show || (key && !golden_refresh) {
        return Err(ReferenceError::new(12, "packet.flags"));
    }
    let qp = reader.u8("packet.qp")?;
    if qp > 63 || reader.u16("packet.reserved")? != 0 {
        return Err(ReferenceError::new(13, "packet.qp_or_reserved"));
    }
    let header_crc = reader.u32("packet.header_crc")?;
    let payload_crc = reader.u32("packet.payload_crc")?;
    if crc32c(&bytes[4..16]) != header_crc {
        return Err(ReferenceError::new(16, "packet.header_crc"));
    }
    let payload = reader.bytes(
        usize::try_from(payload_len).map_err(|_| ReferenceError::new(4, "packet.payload_len"))?,
        "packet.payload",
    )?;
    if crc32c(payload) != payload_crc {
        return Err(ReferenceError::new(20, "packet.payload_crc"));
    }
    Ok(RefPacket {
        payload,
        frame_index,
        qp,
        key,
        golden_refresh,
        consumed: reader.offset(),
    })
}

#[cfg(test)]
mod tests {
    use super::{FRAME_HEADER_SIZE, payload_bounds, read_header};
    use crate::crc::crc32c;

    /// A structurally valid header declaring `payload_len`, sealed so that the
    /// only thing that can refuse it is the bound under test.
    fn header(payload_len: u32) -> Vec<u8> {
        let mut bytes = vec![0_u8; FRAME_HEADER_SIZE];
        bytes[..4].copy_from_slice(b"KFP1");
        bytes[4..8].copy_from_slice(&payload_len.to_le_bytes());
        // Frame zero, key, golden refresh, shown: the one flag combination a
        // first packet may carry.
        bytes[12] = 0b0000_0111;
        bytes[13] = 32;
        let checksum = crc32c(&bytes[4..16]);
        bytes[16..20].copy_from_slice(&checksum.to_le_bytes());
        bytes
    }

    #[test]
    fn the_payload_bounds_are_the_ones_the_specification_declares() {
        // The reference decoder held these as literals matching the production
        // decoder's literals, so the two agreed with each other rather than
        // with the document, and nothing compared them. No committed vector
        // reaches either boundary — the upper one is sixteen megabytes — so the
        // header is driven directly.
        let bounds = payload_bounds();

        assert_eq!(
            read_header(&header(bounds.maximum + 1))
                .expect_err("a payload past the declared cap was admitted")
                .element,
            "packet.payload_len"
        );
        assert_eq!(
            read_header(&header(bounds.minimum - 1))
                .expect_err("a payload under the decoder's initial read was admitted")
                .element,
            "packet.payload_len"
        );

        // And the positive half, so this cannot pass by refusing everything.
        // Only the length is under test here; the header carries no payload, so
        // whatever happens next is a different question.
        for length in [bounds.minimum, bounds.maximum] {
            assert_eq!(
                read_header(&header(length))
                    .expect("a length inside the declared bounds was refused")
                    .payload_len,
                length
            );
        }
    }
}
