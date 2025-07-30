use kf_bitstream::{
    BlockSize, PartitionTree, PlaneClass, SyntaxReader, SyntaxWriter, TransformBlockSize,
};
use kf_range::ContextBank;

fn uniform_tree(size: BlockSize, split_depth: u8) -> PartitionTree {
    if split_depth == 0 || size == BlockSize::N8 {
        PartitionTree::Leaf(size)
    } else {
        let child = size.child().unwrap();
        PartitionTree::Split {
            size,
            children: Box::new([
                uniform_tree(child, split_depth - 1),
                uniform_tree(child, split_depth - 1),
                uniform_tree(child, split_depth - 1),
                uniform_tree(child, split_depth - 1),
            ]),
        }
    }
}

#[test]
fn every_uniform_partition_depth_round_trips() {
    for depth in 0..=3 {
        let tree = uniform_tree(BlockSize::N64, depth);
        let mut writer = SyntaxWriter::new(ContextBank::initial());
        writer.write_partition(&tree).unwrap();
        let (encoded, expected_contexts) = writer.finish();
        let mut reader = SyntaxReader::new(&encoded.bytes, ContextBank::initial()).unwrap();
        assert_eq!(reader.read_partition().unwrap(), tree);
        assert_eq!(reader.contexts(), &expected_contexts);
    }
}

#[test]
fn malformed_nonzero_block_truncation_is_clean_error() {
    let mut levels = vec![0; 16];
    levels[15] = -32_767;
    let mut writer = SyntaxWriter::new(ContextBank::initial());
    writer
        .write_coefficients(PlaneClass::Luma, TransformBlockSize::N4, &levels)
        .unwrap();
    let encoded = writer.finish().0.bytes;
    let mut saw_error = false;
    for length in 5..encoded.len() {
        let mut reader = SyntaxReader::new(&encoded[..length], ContextBank::initial()).unwrap();
        if reader
            .read_coefficients(PlaneClass::Luma, TransformBlockSize::N4)
            .is_err()
        {
            saw_error = true;
            break;
        }
    }
    assert!(saw_error);
}
