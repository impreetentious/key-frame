use kf_range::{Probability, RangeDecoder, RangeEncoder};

#[test]
fn adaptive_round_trip_preserves_symbols_and_context_state() {
    let symbols: Vec<_> = (0_u16..512)
        .map(|index| (index.wrapping_mul(73).wrapping_add(19) & 0x40) != 0)
        .collect();
    let mut encoder = RangeEncoder::new();
    let mut encoder_probability = Probability::new(1733).unwrap();
    for &symbol in &symbols {
        encoder
            .encode_context(symbol, &mut encoder_probability)
            .unwrap();
    }
    let encoded = encoder.finish();

    let mut decoder = RangeDecoder::new(&encoded.bytes).unwrap();
    let mut decoder_probability = Probability::new(1733).unwrap();
    for expected in symbols {
        assert_eq!(
            decoder.decode_context(&mut decoder_probability).unwrap(),
            expected
        );
    }
    assert_eq!(decoder_probability, encoder_probability);
}

#[test]
fn bypass_round_trip_does_not_touch_a_context() {
    let symbols: Vec<_> = [false, true, true, false, true]
        .into_iter()
        .cycle()
        .take(100)
        .collect();
    let mut encoder = RangeEncoder::new();
    for &symbol in &symbols {
        encoder.encode_bypass(symbol).unwrap();
    }
    let encoded = encoder.finish();
    let mut decoder = RangeDecoder::new(&encoded.bytes).unwrap();
    for expected in symbols {
        assert_eq!(decoder.decode_bypass().unwrap(), expected);
    }
}
