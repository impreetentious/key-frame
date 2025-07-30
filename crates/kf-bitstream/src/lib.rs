#![forbid(unsafe_code)]

//! Versioned `.kfv` headers, packets, and resynchronization.

mod error;
mod packet;
mod scanner;
mod sequence;

pub use error::BitstreamError;
pub use packet::{FRAME_HEADER_SIZE, FrameFlags, FramePacket, PacketHeader};
pub use scanner::PacketScanner;
pub use sequence::{BITSTREAM_VERSION, SEQUENCE_HEADER_SIZE, SequenceHeader};
