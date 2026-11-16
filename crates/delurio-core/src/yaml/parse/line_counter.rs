//! The port of the yaml library's parse/line-counter.ts: the 1-based line and column of an offset, from the line
//! starts the parser counted.

use crate::error::At;

/// `LineCounter`
#[derive(Debug, Clone, Default)]
pub struct LineCounter {
    pub line_starts: Vec<usize>,
}

impl LineCounter {
    /// `linePos(offset)`: a binary search for the line; line 0 when the offset is before the first line start
    pub fn line_pos(&self, offset: usize) -> At {
        let starts = &self.line_starts;
        let mut low = 0;
        let mut high = starts.len();
        while low < high {
            let mid = (low + high) >> 1;
            if starts[mid] < offset {
                low = mid + 1;
            } else {
                high = mid;
            }
        }
        if starts.get(low) == Some(&offset) {
            return At { line: low + 1, column: 1 };
        }
        if low == 0 {
            return At { line: 0, column: offset };
        }
        let start = starts[low - 1];
        At { line: low, column: offset - start + 1 }
    }
}
