#![forbid(unsafe_code)]

//! Scalar prediction primitives shared by the encoder and fast decoder.

mod deblock;
mod error;
mod inter;
mod intra;
mod motion_field;

pub use deblock::{CodedBlock, deblock_frame, filter_samples};
pub use error::PredictError;
pub use inter::{MotionVector, PlaneScale, clamp_motion_vector, predict_inter};
pub use intra::{IntraMode, predict_intra};
pub use motion_field::{BlockMotion, MotionField, ReferenceSlot};
