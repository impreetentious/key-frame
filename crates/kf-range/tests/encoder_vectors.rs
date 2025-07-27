use kf_range::{Probability, RangeEncoder};

fn encode(symbols: &[bool], p1: u16) -> (Vec<u8>, u16) {
    let mut encoder = RangeEncoder::new();
    let mut probability = Probability::new(p1).unwrap();
    for &symbol in symbols {
        encoder.encode_context(symbol, &mut probability).unwrap();
    }
    (encoder.finish().bytes, probability.p1())
}

#[test]
fn alternating_oracle_vector_matches() {
    let symbols: Vec<_> = [false, true].into_iter().cycle().take(32).collect();
    let (bytes, final_p1) = encode(&symbols, 2048);
    assert_eq!(
        bytes,
        [0x00, 0x57, 0x0f, 0x04, 0x84, 0x2f, 0x1d, 0xbb, 0x00]
    );
    assert_eq!(final_p1, 2050);
}

#[test]
fn trap_range_coder_carry() {
    let mut symbols = vec![true; 9];
    symbols.extend([false; 3]);
    symbols.extend([true; 21]);
    let (bytes, final_p1) = encode(&symbols, 4095);
    assert_eq!(bytes, [0x00, 0x00, 0x8f, 0xe2, 0x2b, 0x5a, 0x1f]);
    assert_eq!(final_p1, 3897);
}
