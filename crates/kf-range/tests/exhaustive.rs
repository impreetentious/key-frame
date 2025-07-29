use kf_range::{Probability, RangeDecoder, RangeEncoder};

#[test]
fn every_sixteen_bin_word_round_trips_at_representative_probabilities() {
    for initial in [1, 2048, 4095] {
        for word in 0_u32..=u32::from(u16::MAX) {
            let mut encoder = RangeEncoder::new();
            let mut encoder_probability = Probability::new(initial).unwrap();
            for shift in (0..16).rev() {
                let symbol = ((word >> shift) & 1) != 0;
                encoder
                    .encode_context(symbol, &mut encoder_probability)
                    .unwrap();
            }
            let encoded = encoder.finish();
            let mut decoder = RangeDecoder::new(&encoded.bytes).unwrap();
            let mut decoder_probability = Probability::new(initial).unwrap();
            let mut decoded = 0_u32;
            for _ in 0..16 {
                decoded = (decoded << 1)
                    | u32::from(decoder.decode_context(&mut decoder_probability).unwrap());
            }
            assert_eq!(decoded, word, "initial p1={initial}");
            assert_eq!(decoder_probability, encoder_probability);
        }
    }
}
