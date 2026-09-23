//! Render a card description (CommonMark) into styled, pre-wrapped rows.
//!
//! The renderer wraps the text itself instead of leaving it to ratatui's
//! `Wrap`, so the number of rows is exact: the card detail sizes and scrolls
//! the Description section by `render(..).len()`.
//!
//! Headings are bold and colored, inline code is highlighted, fenced code
//! blocks get a gutter, lists keep their markers and hanging indent, block
//! quotes get a bar, and links are underlined with the target shown after them.

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

const INLINE_CODE: Style = Style::new()
    .fg(Color::Rgb(240, 180, 90))
    .bg(Color::Rgb(45, 45, 45));
const CODE_BLOCK: Style = Style::new().fg(Color::Rgb(150, 220, 150));
const GUTTER: Style = Style::new().fg(Color::DarkGray);
const LINK: Style = Style::new()
    .fg(Color::LightBlue)
    .add_modifier(Modifier::UNDERLINED);
const LINK_TARGET: Style = Style::new().fg(Color::DarkGray);
const QUOTE: Style = Style::new().fg(Color::Gray).add_modifier(Modifier::ITALIC);
const MARKER: Style = Style::new().fg(Color::LightCyan);

fn heading_style(level: HeadingLevel) -> Style {
    let base = Style::new().add_modifier(Modifier::BOLD);
    match level {
        HeadingLevel::H1 => base
            .fg(Color::LightMagenta)
            .add_modifier(Modifier::UNDERLINED),
        HeadingLevel::H2 => base.fg(Color::LightMagenta),
        _ => base.fg(Color::LightCyan),
    }
}

/// Render `text` as markdown wrapped to `width` columns. Always returns at
/// least one row for non-empty input; empty input returns no rows.
pub fn render(text: &str, width: u16) -> Vec<Line<'static>> {
    let mut r = Renderer::new(width.max(4) as usize);
    let options = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    for event in Parser::new_ext(text, options) {
        r.event(event);
    }
    r.finish()
}

struct ListState {
    next_number: Option<u64>,
}

struct Renderer {
    width: usize,
    out: Vec<Line<'static>>,
    /// Spans of the logical line being built (before wrapping).
    spans: Vec<Span<'static>>,
    styles: Vec<Style>,
    lists: Vec<ListState>,
    /// Marker for the first row of the current list item (`• `, `3. `).
    item_marker: Option<String>,
    quote_depth: usize,
    code_block: bool,
    link_targets: Vec<(String, usize)>,
}

impl Renderer {
    fn new(width: usize) -> Self {
        Renderer {
            width,
            out: Vec::new(),
            spans: Vec::new(),
            styles: vec![Style::new()],
            lists: Vec::new(),
            item_marker: None,
            quote_depth: 0,
            code_block: false,
            link_targets: Vec::new(),
        }
    }

    fn style(&self) -> Style {
        *self.styles.last().unwrap_or(&Style::new())
    }

    fn push_style(&mut self, patch: Style) {
        let next = self.style().patch(patch);
        self.styles.push(next);
    }

    fn pop_style(&mut self) {
        if self.styles.len() > 1 {
            self.styles.pop();
        }
    }

    fn text(&mut self, text: &str, style: Style) {
        if !text.is_empty() {
            self.spans.push(Span::styled(text.to_string(), style));
        }
    }

    fn event(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) if self.code_block => {
                // Code blocks keep their own line breaks; every source line is
                // a row of its own.
                let text = text.strip_suffix('\n').unwrap_or(&text);
                for line in text.split('\n') {
                    self.text(line, CODE_BLOCK);
                    self.flush();
                }
            }
            Event::Text(text) => {
                let style = self.style();
                self.text(&text, style);
            }
            Event::Code(code) => self.text(&code, INLINE_CODE),
            Event::Html(html) | Event::InlineHtml(html) => {
                let style = self.style().fg(Color::DarkGray);
                for (i, line) in html.trim_end_matches('\n').split('\n').enumerate() {
                    if i > 0 {
                        self.flush();
                    }
                    self.text(line, style);
                }
            }
            Event::SoftBreak => self.text(" ", Style::new()),
            Event::HardBreak => self.flush(),
            Event::Rule => {
                self.flush();
                let rule = "─".repeat(self.width.saturating_sub(self.prefix_width()).max(1));
                self.text(&rule, GUTTER);
                self.flush();
                self.blank();
            }
            Event::TaskListMarker(done) => {
                let (mark, style) = if done {
                    ("[x] ", Style::new().fg(Color::LightGreen))
                } else {
                    ("[ ] ", MARKER)
                };
                self.text(mark, style);
            }
            Event::FootnoteReference(name) => {
                let style = self.style();
                self.text(&format!("[^{name}]"), style);
            }
            _ => {}
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Heading { level, .. } => {
                self.flush();
                self.push_style(heading_style(level));
            }
            Tag::BlockQuote(_) => {
                self.flush();
                self.quote_depth += 1;
                self.push_style(QUOTE);
            }
            Tag::CodeBlock(kind) => {
                self.flush();
                if let CodeBlockKind::Fenced(lang) = kind {
                    if !lang.is_empty() {
                        self.text(&lang, GUTTER.add_modifier(Modifier::ITALIC));
                        self.flush();
                    }
                }
                self.code_block = true;
            }
            Tag::List(start) => {
                self.flush();
                self.lists.push(ListState { next_number: start });
            }
            Tag::Item => {
                self.flush();
                let marker = match self.lists.last_mut() {
                    Some(ListState {
                        next_number: Some(n),
                    }) => {
                        let marker = format!("{n}. ");
                        *n += 1;
                        marker
                    }
                    _ => "• ".to_string(),
                };
                self.item_marker = Some(marker);
            }
            Tag::Emphasis => self.push_style(Style::new().add_modifier(Modifier::ITALIC)),
            Tag::Strong => self.push_style(Style::new().add_modifier(Modifier::BOLD)),
            Tag::Strikethrough => self.push_style(Style::new().add_modifier(Modifier::CROSSED_OUT)),
            Tag::Link { dest_url, .. } => {
                self.link_targets
                    .push((dest_url.to_string(), self.spans.len()));
                self.push_style(LINK);
            }
            Tag::Image { dest_url, .. } => {
                self.text("[image: ", GUTTER);
                self.link_targets
                    .push((dest_url.to_string(), self.spans.len()));
            }
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => {
                self.flush();
                // Tight list items have no Paragraph events; a loose item's
                // paragraphs and top-level ones are separated by a blank row.
                self.blank();
            }
            TagEnd::Heading(_) => {
                self.pop_style();
                self.flush();
                self.blank();
            }
            TagEnd::BlockQuote(_) => {
                self.flush();
                self.pop_style();
                self.quote_depth = self.quote_depth.saturating_sub(1);
                self.blank();
            }
            TagEnd::CodeBlock => {
                self.flush();
                self.code_block = false;
                self.blank();
            }
            TagEnd::List(_) => {
                self.flush();
                self.lists.pop();
                if self.lists.is_empty() {
                    self.blank();
                }
            }
            TagEnd::Item => {
                self.flush();
                self.item_marker = None;
            }
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => self.pop_style(),
            TagEnd::Link => {
                self.pop_style();
                if let Some((url, first_span)) = self.link_targets.pop() {
                    let label: String = self.spans[first_span..]
                        .iter()
                        .map(|s| s.content.as_ref())
                        .collect();
                    let bare = url.trim_start_matches("mailto:");
                    if !url.is_empty() && label != url && label != bare {
                        self.text(&format!(" ({url})"), LINK_TARGET);
                    }
                }
            }
            TagEnd::Image => {
                if let Some((url, _)) = self.link_targets.pop() {
                    self.text(&format!("] ({url})"), GUTTER);
                }
            }
            _ => {}
        }
    }

    /// Width of the indent + quote bars in front of every row.
    fn prefix_width(&self) -> usize {
        self.prefix(false).iter().map(Span::width).sum()
    }

    /// The row prefix: quote bars, list indent, then either the item marker
    /// (first row of an item) or blank space of the same width (hanging indent).
    fn prefix(&self, first_row: bool) -> Vec<Span<'static>> {
        let mut prefix = Vec::new();
        for _ in 0..self.quote_depth {
            prefix.push(Span::styled("▎ ", GUTTER));
        }
        if !self.lists.is_empty() {
            prefix.push(Span::raw("  ".repeat(self.lists.len() - 1)));
            let marker = self.item_marker.clone().unwrap_or_else(|| "• ".to_string());
            if first_row {
                prefix.push(Span::styled(marker, MARKER));
            } else {
                prefix.push(Span::raw(" ".repeat(marker.chars().count())));
            }
        }
        if self.code_block {
            prefix.push(Span::styled("│ ", GUTTER));
        }
        prefix
    }

    /// An empty row, never two in a row and never at the top.
    fn blank(&mut self) {
        if self.out.last().is_some_and(|line| line.width() > 0) {
            self.out.push(Line::default());
        }
    }

    /// Wrap the pending logical line into rows and append them.
    fn flush(&mut self) {
        if self.spans.is_empty() {
            return;
        }
        let spans = std::mem::take(&mut self.spans);
        let first = self.prefix(true);
        let rest = self.prefix(false);
        // Only the first row of an item shows its marker.
        self.item_marker = self
            .item_marker
            .take()
            .map(|m| " ".repeat(m.chars().count()));
        let first_w: usize = first.iter().map(Span::width).sum();
        let rest_w: usize = rest.iter().map(Span::width).sum();
        let rows = wrap(
            &spans,
            self.width.saturating_sub(first_w).max(1),
            self.width.saturating_sub(rest_w).max(1),
        );
        for (i, row) in rows.into_iter().enumerate() {
            let mut line = if i == 0 { first.clone() } else { rest.clone() };
            line.extend(row);
            self.out.push(Line::from(line));
        }
    }

    fn finish(mut self) -> Vec<Line<'static>> {
        self.flush();
        while self.out.last().is_some_and(|line| line.width() == 0) {
            self.out.pop();
        }
        self.out
    }
}

/// Greedy word wrap over styled spans: `first` columns on the first row,
/// `rest` on the following ones. Words longer than a row are hard-broken.
fn wrap(spans: &[Span<'static>], first: usize, rest: usize) -> Vec<Vec<Span<'static>>> {
    // Split every span into words and the spaces between them, keeping style.
    let mut pieces: Vec<(String, Style, bool)> = Vec::new();
    for span in spans {
        let mut word = String::new();
        for ch in span.content.chars() {
            if ch == ' ' {
                if !word.is_empty() {
                    pieces.push((std::mem::take(&mut word), span.style, false));
                }
                pieces.push((" ".to_string(), span.style, true));
            } else {
                word.push(ch);
            }
        }
        if !word.is_empty() {
            pieces.push((word, span.style, false));
        }
    }

    let mut rows: Vec<Vec<Span<'static>>> = vec![Vec::new()];
    let mut used = 0usize;
    let limit = |rows: &Vec<Vec<Span<'static>>>| if rows.len() == 1 { first } else { rest };
    for (text, style, is_space) in pieces {
        let w = Span::raw(text.as_str()).width();
        if is_space {
            // Drop spaces at the start of a wrapped row.
            if used == 0 && rows.len() > 1 {
                continue;
            }
            if used + w <= limit(&rows) {
                rows.last_mut().unwrap().push(Span::styled(text, style));
                used += w;
            }
            continue;
        }
        if used > 0 && used + w > limit(&rows) {
            trim_trailing_space(rows.last_mut().unwrap());
            rows.push(Vec::new());
            used = 0;
        }
        if w <= limit(&rows) {
            rows.last_mut().unwrap().push(Span::styled(text, style));
            used += w;
            continue;
        }
        // Hard-break a word longer than a whole row.
        let mut chunk = String::new();
        for ch in text.chars() {
            let cw = Span::raw(ch.to_string()).width();
            if used + cw > limit(&rows) && used > 0 {
                if !chunk.is_empty() {
                    rows.last_mut()
                        .unwrap()
                        .push(Span::styled(std::mem::take(&mut chunk), style));
                }
                rows.push(Vec::new());
                used = 0;
            }
            chunk.push(ch);
            used += cw;
        }
        if !chunk.is_empty() {
            rows.last_mut().unwrap().push(Span::styled(chunk, style));
        }
    }
    for row in &mut rows {
        trim_trailing_space(row);
    }
    rows
}

fn trim_trailing_space(row: &mut Vec<Span<'static>>) {
    while row.last().is_some_and(|span| span.content == " ") {
        row.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(lines: &[Line<'_>]) -> Vec<String> {
        lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }

    fn span_with<'a>(lines: &'a [Line<'a>], text: &str) -> &'a Span<'a> {
        lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .find(|s| s.content == text)
            .unwrap_or_else(|| panic!("no span {text:?} in {:?}", plain(lines)))
    }

    #[test]
    fn headings_lose_their_hashes_and_are_bold() {
        let lines = render("# Title\n\nBody", 40);
        assert_eq!(plain(&lines), ["Title", "", "Body"]);
        assert!(span_with(&lines, "Title")
            .style
            .add_modifier
            .contains(Modifier::BOLD));
    }

    #[test]
    fn inline_code_is_highlighted_without_backticks() {
        let lines = render("Run `npm test` now", 40);
        assert_eq!(plain(&lines), ["Run npm test now"]);
        assert_eq!(span_with(&lines, "npm").style, INLINE_CODE);
    }

    #[test]
    fn code_blocks_keep_lines_and_get_a_gutter() {
        let lines = render("```sh\nnpm ci\nnpm test\n```", 40);
        assert_eq!(plain(&lines), ["sh", "│ npm ci", "│ npm test"]);
        assert_eq!(span_with(&lines, "npm").style, CODE_BLOCK);
    }

    #[test]
    fn lists_show_markers_and_hang_wrapped_rows() {
        let lines = render("- one two three four\n- b\n\n1. first\n2. second", 12);
        assert_eq!(
            plain(&lines),
            [
                "• one two",
                "  three four",
                "• b",
                "",
                "1. first",
                "2. second"
            ]
        );
    }

    #[test]
    fn nested_lists_indent() {
        let lines = render("- a\n  - b", 20);
        assert_eq!(plain(&lines), ["• a", "  • b"]);
    }

    #[test]
    fn task_lists_show_boxes() {
        let lines = render("- [x] done\n- [ ] todo", 20);
        assert_eq!(plain(&lines), ["• [x] done", "• [ ] todo"]);
    }

    #[test]
    fn links_show_their_target() {
        let lines = render("See [docs](https://x.dev) and <https://y.dev>", 60);
        assert_eq!(
            plain(&lines),
            ["See docs (https://x.dev) and https://y.dev"]
        );
        assert!(span_with(&lines, "docs")
            .style
            .add_modifier
            .contains(Modifier::UNDERLINED));
    }

    #[test]
    fn quotes_get_a_bar() {
        let lines = render("> quoted text", 20);
        assert_eq!(plain(&lines), ["▎ quoted text"]);
    }

    #[test]
    fn long_words_are_hard_broken_and_rows_never_exceed_the_width() {
        let text = "short averyveryverylongwordthatdoesnotfit end";
        for line in render(text, 10) {
            assert!(line.width() <= 10, "{line:?}");
        }
    }

    #[test]
    fn plain_text_paragraphs_stay_as_written() {
        let lines = render("first line\nsame paragraph\n\nsecond", 40);
        assert_eq!(plain(&lines), ["first line same paragraph", "", "second"]);
    }

    #[test]
    fn code_keeps_its_indentation() {
        let lines = render("```\nfn a() {\n    b();\n}\n```", 40);
        assert_eq!(plain(&lines), ["│ fn a() {", "│     b();", "│ }"]);
    }

    #[test]
    fn empty_description_has_no_rows() {
        assert!(render("", 40).is_empty());
    }
}
