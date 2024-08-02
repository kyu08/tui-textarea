use crate::ratatui::buffer::Buffer;
use crate::ratatui::layout::Rect;
use crate::ratatui::text::{Span, Text};
use crate::ratatui::widgets::{Paragraph, Widget};
use crate::textarea::TextArea;
use crate::util::num_digits;
#[cfg(feature = "ratatui")]
use ratatui::text::Line;
use std::cmp;
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(feature = "tuirs")]
use tui::text::Spans as Line;

// &mut 'a (u16, u16, u16, u16) is not available since `render` method takes immutable reference of TextArea
// instance. In the case, the TextArea instance cannot be accessed from any other objects since it is mutablly
// borrowed.
//
// `ratatui::terminal::Frame::render_stateful_widget` would be an assumed way to render a stateful widget. But at this
// point we stick with using `ratatui::terminal::Frame::render_widget` because it is simpler API. Users don't need to
// manage states of textarea instances separately.
// https://docs.rs/ratatui/latest/ratatui/terminal/struct.Frame.html#method.render_stateful_widget
#[derive(Default, Debug)]
pub struct Viewport(AtomicU64);

impl Clone for Viewport {
    fn clone(&self) -> Self {
        let u = self.0.load(Ordering::Relaxed);
        Viewport(AtomicU64::new(u))
    }
}

impl Viewport {
    pub fn scroll_top(&self) -> (u16, u16) {
        let u = self.0.load(Ordering::Relaxed);
        ((u >> 16) as u16, u as u16)
    }

    pub fn rect(&self) -> (u16, u16, u16, u16) {
        let u = self.0.load(Ordering::Relaxed);
        let width = (u >> 48) as u16;
        let height = (u >> 32) as u16;
        let row = (u >> 16) as u16;
        let col = u as u16;
        (row, col, width, height)
    }

    pub fn position(&self) -> (u16, u16, u16, u16) {
        let (row_top, col_top, width, height) = self.rect();
        let row_bottom = row_top.saturating_add(height).saturating_sub(1);
        let col_bottom = col_top.saturating_add(width).saturating_sub(1);

        (
            row_top,
            col_top,
            cmp::max(row_top, row_bottom),
            cmp::max(col_top, col_bottom),
        )
    }

    fn store(&self, row: u16, col: u16, width: u16, height: u16) {
        // Pack four u16 values into one u64 value
        let u =
            ((width as u64) << 48) | ((height as u64) << 32) | ((row as u64) << 16) | col as u64;
        self.0.store(u, Ordering::Relaxed);
    }

    pub fn scroll(&mut self, rows: i16, cols: i16) {
        fn apply_scroll(pos: u16, delta: i16) -> u16 {
            if delta >= 0 {
                pos.saturating_add(delta as u16)
            } else {
                pos.saturating_sub(-delta as u16)
            }
        }

        let u = self.0.get_mut();
        let row = apply_scroll((*u >> 16) as u16, rows);
        let col = apply_scroll(*u as u16, cols);
        *u = (*u & 0xffff_ffff_0000_0000) | ((row as u64) << 16) | (col as u64);
    }
}

impl<'a> TextArea<'a> {
    fn lnum_width(&self) -> u8 {
        if self.line_number_style().is_none() {
            return 0;
        }
        num_digits(self.lines().len()) + 2
    }

    #[inline]
    fn text_widget(&'a self, top_row: usize, height: usize) -> Text<'a> {
        let lines_len = self.lines().len();
        let lnum_len = num_digits(lines_len);
        let bottom_row = cmp::min(top_row + height, lines_len);
        let mut lines = Vec::with_capacity(bottom_row - top_row);
        for (i, line) in self.lines()[top_row..bottom_row].iter().enumerate() {
            lines.push(self.line_spans(line.as_str(), top_row + i, lnum_len));
        }
        Text::from(lines)
    }

    #[inline]
    fn placeholder_widget(&'a self) -> Text<'a> {
        let cursor = Span::styled(" ", self.cursor_style);
        let text = Span::raw(self.placeholder.as_str());
        Text::from(Line::from(vec![cursor, text]))
    }
}

impl Widget for &TextArea<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let Rect { width, height, .. } = if let Some(b) = self.block() {
            b.inner(area)
        } else {
            area
        };

        fn next_scroll_top(prev_top: u16, cursor: u16, length: u16) -> u16 {
            if cursor < prev_top {
                cursor
            } else if prev_top + length <= cursor {
                cursor + 1 - length
            } else {
                prev_top
            }
        }

        let cursor = self.cursor();
        let (top_row, top_col) = self.viewport.scroll_top();
        let top_row = next_scroll_top(top_row, cursor.0 as u16, height);
        let scroll_width = width.saturating_sub(self.lnum_width() as _);
        let top_col = next_scroll_top(top_col, cursor.1 as u16, scroll_width);

        let (text, style) = if !self.placeholder.is_empty() && self.is_empty() {
            (self.placeholder_widget(), self.placeholder_style)
        } else {
            (self.text_widget(top_row as _, height as _), self.style())
        };

        // To get fine control over the text color and the surrrounding block they have to be rendered separately
        // see https://github.com/ratatui-org/ratatui/issues/144
        let mut text_area = area;
        let mut inner = Paragraph::new(text)
            .style(style)
            .alignment(self.alignment());
        if let Some(b) = self.block() {
            text_area = b.inner(area);
            b.clone().render(area, buf)
        }
        if top_col != 0 {
            inner = inner.scroll((0, top_col));
        }

        // Store scroll top position for rendering on the next tick
        self.viewport.store(top_row, top_col, width, height);

        inner.render(text_area, buf);
    }
}
