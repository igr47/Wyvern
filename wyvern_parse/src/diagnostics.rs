use std::ops::Range;

#[derive(Debug, Clone, PartialEq)]
pub struct Diagnostic {
    pub span: Range<usize>,
    pub message: String,
}

impl Diagnostic {
    /// Human-readable error with the offending line and a caret underline.
    pub fn render(&self, src: &str) -> String {
        let start = self.span.start.min(src.len());
        let line_start = src[..start].rfind('\n').map_or(0, |i| i + 1);
        let line_end = src[start..].find('\n').map_or(src.len(), |i| start + i);
        let line_no = src[..start].matches('\n').count() + 1;
        let col = src[line_start..start].chars().count();
        let end = self.span.end.min(line_end).max(start);
        let width = src[start..end].chars().count().max(1);
        format!(
            "error: {}\n --> line {}, column {}\n  | {}\n  | {}{}\n",
            self.message,
            line_no,
            col + 1,
            &src[line_start..line_end],
            " ".repeat(col),
            "^".repeat(width),
        )
    }
}
