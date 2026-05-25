use std::sync::OnceLock;

use kf_spec::V1_ASSETS;

use crate::{
    BitstreamError, FrameType, IntraMode, MotionVector, Prediction, ReferenceFrame, SyntaxElement,
    SyntaxReader, SyntaxWriter,
};

/// The largest motion-vector difference the syntax admits, in quarter pels.
///
/// Derived, not written. Two vectors inside the declared full-pixel range can
/// differ by the whole width of that range, and the difference is coded at the
/// declared fractional precision — so the bound is
/// `(mv_fullpel_max - mv_fullpel_min) << mv_fractional_bits`. It was `512` here,
/// which is that arithmetic done once by hand: the three declarations it comes
/// from were reconciled against `mc.toml` and read by nothing that codes a
/// vector, so widening the declared range would have left this coder rejecting
/// differences the specification allows, with no vector anywhere near the bound
/// to notice.
fn max_mvd_magnitude() -> u32 {
    static MAX: OnceLock<u32> = OnceLock::new();
    *MAX.get_or_init(|| {
        let span = declared("mv_fullpel_max") - declared("mv_fullpel_min");
        let fractional = u32::try_from(declared("mv_fractional_bits"))
            .expect("invariant: a fractional-bit count is not negative");
        u32::try_from(span).expect("invariant: the declared range is not negative") << fractional
    })
}

/// One declared scalar from the frozen constants.
fn declared(key: &str) -> i64 {
    let asset = V1_ASSETS
        .iter()
        .find(|asset| asset.name == "constants.toml")
        .expect("invariant: kf-spec exposes constants.toml");
    let prefix = format!("{key} = ");
    asset
        .contents
        .lines()
        .find_map(|line| line.strip_prefix(&prefix)?.trim().parse().ok())
        .unwrap_or_else(|| panic!("invariant: checked constants declare {key}"))
}

impl SyntaxWriter {
    /// Writes the complete frame-legal prediction-kind branch for one block.
    pub fn write_prediction(
        &mut self,
        frame_type: FrameType,
        prediction: Prediction,
    ) -> Result<(), BitstreamError> {
        match (frame_type, prediction) {
            (FrameType::Key, Prediction::Intra(mode)) => self.write_intra_mode(mode),
            (FrameType::Key, _) => Err(invalid("prediction.key_inter")),
            (FrameType::P, Prediction::Skip { reference }) => {
                self.record_element(SyntaxElement::Skip);
                self.context(12, true)?;
                self.write_reference(reference)
            }
            (FrameType::P, Prediction::Intra(mode)) => {
                self.record_element(SyntaxElement::Skip);
                self.context(12, false)?;
                self.record_element(SyntaxElement::IsInter);
                self.context(15, false)?;
                self.write_intra_mode(mode)
            }
            (FrameType::P, Prediction::Inter { reference, mvd }) => {
                self.record_element(SyntaxElement::Skip);
                self.context(12, false)?;
                self.record_element(SyntaxElement::IsInter);
                self.context(15, true)?;
                self.write_reference(reference)?;
                self.write_mvd(mvd)
            }
        }
    }

    fn write_intra_mode(&mut self, mode: IntraMode) -> Result<(), BitstreamError> {
        self.record_element(SyntaxElement::IntraMode);
        let index = mode.index();
        for (shift, context) in [(2, 18), (1, 21), (0, 22)] {
            self.context(context, ((index >> shift) & 1) != 0)?;
        }
        Ok(())
    }

    fn write_reference(&mut self, reference: ReferenceFrame) -> Result<(), BitstreamError> {
        self.record_element(SyntaxElement::RefSelect);
        self.context(28, reference == ReferenceFrame::Golden)
    }

    fn write_mvd(&mut self, mvd: MotionVector) -> Result<(), BitstreamError> {
        self.record_element(SyntaxElement::Mvd);
        self.write_signed_exp_golomb(mvd.x_q4, 30)?;
        self.write_signed_exp_golomb(mvd.y_q4, 33)
    }

    fn write_signed_exp_golomb(
        &mut self,
        value: i16,
        context_base: u16,
    ) -> Result<(), BitstreamError> {
        let magnitude = u32::from(value.unsigned_abs());
        if magnitude > max_mvd_magnitude() {
            return Err(invalid("prediction.mvd_magnitude"));
        }
        let code_number = magnitude + 1;
        let prefix = 31 - code_number.leading_zeros();
        for position in 0..prefix {
            if position < 3 {
                self.context(context_base + u16::try_from(position).unwrap(), false)?;
            } else {
                self.bypass(false)?;
            }
        }
        if prefix < 3 {
            self.context(context_base + u16::try_from(prefix).unwrap(), true)?;
        } else {
            self.bypass(true)?;
        }
        for shift in (0..prefix).rev() {
            self.bypass(((code_number >> shift) & 1) != 0)?;
        }
        if magnitude != 0 {
            self.bypass(value < 0)?;
        }
        Ok(())
    }
}

impl SyntaxReader<'_> {
    /// Reads the complete prediction-kind branch for one block.
    pub fn read_prediction(&mut self, frame_type: FrameType) -> Result<Prediction, BitstreamError> {
        if frame_type == FrameType::Key {
            return Ok(Prediction::Intra(self.read_intra_mode()?));
        }
        self.record_element(SyntaxElement::Skip);
        if self.context(12)? {
            return Ok(Prediction::Skip {
                reference: self.read_reference()?,
            });
        }
        self.record_element(SyntaxElement::IsInter);
        if !self.context(15)? {
            return Ok(Prediction::Intra(self.read_intra_mode()?));
        }
        Ok(Prediction::Inter {
            reference: self.read_reference()?,
            mvd: self.read_mvd()?,
        })
    }

    fn read_intra_mode(&mut self) -> Result<IntraMode, BitstreamError> {
        self.record_element(SyntaxElement::IntraMode);
        let mut index = 0_u8;
        for context in [18, 21, 22] {
            index = (index << 1) | u8::from(self.context(context)?);
        }
        IntraMode::from_index(index)
    }

    fn read_reference(&mut self) -> Result<ReferenceFrame, BitstreamError> {
        self.record_element(SyntaxElement::RefSelect);
        Ok(if self.context(28)? {
            ReferenceFrame::Golden
        } else {
            ReferenceFrame::Last
        })
    }

    fn read_mvd(&mut self) -> Result<MotionVector, BitstreamError> {
        self.record_element(SyntaxElement::Mvd);
        Ok(MotionVector {
            x_q4: self.read_signed_exp_golomb(30)?,
            y_q4: self.read_signed_exp_golomb(33)?,
        })
    }

    fn read_signed_exp_golomb(&mut self, context_base: u16) -> Result<i16, BitstreamError> {
        let mut prefix = 0_u32;
        loop {
            let terminal = if prefix < 3 {
                self.context(context_base + u16::try_from(prefix).unwrap())?
            } else {
                self.bypass()?
            };
            if terminal {
                break;
            }
            prefix += 1;
            if prefix > 10 {
                return Err(invalid("prediction.mvd_prefix"));
            }
        }
        let mut code_number = 1_u32 << prefix;
        for shift in (0..prefix).rev() {
            code_number |= u32::from(self.bypass()?) << shift;
        }
        let magnitude = code_number - 1;
        if magnitude > max_mvd_magnitude() {
            return Err(invalid("prediction.mvd_magnitude"));
        }
        if magnitude == 0 {
            return Ok(0);
        }
        let magnitude = i16::try_from(magnitude).map_err(|_| invalid("prediction.mvd"))?;
        Ok(if self.bypass()? {
            -magnitude
        } else {
            magnitude
        })
    }
}

fn invalid(element: &'static str) -> BitstreamError {
    BitstreamError::InvalidField { offset: 0, element }
}
