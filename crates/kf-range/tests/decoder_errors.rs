use kf_range::{Probability, RangeDecoder, RangeEncoder, RangeError};

#[test]
fn trap_range_eof() {
    let symbols: Vec<_> = (0_u16..1000)
        .map(|value| value.wrapping_mul(109).count_ones().is_multiple_of(2))
        .collect();
    let mut encoder = RangeEncoder::new();
    let mut probability = Probability::new(2048).unwrap();
    for &symbol in &symbols {
        encoder.encode_context(symbol, &mut probability).unwrap();
    }
    let bytes = encoder.finish().bytes;
    let truncated = &bytes[..5];
    let mut decoder = RangeDecoder::new(truncated).unwrap();
    let mut probability = Probability::new(2048).unwrap();
    let error = symbols
        .iter()
        .find_map(|_| decoder.decode_context(&mut probability).err())
        .expect("invariant: initializer-only payload must end during syntax");
    assert_eq!(error, RangeError::EndOfInput { byte_offset: 5 });
}
