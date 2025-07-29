use kf_range::{ContextBank, RangeDecoder, RangeEncoder};

#[test]
fn trap_context_lockstep() {
    let schedule: Vec<_> = (0_u16..576)
        .map(|index| {
            let context = index % 144;
            let symbol = index.wrapping_mul(29).count_ones().is_multiple_of(2);
            (context, symbol)
        })
        .collect();
    let mut encoder = RangeEncoder::new();
    let mut encoder_bank = ContextBank::initial();
    for &(context, symbol) in &schedule {
        encoder
            .encode_context(symbol, encoder_bank.get_mut(context).unwrap())
            .unwrap();
    }
    let encoded = encoder.finish();

    let mut decoder = RangeDecoder::new(&encoded.bytes).unwrap();
    let mut decoder_bank = ContextBank::initial();
    for (context, expected) in schedule {
        assert_eq!(
            decoder
                .decode_context(decoder_bank.get_mut(context).unwrap())
                .unwrap(),
            expected
        );
    }
    assert_eq!(decoder_bank, encoder_bank);
}
