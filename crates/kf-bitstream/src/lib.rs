#![forbid(unsafe_code)]

//! Versioned `.kfv` headers, packets, syntax, and resynchronization.

mod error;
mod packet;
mod scanner;
mod sequence;
mod syntax;

pub use error::BitstreamError;
pub use packet::{FRAME_HEADER_SIZE, FrameFlags, FramePacket, PacketHeader};
pub use scanner::{PacketScanner, ScanEvent};
pub use sequence::{BITSTREAM_VERSION, SEQUENCE_HEADER_SIZE, SequenceHeader};
pub use syntax::{
    BlockSize, FrameType, IntraMode, MotionVector, PartitionTree, PlaneClass, Prediction,
    ReferenceFrame, SyntaxReader, SyntaxWriter, TransformBlockSize,
};
