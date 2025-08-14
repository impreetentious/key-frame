use crate::{BitstreamError, BlockSize, PartitionTree, SyntaxElement, SyntaxReader, SyntaxWriter};

impl SyntaxWriter {
    /// Writes one validated top-down superblock partition tree.
    pub fn write_partition(&mut self, tree: &PartitionTree) -> Result<(), BitstreamError> {
        tree.validate()?;
        if tree.size() != BlockSize::N64 {
            return Err(BitstreamError::InvalidField {
                offset: 0,
                element: "partition.root_size",
            });
        }
        self.write_partition_node(tree)
    }

    fn write_partition_node(&mut self, tree: &PartitionTree) -> Result<(), BitstreamError> {
        let size = tree.size();
        if size == BlockSize::N8 {
            return Ok(());
        }
        let split = matches!(tree, PartitionTree::Split { .. });
        self.write_partition_decision(size, split)?;
        if let PartitionTree::Split { children, .. } = tree {
            for child in children.iter() {
                self.write_partition_node(child)?;
            }
        }
        Ok(())
    }

    /// Writes one validated partition decision for encoder RDO snapshots.
    pub fn write_partition_decision(
        &mut self,
        size: BlockSize,
        split: bool,
    ) -> Result<(), BitstreamError> {
        if size == BlockSize::N8 {
            return if split {
                Err(BitstreamError::InvalidField {
                    offset: 0,
                    element: "partition.split_at_minimum",
                })
            } else {
                Ok(())
            };
        }
        self.record_element(SyntaxElement::PartitionTree);
        self.context(u16::from(size.depth()) * 3, split)
    }
}

impl SyntaxReader<'_> {
    /// Reads one top-down superblock partition tree.
    pub fn read_partition(&mut self) -> Result<PartitionTree, BitstreamError> {
        self.read_partition_node(BlockSize::N64)
    }

    fn read_partition_node(&mut self, size: BlockSize) -> Result<PartitionTree, BitstreamError> {
        if size == BlockSize::N8 {
            return Ok(PartitionTree::Leaf(size));
        }
        let context_id = u16::from(size.depth()) * 3;
        self.record_element(SyntaxElement::PartitionTree);
        if !self.context(context_id)? {
            return Ok(PartitionTree::Leaf(size));
        }
        let child_size = size.child()?;
        let children = Box::new([
            self.read_partition_node(child_size)?,
            self.read_partition_node(child_size)?,
            self.read_partition_node(child_size)?,
            self.read_partition_node(child_size)?,
        ]);
        Ok(PartitionTree::Split { size, children })
    }
}
