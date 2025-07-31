#![forbid(unsafe_code)]

//! Integer transform and quantization paths for Key Frame version 1.

mod error;
mod forward;
mod inverse;
mod matrix;
mod quant;

pub use error::TransformError;
pub use forward::forward_transform;
pub use inverse::inverse_transform;
pub use matrix::TransformSize;
pub use quant::{dequantize, dequantize_block, lambda_q8, quantize, quantize_block};
