use kf_bitstream::{
    BlockSize, FrameType, IntraMode, PartitionTree, Prediction, SyntaxReader, SyntaxWriter,
};
use kf_range::ContextBank;

#[test]
fn partition_and_intra_syntax_record_matching_context_ids() {
    let tree = PartitionTree::Split {
        size: BlockSize::N64,
        children: Box::new([
            PartitionTree::Leaf(BlockSize::N32),
            PartitionTree::Leaf(BlockSize::N32),
            PartitionTree::Leaf(BlockSize::N32),
            PartitionTree::Leaf(BlockSize::N32),
        ]),
    };
    let mut writer = SyntaxWriter::new(ContextBank::initial());
    writer.write_partition(&tree).unwrap();
    writer
        .write_prediction(FrameType::Key, Prediction::Intra(IntraMode::Dc))
        .unwrap();
    let writer_coverage = writer.coverage().clone();
    let (encoded, encoder_contexts) = writer.finish();

    let mut reader = SyntaxReader::new(&encoded.bytes, ContextBank::initial()).unwrap();
    assert_eq!(reader.read_partition().unwrap(), tree);
    assert_eq!(
        reader.read_prediction(FrameType::Key).unwrap(),
        Prediction::Intra(IntraMode::Dc)
    );
    assert_eq!(reader.contexts(), &encoder_contexts);
    assert_eq!(reader.coverage(), &writer_coverage);
    assert!(writer_coverage.contains(0));
    assert!(writer_coverage.contains(18));
    assert!(writer_coverage.contains(21));
    assert!(writer_coverage.contains(22));
    assert!(!writer_coverage.contains(12));
}

#[test]
fn bypass_bins_do_not_acquire_context_ids() {
    let mut writer = SyntaxWriter::new(ContextBank::initial());
    writer
        .write_prediction(
            FrameType::P,
            kf_bitstream::Prediction::Inter {
                reference: kf_bitstream::ReferenceFrame::Last,
                mvd: kf_bitstream::MotionVector { x_q4: 16, y_q4: -4 },
            },
        )
        .unwrap();
    let coverage = writer.coverage().clone();
    assert!(coverage.contains(12));
    assert!(coverage.contains(15));
    assert!(coverage.contains(28));
    assert!(coverage.contains(30));
    assert!(coverage.contains(31));
    assert!(coverage.contains(32));
    assert!(coverage.contains(33));
    assert!(coverage.contains(34));
    assert!(coverage.contains(35));
    assert!(!coverage.contains(29));
}
