use kf_ref::ReferenceDecoder;

const DAMAGED: &[u8] = include_bytes!("../../../conformance/crashes/false_sync_prefix.kfv");
const ORACLE: &[u8] = include_bytes!("../../../conformance/oracle/intra64_dc_all_zero.kfv");

#[test]
fn trap_sync_false_positive_decodes_the_real_packet() {
    let recovered = ReferenceDecoder::new().decode_stream(DAMAGED).unwrap();
    let clean = ReferenceDecoder::new().decode_stream(ORACLE).unwrap();
    assert_eq!(recovered, clean);
}
