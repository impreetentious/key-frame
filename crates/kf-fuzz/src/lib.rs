#![forbid(unsafe_code)]

//! Deterministic decoder campaigns for preflight smoke and nightly budgets.

use std::panic::{self, AssertUnwindSafe};

use kf_core::{Xoshiro256PlusPlus, crc32c};
use kf_dec::FastDecoder;
use kf_frame::Frame;
use kf_ref::ReferenceDecoder;

/// Nightly iteration count per target. The budget is a counter, not a clock.
pub const NIGHTLY_ITERATIONS: u32 = 20_000_000;

/// Preflight and unit-test iteration count per target.
pub const SMOKE_ITERATIONS: u32 = 256;

const ORACLE: &[u8] = include_bytes!("../../../conformance/oracle/intra64_dc_all_zero.kfv");
const INTER: &[u8] = include_bytes!("../../../conformance/encoder/inter_motion64_qp32.kfv");

/// One of the four decoder campaigns.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FuzzTarget {
    /// Completely unstructured random buffers.
    ArbitraryBytes,
    /// Bit flips, truncations, length lies, false syncs, and CRC damage.
    StructuredMutation,
    /// Mutations confined to the sequence header.
    HeaderFuzz,
    /// Deletion of a whole packet from a valid multi-frame stream.
    FrameDeletion,
}

impl FuzzTarget {
    /// Every campaign the nightly job must run.
    pub const ALL: [Self; 4] = [
        Self::ArbitraryBytes,
        Self::StructuredMutation,
        Self::HeaderFuzz,
        Self::FrameDeletion,
    ];

    /// Stable name used in logs and failure messages.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::ArbitraryBytes => "arbitrary-bytes",
            Self::StructuredMutation => "structured-mutation",
            Self::HeaderFuzz => "header-fuzz",
            Self::FrameDeletion => "frame-deletion",
        }
    }
}

/// Completed campaign with no panics, hangs, or decoder disagreement.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CampaignReport {
    pub target: FuzzTarget,
    pub iterations: u32,
}

/// Runs `iterations` probes against one target.
pub fn run_campaign(target: FuzzTarget, iterations: u32) -> Result<CampaignReport, String> {
    let mut rng = Xoshiro256PlusPlus::from_state([
        0x9e37_79b9_7f4a_7c15,
        0xf39c_c060_5ced_c834,
        0x6254_85b3_6ee9_0e49,
        match target {
            FuzzTarget::ArbitraryBytes => 1,
            FuzzTarget::StructuredMutation => 2,
            FuzzTarget::HeaderFuzz => 3,
            FuzzTarget::FrameDeletion => 4,
        },
    ]);
    for iteration in 0..iterations {
        let input = mutate(target, &mut rng);
        probe(&input).map_err(|error| {
            format!(
                "{} iteration {iteration} ({error}); {} input bytes; hex={}",
                target.name(),
                input.len(),
                hex_preview(&input)
            )
        })?;
    }
    Ok(CampaignReport { target, iterations })
}

fn probe(bytes: &[u8]) -> Result<(), String> {
    let fast = panic::catch_unwind(AssertUnwindSafe(|| run_fast(bytes)));
    let reference = panic::catch_unwind(AssertUnwindSafe(|| run_reference(bytes)));
    let fast = fast.map_err(|_| "fast decoder panicked".to_owned())?;
    let reference = reference.map_err(|_| "reference decoder panicked".to_owned())?;
    match (fast, reference) {
        (Probe::Decoded(left), Probe::Decoded(right)) => {
            if left != right {
                return Err("decoders disagree on a reconstructed stream".to_owned());
            }
            Ok(())
        }
        (
            Probe::Failed {
                has_references: true,
                ..
            },
            _,
        )
        | (
            _,
            Probe::Failed {
                has_references: true,
                ..
            },
        ) => Err("failed decode retained a reference".to_owned()),
        (Probe::Decoded(_), Probe::Failed { .. }) => {
            Err("fast decoder accepted a stream the reference decoder rejected".to_owned())
        }
        (Probe::Failed { .. }, Probe::Decoded(_)) => {
            Err("reference decoder accepted a stream the fast decoder rejected".to_owned())
        }
        (Probe::Failed { .. }, Probe::Failed { .. }) => Ok(()),
    }
}

enum Probe {
    Decoded(Vec<Frame>),
    Failed { has_references: bool },
}

fn run_fast(bytes: &[u8]) -> Probe {
    let mut decoder = FastDecoder::new();
    match decoder.decode_stream(bytes) {
        Ok(frames) => Probe::Decoded(frames),
        Err(_) => Probe::Failed {
            has_references: decoder.has_references(),
        },
    }
}

fn run_reference(bytes: &[u8]) -> Probe {
    let mut decoder = ReferenceDecoder::new();
    match decoder.decode_stream(bytes) {
        Ok(frames) => Probe::Decoded(frames),
        Err(_) => Probe::Failed {
            has_references: decoder.has_references(),
        },
    }
}

fn mutate(target: FuzzTarget, rng: &mut Xoshiro256PlusPlus) -> Vec<u8> {
    match target {
        FuzzTarget::ArbitraryBytes => {
            let len = 1 + take(rng, 384);
            random_bytes(rng, len)
        }
        FuzzTarget::StructuredMutation => {
            let bytes = choose_seed(rng);
            structured(rng, bytes)
        }
        FuzzTarget::HeaderFuzz => {
            let bytes = choose_seed(rng);
            header_fuzz(rng, bytes)
        }
        FuzzTarget::FrameDeletion => delete_packet(rng, INTER.to_vec()),
    }
}

fn choose_seed(rng: &mut Xoshiro256PlusPlus) -> Vec<u8> {
    if take(rng, 2) == 0 {
        ORACLE.to_vec()
    } else {
        INTER.to_vec()
    }
}

fn structured(rng: &mut Xoshiro256PlusPlus, mut bytes: Vec<u8>) -> Vec<u8> {
    if bytes.is_empty() {
        return random_bytes(rng, 16);
    }
    match take(rng, 5) {
        0 => {
            let index = take(rng, bytes.len() as u32) as usize;
            bytes[index] ^= 1 << take(rng, 8);
            bytes
        }
        1 => {
            let keep = take(rng, bytes.len() as u32 + 1) as usize;
            bytes.truncate(keep);
            bytes
        }
        2 => {
            insert_false_sync(rng, &mut bytes);
            bytes
        }
        3 => {
            lie_about_payload_len(rng, &mut bytes);
            bytes
        }
        _ => {
            if bytes.len() > 20 {
                let index = 20 + take(rng, 4) as usize;
                if index < bytes.len() {
                    bytes[index] ^= 0xFF;
                }
            }
            bytes
        }
    }
}

fn header_fuzz(rng: &mut Xoshiro256PlusPlus, mut bytes: Vec<u8>) -> Vec<u8> {
    let span = bytes.len().min(24);
    if span == 0 {
        return random_bytes(rng, 24);
    }
    let index = take(rng, span as u32) as usize;
    bytes[index] ^= 1 << take(rng, 8);
    bytes
}

fn delete_packet(rng: &mut Xoshiro256PlusPlus, mut bytes: Vec<u8>) -> Vec<u8> {
    let starts = packet_starts(&bytes);
    if starts.len() < 2 {
        return structured(rng, bytes);
    }
    let choice = take(rng, starts.len() as u32 - 1) as usize + 1;
    let start = starts[choice];
    let end = starts.get(choice + 1).copied().unwrap_or(bytes.len());
    bytes.drain(start..end);
    bytes
}

fn packet_starts(bytes: &[u8]) -> Vec<usize> {
    let mut starts = Vec::new();
    let mut offset = 0;
    while offset + 4 <= bytes.len() {
        if &bytes[offset..offset + 4] == b"KFP1" {
            starts.push(offset);
            offset += 4;
        } else {
            offset += 1;
        }
    }
    starts
}

fn insert_false_sync(rng: &mut Xoshiro256PlusPlus, bytes: &mut Vec<u8>) {
    let at = take(rng, bytes.len() as u32 + 1) as usize;
    let mut injected = b"KFP1".to_vec();
    injected.extend(random_bytes(rng, 8));
    bytes.splice(at..at, injected);
}

fn lie_about_payload_len(rng: &mut Xoshiro256PlusPlus, bytes: &mut [u8]) {
    let Some(start) = packet_starts(bytes).first().copied() else {
        return;
    };
    if start + 16 > bytes.len() {
        return;
    }
    let lied = 5 + take(rng, 4096);
    bytes[start + 4..start + 8].copy_from_slice(&lied.to_le_bytes());
    let header_crc = crc32c(&bytes[start + 4..start + 16]);
    bytes[start + 16..start + 20].copy_from_slice(&header_crc.to_le_bytes());
}

fn random_bytes(rng: &mut Xoshiro256PlusPlus, len: u32) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(len as usize);
    while bytes.len() < len as usize {
        bytes.extend_from_slice(&rng.next_u64().to_le_bytes());
    }
    bytes.truncate(len as usize);
    bytes
}

fn take(rng: &mut Xoshiro256PlusPlus, max_exclusive: u32) -> u32 {
    if max_exclusive <= 1 {
        0
    } else {
        u32::try_from(rng.next_u64() % u64::from(max_exclusive)).unwrap_or(0)
    }
}

fn hex_preview(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
