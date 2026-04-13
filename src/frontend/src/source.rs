//! Source file and span primitives shared across the frontend.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SourceFileId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SourcePosition {
    pub offset: u32,
    pub line: u32,
    pub column: u32,
}

impl SourcePosition {
    pub const fn new(offset: u32, line: u32, column: u32) -> Self {
        Self {
            offset,
            line,
            column,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SourceSpan {
    pub file_id: SourceFileId,
    pub start: SourcePosition,
    pub end: SourcePosition,
}

impl SourceSpan {
    pub const fn new(file_id: SourceFileId, start: SourcePosition, end: SourcePosition) -> Self {
        Self {
            file_id,
            start,
            end,
        }
    }

    pub const fn byte_len(&self) -> u32 {
        self.end.offset.saturating_sub(self.start.offset)
    }
}

#[cfg(test)]
mod tests {
    use super::{SourceFileId, SourcePosition, SourceSpan};

    #[test]
    fn span_byte_len_uses_offsets() {
        let span = SourceSpan::new(
            SourceFileId(7),
            SourcePosition::new(10, 2, 4),
            SourcePosition::new(14, 2, 8),
        );

        assert_eq!(span.byte_len(), 4);
    }
}
