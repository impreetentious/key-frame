use kf_range::{Probability, RangeDecoder, RangeEncoder};

fn adaptive(symbols: &[bool], initial: u16, expected: &[u8], final_p1: u16) {
    let mut encoder = RangeEncoder::new();
    let mut encoder_probability = Probability::new(initial).unwrap();
    for &symbol in symbols {
        encoder
            .encode_context(symbol, &mut encoder_probability)
            .unwrap();
    }
    let encoded = encoder.finish();
    assert_eq!(encoded.bytes, expected);
    assert_eq!(encoder_probability.p1(), final_p1);

    let mut decoder = RangeDecoder::new(&encoded.bytes).unwrap();
    let mut decoder_probability = Probability::new(initial).unwrap();
    for &symbol in symbols {
        assert_eq!(
            decoder.decode_context(&mut decoder_probability).unwrap(),
            symbol
        );
    }
    assert_eq!(decoder_probability, encoder_probability);
}

#[test]
fn every_adaptive_oracle_vector_replays() {
    adaptive(&[false; 24], 2048, &[0, 0, 0, 0, 0, 0], 949);
    adaptive(
        &[true; 24],
        2048,
        &[0x00, 0xff, 0xfe, 0x99, 0x81, 0xe2],
        3134,
    );
    let alternating: Vec<_> = [false, true].into_iter().cycle().take(32).collect();
    adaptive(
        &alternating,
        2048,
        &[0x00, 0x57, 0x0f, 0x04, 0x84, 0x2f, 0x1d, 0xbb, 0x00],
        2050,
    );
    let asymmetric: Vec<_> = [true, true, true, false, true, true, false, true]
        .into_iter()
        .cycle()
        .take(32)
        .collect();
    adaptive(
        &asymmetric,
        3072,
        &[0x00, 0x9e, 0x0d, 0xa3, 0xa0, 0xce, 0xdb, 0x42],
        3041,
    );
    let mut carry = vec![true; 9];
    carry.extend([false; 3]);
    carry.extend([true; 21]);
    adaptive(
        &carry,
        4095,
        &[0x00, 0x00, 0x8f, 0xe2, 0x2b, 0x5a, 0x1f],
        3897,
    );
}

#[test]
fn bypass_oracle_vector_replays_without_adaptation() {
    let symbols: Vec<_> = [false, true].into_iter().cycle().take(40).collect();
    let mut encoder = RangeEncoder::new();
    for &symbol in &symbols {
        encoder.encode_bypass(symbol).unwrap();
    }
    let encoded = encoder.finish();
    assert_eq!(encoded.bytes, [0x00, 0x55, 0x55, 0x4d, 0x55, 0x55, 0, 0, 0]);
    let mut decoder = RangeDecoder::new(&encoded.bytes).unwrap();
    for symbol in symbols {
        assert_eq!(decoder.decode_bypass().unwrap(), symbol);
    }
}
