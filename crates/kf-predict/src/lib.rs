#![forbid(unsafe_code)]

//! Scalar prediction primitives shared by the encoder and fast decoder.

mod error;
mod inter;
mod intra;
mod motion_field;

pub use error::PredictError;
pub use inter::{MotionVector, PlaneScale, clamp_motion_vector, predict_inter};
pub use intra::{IntraMode, predict_intra};
pub use motion_field::{BlockMotion, MotionField, ReferenceSlot};
