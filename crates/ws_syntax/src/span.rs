use std::ops::Range;

/// Byte range into a source file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub fn new(start: u32, end: u32) -> Self {
        Span { start, end }
    }

    pub fn to(self, other: Span) -> Span {
        Span::new(self.start.min(other.start), self.end.max(other.end))
    }

    pub fn range(self) -> Range<usize> {
        self.start as usize..self.end as usize
    }
}

/// 1-based line and column; the column counts chars, not bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineCol {
    pub line: u32,
    pub column: u32,
}

pub struct LineIndex<'s> {
    src: &'s str,
    line_starts: Vec<usize>,
}

impl<'s> LineIndex<'s> {
    pub fn new(src: &'s str) -> Self {
        let line_starts = std::iter::once(0)
            .chain(src.match_indices('\n').map(|(i, _)| i + 1))
            .collect();
        LineIndex { src, line_starts }
    }

    pub fn line_col(&self, offset: u32) -> LineCol {
        let offset = (offset as usize).min(self.src.len());
        let line = self.line_starts.partition_point(|&s| s <= offset) - 1;
        let start = self.line_starts[line];
        let column = self
            .src
            .get(start..offset)
            .map_or(offset - start, |s| s.chars().count());
        LineCol {
            line: line as u32 + 1,
            column: column as u32 + 1,
        }
    }
}
