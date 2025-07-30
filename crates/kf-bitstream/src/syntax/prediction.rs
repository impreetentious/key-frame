use crate::{
    BitstreamError, FrameType, IntraMode, MotionVector, Prediction, ReferenceFrame, SyntaxReader,
    SyntaxWriter,
};

const MAX_MVD_MAGNITUDE: u32 = 512;

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
                self.context(12, true)?;
                self.write_reference(reference)
            }
            (FrameType::P, Prediction::Intra(mode)) => {
                self.context(12, false)?;
                self.context(15, false)?;
                self.write_intra_mode(mode)
            }
            (FrameType::P, Prediction::Inter { reference, mvd }) => {
                self.context(12, false)?;
                self.context(15, true)?;
                self.write_reference(reference)?;
                self.write_mvd(mvd)
            }
        }
    }

    fn write_intra_mode(&mut self, mode: IntraMode) -> Result<(), BitstreamError> {
        let index = mode.index();
        for (shift, context) in [(2, 18), (1, 21), (0, 22)] {
            self.context(context, ((index >> shift) & 1) != 0)?;
        }
        Ok(())
    }

    fn write_reference(&mut self, reference: ReferenceFrame) -> Result<(), BitstreamError> {
        self.context(28, reference == ReferenceFrame::Golden)
    }

    fn write_mvd(&mut self, mvd: MotionVector) -> Result<(), BitstreamError> {
        self.write_signed_exp_golomb(mvd.x_q4, 30)?;
        self.write_signed_exp_golomb(mvd.y_q4, 33)
    }

    fn write_signed_exp_golomb(
        &mut self,
        value: i16,
        context_base: u16,
    ) -> Result<(), BitstreamError> {
        let magnitude = u32::from(value.unsigned_abs());
        if magnitude > MAX_MVD_MAGNITUDE {
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
        if self.context(12)? {
            return Ok(Prediction::Skip {
                reference: self.read_reference()?,
            });
        }
        if !self.context(15)? {
            return Ok(Prediction::Intra(self.read_intra_mode()?));
        }
        Ok(Prediction::Inter {
            reference: self.read_reference()?,
            mvd: self.read_mvd()?,
        })
    }

    fn read_intra_mode(&mut self) -> Result<IntraMode, BitstreamError> {
        let mut index = 0_u8;
        for context in [18, 21, 22] {
            index = (index << 1) | u8::from(self.context(context)?);
        }
        IntraMode::from_index(index)
    }

    fn read_reference(&mut self) -> Result<ReferenceFrame, BitstreamError> {
        Ok(if self.context(28)? {
            ReferenceFrame::Golden
        } else {
            ReferenceFrame::Last
        })
    }

    fn read_mvd(&mut self) -> Result<MotionVector, BitstreamError> {
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
        if magnitude > MAX_MVD_MAGNITUDE {
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
