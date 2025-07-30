use kf_bitstream::{BlockSize, PartitionTree, SyntaxReader, SyntaxWriter};
use kf_range::ContextBank;

fn mixed_tree() -> PartitionTree {
    PartitionTree::Split {
        size: BlockSize::N64,
        children: Box::new([
            PartitionTree::Leaf(BlockSize::N32),
            PartitionTree::Split {
                size: BlockSize::N32,
                children: Box::new([
                    PartitionTree::Leaf(BlockSize::N16),
                    PartitionTree::Leaf(BlockSize::N16),
                    PartitionTree::Leaf(BlockSize::N16),
                    PartitionTree::Leaf(BlockSize::N16),
                ]),
            },
            PartitionTree::Leaf(BlockSize::N32),
            PartitionTree::Leaf(BlockSize::N32),
        ]),
    }
}

#[test]
fn partition_tree_round_trips_and_contexts_match() {
    let tree = mixed_tree();
    let mut writer = SyntaxWriter::new(ContextBank::initial());
    writer.write_partition(&tree).unwrap();
    let (encoded, encoder_contexts) = writer.finish();
    let mut reader = SyntaxReader::new(&encoded.bytes, ContextBank::initial()).unwrap();
    assert_eq!(reader.read_partition().unwrap(), tree);
    assert_eq!(reader.contexts(), &encoder_contexts);
    assert_eq!(tree.leaves().len(), 7);
}

#[test]
fn malformed_child_size_is_rejected() {
    let invalid = PartitionTree::Split {
        size: BlockSize::N16,
        children: Box::new([
            PartitionTree::Leaf(BlockSize::N16),
            PartitionTree::Leaf(BlockSize::N8),
            PartitionTree::Leaf(BlockSize::N8),
            PartitionTree::Leaf(BlockSize::N8),
        ]),
    };
    assert!(invalid.validate().is_err());
}
