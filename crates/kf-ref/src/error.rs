use core::fmt;

/// Independent decoder failure with a byte offset and stable element name.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReferenceError {
    pub offset: u32,
    pub element: &'static str,
}

impl ReferenceError {
    pub(crate) const fn new(offset: u32, element: &'static str) -> Self {
        Self { offset, element }
    }
}

impl fmt::Display for ReferenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "invalid {} at byte {}",
            self.element, self.offset
        )
    }
}

impl std::error::Error for ReferenceError {}
