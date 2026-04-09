use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct SourceId(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct TextRange {
    start: usize,
    end: usize,
}

impl TextRange {
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    pub const fn start(self) -> usize {
        self.start
    }

    pub const fn end(self) -> usize {
        self.end
    }

    pub const fn len(self) -> usize {
        self.end.saturating_sub(self.start)
    }

    pub const fn is_empty(self) -> bool {
        self.start == self.end
    }
}

impl fmt::Debug for TextRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}..{}", self.start, self.end)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Span {
    pub source_id: SourceId,
    pub range: TextRange,
}

impl Span {
    pub const fn new(source_id: SourceId, start: usize, end: usize) -> Self {
        Self {
            source_id,
            range: TextRange::new(start, end),
        }
    }

    pub const fn start(self) -> usize {
        self.range.start()
    }

    pub const fn end(self) -> usize {
        self.range.end()
    }
}

#[derive(Debug, Clone)]
pub struct LineIndex {
    starts: Vec<usize>,
}

impl LineIndex {
    pub fn new(source: &str) -> Self {
        let mut starts = vec![0];
        for (idx, ch) in source.char_indices() {
            if ch == '\n' {
                starts.push(idx + ch.len_utf8());
            }
        }
        Self { starts }
    }

    pub fn line_col(&self, offset: usize) -> (usize, usize) {
        let line = match self.starts.binary_search(&offset) {
            Ok(index) => index,
            Err(index) => index.saturating_sub(1),
        };
        let column = offset.saturating_sub(self.starts[line]);
        (line + 1, column + 1)
    }
}
