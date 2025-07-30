use crate::BitstreamError;

/// Version-one coding-block side lengths.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlockSize {
    N8,
    N16,
    N32,
    N64,
}

impl BlockSize {
    #[must_use]
    pub const fn side(self) -> u8 {
        match self {
            Self::N8 => 8,
            Self::N16 => 16,
            Self::N32 => 32,
            Self::N64 => 64,
        }
    }

    #[must_use]
    pub const fn depth(self) -> u8 {
        match self {
            Self::N64 => 0,
            Self::N32 => 1,
            Self::N16 => 2,
            Self::N8 => 3,
        }
    }

    pub const fn child(self) -> Result<Self, BitstreamError> {
        match self {
            Self::N64 => Ok(Self::N32),
            Self::N32 => Ok(Self::N16),
            Self::N16 => Ok(Self::N8),
            Self::N8 => Err(BitstreamError::InvalidField {
                offset: 0,
                element: "partition.split_at_minimum",
            }),
        }
    }
}

/// One complete superblock quadtree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PartitionTree {
    Leaf(BlockSize),
    Split {
        size: BlockSize,
        children: Box<[PartitionTree; 4]>,
    },
}

/// Frame class that selects the prediction-kind syntax tree.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameType {
    Key,
    P,
}

/// Closed version-one intra prediction mode set.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IntraMode {
    Dc,
    Planar,
    Horizontal,
    Vertical,
    D45,
    D135,
    D117,
    D153,
}

impl IntraMode {
    pub(crate) const fn index(self) -> u8 {
        match self {
            Self::Dc => 0,
            Self::Planar => 1,
            Self::Horizontal => 2,
            Self::Vertical => 3,
            Self::D45 => 4,
            Self::D135 => 5,
            Self::D117 => 6,
            Self::D153 => 7,
        }
    }

    pub(crate) const fn from_index(index: u8) -> Result<Self, BitstreamError> {
        match index {
            0 => Ok(Self::Dc),
            1 => Ok(Self::Planar),
            2 => Ok(Self::Horizontal),
            3 => Ok(Self::Vertical),
            4 => Ok(Self::D45),
            5 => Ok(Self::D135),
            6 => Ok(Self::D117),
            7 => Ok(Self::D153),
            _ => Err(BitstreamError::InvalidField {
                offset: 0,
                element: "prediction.intra_mode",
            }),
        }
    }
}

/// Reference slot selected by an inter or skip block.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReferenceFrame {
    Last,
    Golden,
}

/// Quarter-pel motion-vector difference.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MotionVector {
    pub x_q4: i16,
    pub y_q4: i16,
}

/// Complete block prediction branch after partition decoding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Prediction {
    Intra(IntraMode),
    Skip {
        reference: ReferenceFrame,
    },
    Inter {
        reference: ReferenceFrame,
        mvd: MotionVector,
    },
}

/// Context-conditioning plane class for coefficient presence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlaneClass {
    Luma,
    Chroma,
}

/// Closed set of derived transform block sizes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransformBlockSize {
    N4,
    N8,
    N16,
    N32,
}

impl TransformBlockSize {
    #[must_use]
    pub const fn side(self) -> usize {
        match self {
            Self::N4 => 4,
            Self::N8 => 8,
            Self::N16 => 16,
            Self::N32 => 32,
        }
    }

    pub(crate) const fn group(self) -> u16 {
        match self {
            Self::N4 => 0,
            Self::N8 => 1,
            Self::N16 => 2,
            Self::N32 => 3,
        }
    }

    pub(crate) const fn position_bits(self) -> u8 {
        match self {
            Self::N4 => 2,
            Self::N8 => 3,
            Self::N16 => 4,
            Self::N32 => 5,
        }
    }
}

impl PartitionTree {
    /// Validates parent/child sizes and the 8-pixel recursion floor.
    pub fn validate(&self) -> Result<(), BitstreamError> {
        match self {
            Self::Leaf(_) => Ok(()),
            Self::Split { size, children } => {
                let child_size = size.child()?;
                for child in children.iter() {
                    if child.size() != child_size {
                        return Err(BitstreamError::InvalidField {
                            offset: 0,
                            element: "partition.child_size",
                        });
                    }
                    child.validate()?;
                }
                Ok(())
            }
        }
    }

    #[must_use]
    pub const fn size(&self) -> BlockSize {
        match self {
            Self::Leaf(size) | Self::Split { size, .. } => *size,
        }
    }

    /// Collects leaves in normative quadtree order.
    pub fn leaves(&self) -> Vec<BlockSize> {
        let mut leaves = Vec::new();
        self.push_leaves(&mut leaves);
        leaves
    }

    fn push_leaves(&self, leaves: &mut Vec<BlockSize>) {
        match self {
            Self::Leaf(size) => leaves.push(*size),
            Self::Split { children, .. } => {
                for child in children.iter() {
                    child.push_leaves(leaves);
                }
            }
        }
    }
}
