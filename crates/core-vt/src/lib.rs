//! core-vt: turns the bytes a shell prints into a screen of cells.
//!
//! No UI and no OS code, so the window, a web viewer, a phone app or an AI screen reader can all
//! share it. Feed bytes with [`Terminal::feed`], read the screen with [`Terminal::line`], and send
//! [`Terminal::take_responses`] back to the shell (answers to "where is the cursor?" and similar).

use std::collections::VecDeque;
use unicode_width::UnicodeWidthChar;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Color {
    #[default]
    Default,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Attrs {
    pub fg: Color,
    pub bg: Color,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
}

/// One screen cell. A wide character (CJK, most emoji) takes two cells: the character itself,
/// then a `spacer` cell that renderers skip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub ch: char,
    pub attrs: Attrs,
    pub wide: bool,
    pub spacer: bool,
}

impl Default for Cell {
    fn default() -> Self {
        Cell { ch: ' ', attrs: Attrs::default(), wide: false, spacer: false }
    }
}

/// Shell-integration marks (OSC 133). The future "blocks" layer turns these into
/// command + output + exit-code records.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Mark {
    PromptStart,
    CommandStart,
    OutputStart,
    CommandEnd(Option<i32>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Ground,
    Escape,
    EscIntermediate,
    Csi,
    Osc,
    OscEscape,
    /// DCS / SOS / PM / APC: swallowed until the string terminator.
    IgnoreString,
    IgnoreStringEscape,
}

#[derive(Clone, Copy, Default)]
struct SavedCursor {
    row: usize,
    col: usize,
    pen: Attrs,
}

pub struct Terminal {
    cols: usize,
    rows: usize,
    grid: Vec<Vec<Cell>>,
    alt_saved: Option<(Vec<Vec<Cell>>, SavedCursor)>,
    scrollback: VecDeque<Vec<Cell>>,
    pub max_scrollback: usize,
    row: usize,
    col: usize,
    pending_wrap: bool,
    pen: Attrs,
    saved: SavedCursor,
    scroll_top: usize,
    scroll_bot: usize,
    tabs: Vec<bool>,

    pub cursor_visible: bool,
    pub autowrap: bool,
    pub app_cursor_keys: bool,
    pub bracketed_paste: bool,
    pub title: String,
    pub marks: Vec<(usize, Mark)>,
    /// Bumped on every change so viewers know when to redraw.
    pub generation: u64,

    state: State,
    params: Vec<u32>,
    cur_param: Option<u32>,
    private: Option<u8>,
    intermediates: Vec<u8>,
    osc: Vec<u8>,
    utf8: [u8; 4],
    utf8_len: usize,
    utf8_need: usize,
    responses: Vec<u8>,
}

impl Terminal {
    pub fn new(cols: usize, rows: usize) -> Self {
        let cols = cols.max(1);
        let rows = rows.max(1);
        Terminal {
            cols,
            rows,
            grid: vec![vec![Cell::default(); cols]; rows],
            alt_saved: None,
            scrollback: VecDeque::new(),
            max_scrollback: 10_000,
            row: 0,
            col: 0,
            pending_wrap: false,
            pen: Attrs::default(),
            saved: SavedCursor::default(),
            scroll_top: 0,
            scroll_bot: rows - 1,
            tabs: default_tabs(cols),
            cursor_visible: true,
            autowrap: true,
            app_cursor_keys: false,
            bracketed_paste: false,
            title: String::new(),
            marks: Vec::new(),
            generation: 0,
            state: State::Ground,
            params: Vec::new(),
            cur_param: None,
            private: None,
            intermediates: Vec::new(),
            osc: Vec::new(),
            utf8: [0; 4],
            utf8_len: 0,
            utf8_need: 0,
            responses: Vec::new(),
        }
    }

    pub fn cols(&self) -> usize {
        self.cols
    }
    pub fn rows(&self) -> usize {
        self.rows
    }
    pub fn cursor(&self) -> (usize, usize) {
        (self.row, self.col.min(self.cols - 1))
    }
    pub fn scrollback_len(&self) -> usize {
        self.scrollback.len()
    }
    pub fn in_alt_screen(&self) -> bool {
        self.alt_saved.is_some()
    }

    /// A visible row, `offset` lines scrolled back into history (0 = live screen).
    pub fn line(&self, row: usize, offset: usize) -> &[Cell] {
        let offset = offset.min(self.scrollback.len());
        if row < offset {
            &self.scrollback[self.scrollback.len() - offset + row]
        } else {
            &self.grid[row - offset]
        }
    }

    /// All lines, history first: indexes run from 0 to `total_lines() - 1`.
    pub fn total_lines(&self) -> usize {
        self.scrollback.len() + self.rows
    }

    pub fn abs_line(&self, i: usize) -> &[Cell] {
        if i < self.scrollback.len() {
            &self.scrollback[i]
        } else {
            &self.grid[(i - self.scrollback.len()).min(self.rows - 1)]
        }
    }

    /// Text between two (line, column) points, inclusive, in reading order. Lines are joined
    /// with newlines and their trailing spaces dropped.
    pub fn text_between(&self, a: (usize, usize), b: (usize, usize)) -> String {
        let (start, end) = if a <= b { (a, b) } else { (b, a) };
        let last = self.total_lines().saturating_sub(1);
        let mut out = String::new();
        for i in start.0..=end.0.min(last) {
            let line = self.abs_line(i);
            let from = if i == start.0 { start.1 } else { 0 };
            let to = if i == end.0 { (end.1 + 1).min(line.len()) } else { line.len() };
            let piece: String = line[from.min(to)..to].iter().filter(|c| !c.spacer).map(|c| c.ch).collect();
            out.push_str(piece.trim_end());
            if i != end.0 {
                out.push('\n');
            }
        }
        out
    }

    /// Plain text of a live screen row, trailing spaces trimmed.
    pub fn row_text(&self, row: usize) -> String {
        let s: String = self.grid[row].iter().filter(|c| !c.spacer).map(|c| c.ch).collect();
        s.trim_end().to_string()
    }

    /// Bytes the shell asked for (cursor position reports, device attributes).
    pub fn take_responses(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.responses)
    }

    pub fn resize(&mut self, cols: usize, rows: usize) {
        let cols = cols.max(1);
        let rows = rows.max(1);
        if cols == self.cols && rows == self.rows {
            return;
        }
        for line in self.grid.iter_mut() {
            line.resize(cols, Cell::default());
        }
        // Shrinking: push lines above the cursor into history first, so the prompt stays visible.
        while self.grid.len() > rows {
            if self.row > 0 {
                let line = self.grid.remove(0);
                self.push_scrollback(line);
                self.row -= 1;
            } else {
                self.grid.pop();
            }
        }
        while self.grid.len() < rows {
            self.grid.push(vec![Cell::default(); cols]);
        }
        self.cols = cols;
        self.rows = rows;
        self.row = self.row.min(rows - 1);
        self.col = self.col.min(cols - 1);
        self.pending_wrap = false;
        self.scroll_top = 0;
        self.scroll_bot = rows - 1;
        self.tabs = default_tabs(cols);
        self.generation += 1;
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.byte(b);
        }
        self.generation += 1;
    }

    fn byte(&mut self, b: u8) {
        // UTF-8 decoding happens only in text-carrying states.
        if self.utf8_need > 0 {
            if b & 0xC0 == 0x80 {
                self.utf8[self.utf8_len] = b;
                self.utf8_len += 1;
                if self.utf8_len == self.utf8_need {
                    let ch = std::str::from_utf8(&self.utf8[..self.utf8_len])
                        .ok()
                        .and_then(|s| s.chars().next())
                        .unwrap_or('\u{FFFD}');
                    self.utf8_need = 0;
                    self.utf8_len = 0;
                    self.char_in(ch);
                }
                return;
            }
            self.utf8_need = 0;
            self.utf8_len = 0;
            self.char_in('\u{FFFD}');
        }
        if b >= 0x80 {
            let need = match b {
                0xC2..=0xDF => 2,
                0xE0..=0xEF => 3,
                0xF0..=0xF4 => 4,
                _ => 0,
            };
            if need == 0 {
                self.char_in('\u{FFFD}');
            } else {
                self.utf8[0] = b;
                self.utf8_len = 1;
                self.utf8_need = need;
            }
            return;
        }
        self.char_in(b as char);
    }

    fn char_in(&mut self, ch: char) {
        let c = ch as u32;
        // These interrupt any sequence.
        if c == 0x18 || c == 0x1A {
            self.state = State::Ground;
            return;
        }
        match self.state {
            State::Ground => {
                if c < 0x20 || c == 0x7F {
                    if c == 0x1B {
                        self.enter_escape();
                    } else {
                        self.control(c as u8);
                    }
                } else {
                    self.print(ch);
                }
            }
            State::Escape => self.escape(ch),
            State::EscIntermediate => {
                if (0x20..=0x2F).contains(&c) {
                    self.intermediates.push(c as u8);
                } else if c == 0x1B {
                    self.enter_escape();
                } else if c >= 0x30 {
                    self.esc_dispatch(ch);
                    self.state = State::Ground;
                } else {
                    self.control(c as u8);
                }
            }
            State::Csi => self.csi_byte(ch),
            State::Osc => match c {
                0x07 => self.osc_end(),
                0x1B => self.state = State::OscEscape,
                _ => {
                    if self.osc.len() < 4096 {
                        let mut buf = [0u8; 4];
                        self.osc.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                    }
                }
            },
            State::OscEscape => {
                self.osc_end();
                if ch != '\\' {
                    self.enter_escape();
                    self.escape(ch);
                }
            }
            State::IgnoreString => {
                if c == 0x1B {
                    self.state = State::IgnoreStringEscape;
                } else if c == 0x07 {
                    self.state = State::Ground;
                }
            }
            State::IgnoreStringEscape => {
                self.state = if ch == '\\' { State::Ground } else { State::IgnoreString };
            }
        }
    }

    fn enter_escape(&mut self) {
        self.state = State::Escape;
        self.intermediates.clear();
    }

    fn control(&mut self, b: u8) {
        match b {
            0x08 => {
                self.pending_wrap = false;
                self.col = self.col.saturating_sub(1);
            }
            0x09 => {
                self.pending_wrap = false;
                let mut c = self.col + 1;
                while c < self.cols - 1 && !self.tabs[c] {
                    c += 1;
                }
                self.col = c.min(self.cols - 1);
            }
            0x0A..=0x0C => self.linefeed(),
            0x0D => {
                self.pending_wrap = false;
                self.col = 0;
            }
            _ => {}
        }
    }

    fn escape(&mut self, ch: char) {
        self.state = State::Ground;
        match ch {
            '[' => {
                self.state = State::Csi;
                self.params.clear();
                self.cur_param = None;
                self.private = None;
                self.intermediates.clear();
            }
            ']' => {
                self.state = State::Osc;
                self.osc.clear();
            }
            'P' | 'X' | '^' | '_' => self.state = State::IgnoreString,
            '\x20'..='\x2F' => {
                self.intermediates.push(ch as u8);
                self.state = State::EscIntermediate;
            }
            _ => self.esc_dispatch(ch),
        }
    }

    fn esc_dispatch(&mut self, ch: char) {
        if !self.intermediates.is_empty() {
            // Character-set designations like ESC ( B: we are always UTF-8.
            self.intermediates.clear();
            return;
        }
        match ch {
            '7' => self.save_cursor(),
            '8' => self.restore_cursor(),
            'D' => self.linefeed(),
            'E' => {
                self.col = 0;
                self.linefeed();
            }
            'M' => self.reverse_index(),
            'H' => {
                if self.col < self.cols {
                    self.tabs[self.col] = true;
                }
            }
            'c' => {
                let (c, r, sb) = (self.cols, self.rows, std::mem::take(&mut self.scrollback));
                *self = Terminal::new(c, r);
                self.scrollback = sb;
            }
            _ => {}
        }
    }

    fn csi_byte(&mut self, ch: char) {
        let c = ch as u32;
        match c {
            0x30..=0x39 => {
                let d = c - 0x30;
                self.cur_param = Some(self.cur_param.unwrap_or(0).saturating_mul(10).saturating_add(d).min(65535));
            }
            // ';' and ':' both separate; colon sub-parameters are flattened.
            0x3A | 0x3B => {
                self.params.push(self.cur_param.take().unwrap_or(0));
            }
            0x3C..=0x3F => {
                if self.params.is_empty() && self.cur_param.is_none() {
                    self.private = Some(c as u8);
                }
            }
            0x20..=0x2F => self.intermediates.push(c as u8),
            0x40..=0x7E => {
                if let Some(p) = self.cur_param.take() {
                    self.params.push(p);
                }
                self.state = State::Ground;
                self.csi_dispatch(ch);
            }
            0x1B => self.enter_escape(),
            _ if c < 0x20 => self.control(c as u8),
            _ => {}
        }
    }

    fn p(&self, i: usize, default: u32) -> u32 {
        match self.params.get(i) {
            Some(&0) | None => default,
            Some(&v) => v,
        }
    }

    fn csi_dispatch(&mut self, ch: char) {
        let n = self.p(0, 1) as usize;
        if !self.intermediates.is_empty() {
            // e.g. CSI ... SP q (cursor style), CSI ! p (soft reset)
            if ch == 'p' && self.intermediates == b"!" {
                self.pen = Attrs::default();
                self.scroll_top = 0;
                self.scroll_bot = self.rows - 1;
                self.cursor_visible = true;
            }
            return;
        }
        match (self.private, ch) {
            (None, 'A') => self.move_to(self.row.saturating_sub(n), self.col),
            (None, 'B') | (None, 'e') => self.move_to(self.row + n, self.col),
            (None, 'C') | (None, 'a') => self.move_to(self.row, self.col + n),
            (None, 'D') => self.move_to(self.row, self.col.saturating_sub(n)),
            (None, 'E') => self.move_to(self.row + n, 0),
            (None, 'F') => self.move_to(self.row.saturating_sub(n), 0),
            (None, 'G') | (None, '`') => self.move_to(self.row, n - 1),
            (None, 'd') => self.move_to(n - 1, self.col),
            (None, 'H') | (None, 'f') => {
                let r = self.p(0, 1) as usize - 1;
                let c = self.p(1, 1) as usize - 1;
                self.move_to(r, c);
            }
            (None, 'J') | (Some(b'?'), 'J') => self.erase_display(self.params.first().copied().unwrap_or(0)),
            (None, 'K') | (Some(b'?'), 'K') => self.erase_line(self.params.first().copied().unwrap_or(0)),
            (None, 'X') => {
                let row = self.row;
                let end = (self.col + n).min(self.cols);
                self.clear_cells(row, self.col, end);
            }
            (None, '@') => self.insert_chars(n),
            (None, 'P') => self.delete_chars(n),
            (None, 'L') => self.insert_lines(n),
            (None, 'M') => self.delete_lines(n),
            (None, 'S') => self.scroll_up(n),
            (None, 'T') => self.scroll_down(n),
            (None, 'm') => self.sgr(),
            (None, 'r') => {
                let top = self.p(0, 1) as usize - 1;
                let bot = self.p(1, self.rows as u32) as usize - 1;
                if top < bot && bot < self.rows {
                    self.scroll_top = top;
                    self.scroll_bot = bot;
                    self.move_to(0, 0);
                }
            }
            (None, 's') => self.save_cursor(),
            (None, 'u') => self.restore_cursor(),
            (None, 'g') => match self.params.first().copied().unwrap_or(0) {
                0 => {
                    if self.col < self.cols {
                        self.tabs[self.col] = false;
                    }
                }
                3 => self.tabs.iter_mut().for_each(|t| *t = false),
                _ => {}
            },
            (None, 'n') => match self.params.first().copied().unwrap_or(0) {
                5 => self.responses.extend_from_slice(b"\x1b[0n"),
                6 => {
                    let s = format!("\x1b[{};{}R", self.row + 1, self.col.min(self.cols - 1) + 1);
                    self.responses.extend_from_slice(s.as_bytes());
                }
                _ => {}
            },
            (None, 'c') => {
                if self.params.first().copied().unwrap_or(0) == 0 {
                    self.responses.extend_from_slice(b"\x1b[?62;22c");
                }
            }
            (Some(b'>'), 'c') => self.responses.extend_from_slice(b"\x1b[>0;10;1c"),
            (None, 'h') | (None, 'l') => {}
            (Some(b'?'), 'h') => self.private_modes(true),
            (Some(b'?'), 'l') => self.private_modes(false),
            _ => {}
        }
    }

    fn private_modes(&mut self, on: bool) {
        for i in 0..self.params.len() {
            match self.params[i] {
                1 => self.app_cursor_keys = on,
                7 => self.autowrap = on,
                25 => self.cursor_visible = on,
                47 | 1047 | 1049 => self.set_alt_screen(on),
                2004 => self.bracketed_paste = on,
                _ => {}
            }
        }
    }

    fn set_alt_screen(&mut self, on: bool) {
        if on && self.alt_saved.is_none() {
            let saved = SavedCursor { row: self.row, col: self.col, pen: self.pen };
            let blank = vec![vec![Cell::default(); self.cols]; self.rows];
            let main = std::mem::replace(&mut self.grid, blank);
            self.alt_saved = Some((main, saved));
        } else if !on {
            if let Some((mut main, saved)) = self.alt_saved.take() {
                for line in main.iter_mut() {
                    line.resize(self.cols, Cell::default());
                }
                main.resize(self.rows, vec![Cell::default(); self.cols]);
                self.grid = main;
                self.row = saved.row.min(self.rows - 1);
                self.col = saved.col.min(self.cols - 1);
                self.pen = saved.pen;
            }
        }
        self.pending_wrap = false;
    }

    fn sgr(&mut self) {
        if self.params.is_empty() {
            self.pen = Attrs::default();
            return;
        }
        let mut i = 0;
        while i < self.params.len() {
            let v = self.params[i];
            match v {
                0 => self.pen = Attrs::default(),
                1 => self.pen.bold = true,
                3 => self.pen.italic = true,
                4 => self.pen.underline = true,
                7 => self.pen.inverse = true,
                22 => self.pen.bold = false,
                23 => self.pen.italic = false,
                24 => self.pen.underline = false,
                27 => self.pen.inverse = false,
                30..=37 => self.pen.fg = Color::Indexed((v - 30) as u8),
                39 => self.pen.fg = Color::Default,
                40..=47 => self.pen.bg = Color::Indexed((v - 40) as u8),
                49 => self.pen.bg = Color::Default,
                90..=97 => self.pen.fg = Color::Indexed((v - 90 + 8) as u8),
                100..=107 => self.pen.bg = Color::Indexed((v - 100 + 8) as u8),
                38 | 48 => {
                    let (color, used) = self.extended_color(i + 1);
                    if let Some(color) = color {
                        if v == 38 {
                            self.pen.fg = color;
                        } else {
                            self.pen.bg = color;
                        }
                    }
                    i += used;
                }
                _ => {}
            }
            i += 1;
        }
    }

    fn extended_color(&self, i: usize) -> (Option<Color>, usize) {
        match self.params.get(i) {
            Some(5) => (self.params.get(i + 1).map(|&n| Color::Indexed(n.min(255) as u8)), 2),
            Some(2) => {
                let g = |k: usize| self.params.get(i + k).map(|&x: &u32| x.min(255) as u8);
                match (g(1), g(2), g(3)) {
                    (Some(r), Some(gr), Some(b)) => (Some(Color::Rgb(r, gr, b)), 4),
                    _ => (None, self.params.len()),
                }
            }
            _ => (None, 1),
        }
    }

    fn osc_end(&mut self) {
        self.state = State::Ground;
        let text = String::from_utf8_lossy(&self.osc).into_owned();
        let (code, rest) = text.split_once(';').unwrap_or((text.as_str(), ""));
        match code {
            "0" | "2" => self.title = rest.to_string(),
            "133" => {
                let mut parts = rest.split(';');
                let mark = match parts.next() {
                    Some("A") => Some(Mark::PromptStart),
                    Some("B") => Some(Mark::CommandStart),
                    Some("C") => Some(Mark::OutputStart),
                    Some("D") => Some(Mark::CommandEnd(parts.next().and_then(|s| s.parse().ok()))),
                    _ => None,
                };
                if let Some(m) = mark {
                    let abs_row = self.scrollback.len() + self.row;
                    self.marks.push((abs_row, m));
                    if self.marks.len() > 10_000 {
                        self.marks.drain(..1000);
                    }
                }
            }
            _ => {}
        }
        self.osc.clear();
    }

    fn print(&mut self, ch: char) {
        let width = ch.width().unwrap_or(0);
        if width == 0 {
            return; // combining marks: not rendered yet
        }
        if self.pending_wrap {
            if self.autowrap {
                self.col = 0;
                self.linefeed();
            }
            self.pending_wrap = false;
        }
        if width == 2 && self.col == self.cols - 1 {
            if self.autowrap {
                self.grid[self.row][self.col] = Cell { attrs: self.pen, ..Cell::default() };
                self.col = 0;
                self.linefeed();
            } else {
                return;
            }
        }
        let (row, col) = (self.row, self.col);
        self.grid[row][col] = Cell { ch, attrs: self.pen, wide: width == 2, spacer: false };
        if width == 2 && col + 1 < self.cols {
            self.grid[row][col + 1] = Cell { ch: ' ', attrs: self.pen, wide: false, spacer: true };
        }
        let next = col + width;
        if next >= self.cols {
            self.col = self.cols - 1;
            self.pending_wrap = true;
        } else {
            self.col = next;
        }
    }

    fn move_to(&mut self, row: usize, col: usize) {
        self.row = row.min(self.rows - 1);
        self.col = col.min(self.cols - 1);
        self.pending_wrap = false;
    }

    fn linefeed(&mut self) {
        self.pending_wrap = false;
        if self.row == self.scroll_bot {
            self.scroll_up(1);
        } else if self.row < self.rows - 1 {
            self.row += 1;
        }
    }

    fn reverse_index(&mut self) {
        self.pending_wrap = false;
        if self.row == self.scroll_top {
            self.scroll_down(1);
        } else if self.row > 0 {
            self.row -= 1;
        }
    }

    fn blank_line(&self) -> Vec<Cell> {
        vec![Cell { attrs: Attrs { bg: self.pen.bg, ..Attrs::default() }, ..Cell::default() }; self.cols]
    }

    fn push_scrollback(&mut self, line: Vec<Cell>) {
        if self.max_scrollback == 0 {
            return;
        }
        if self.scrollback.len() >= self.max_scrollback {
            self.scrollback.pop_front();
        }
        self.scrollback.push_back(line);
    }

    fn scroll_up(&mut self, n: usize) {
        let n = n.min(self.scroll_bot - self.scroll_top + 1);
        for _ in 0..n {
            let line = self.grid.remove(self.scroll_top);
            if self.scroll_top == 0 && self.alt_saved.is_none() {
                self.push_scrollback(line);
            }
            let blank = self.blank_line();
            self.grid.insert(self.scroll_bot, blank);
        }
    }

    fn scroll_down(&mut self, n: usize) {
        let n = n.min(self.scroll_bot - self.scroll_top + 1);
        for _ in 0..n {
            self.grid.remove(self.scroll_bot);
            let blank = self.blank_line();
            self.grid.insert(self.scroll_top, blank);
        }
    }

    fn insert_lines(&mut self, n: usize) {
        if self.row < self.scroll_top || self.row > self.scroll_bot {
            return;
        }
        let n = n.min(self.scroll_bot - self.row + 1);
        for _ in 0..n {
            self.grid.remove(self.scroll_bot);
            let blank = self.blank_line();
            self.grid.insert(self.row, blank);
        }
        self.col = 0;
        self.pending_wrap = false;
    }

    fn delete_lines(&mut self, n: usize) {
        if self.row < self.scroll_top || self.row > self.scroll_bot {
            return;
        }
        let n = n.min(self.scroll_bot - self.row + 1);
        for _ in 0..n {
            self.grid.remove(self.row);
            let blank = self.blank_line();
            self.grid.insert(self.scroll_bot, blank);
        }
        self.col = 0;
        self.pending_wrap = false;
    }

    fn insert_chars(&mut self, n: usize) {
        let blank = Cell { attrs: Attrs { bg: self.pen.bg, ..Attrs::default() }, ..Cell::default() };
        let (row, col, cols) = (self.row, self.col, self.cols);
        let line = &mut self.grid[row];
        for _ in 0..n.min(cols - col) {
            line.pop();
            line.insert(col, blank);
        }
        self.pending_wrap = false;
    }

    fn delete_chars(&mut self, n: usize) {
        let blank = Cell { attrs: Attrs { bg: self.pen.bg, ..Attrs::default() }, ..Cell::default() };
        let (row, col, cols) = (self.row, self.col, self.cols);
        let line = &mut self.grid[row];
        for _ in 0..n.min(cols - col) {
            line.remove(col);
            line.push(blank);
        }
        self.pending_wrap = false;
    }

    fn clear_cells(&mut self, row: usize, from: usize, to: usize) {
        let blank = Cell { attrs: Attrs { bg: self.pen.bg, ..Attrs::default() }, ..Cell::default() };
        for c in from..to.min(self.cols) {
            self.grid[row][c] = blank;
        }
    }

    fn erase_line(&mut self, mode: u32) {
        let row = self.row;
        match mode {
            0 => self.clear_cells(row, self.col, self.cols),
            1 => self.clear_cells(row, 0, self.col + 1),
            2 => self.clear_cells(row, 0, self.cols),
            _ => {}
        }
        self.pending_wrap = false;
    }

    fn erase_display(&mut self, mode: u32) {
        match mode {
            0 => {
                self.erase_line(0);
                for r in self.row + 1..self.rows {
                    self.clear_cells(r, 0, self.cols);
                }
            }
            1 => {
                self.erase_line(1);
                for r in 0..self.row {
                    self.clear_cells(r, 0, self.cols);
                }
            }
            2 => {
                for r in 0..self.rows {
                    self.clear_cells(r, 0, self.cols);
                }
            }
            3 => self.scrollback.clear(),
            _ => {}
        }
        self.pending_wrap = false;
    }

    fn save_cursor(&mut self) {
        self.saved = SavedCursor { row: self.row, col: self.col, pen: self.pen };
    }

    fn restore_cursor(&mut self) {
        let s = self.saved;
        self.move_to(s.row, s.col);
        self.pen = s.pen;
    }
}

fn default_tabs(cols: usize) -> Vec<bool> {
    (0..cols).map(|c| c % 8 == 0 && c != 0).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prints_and_wraps() {
        let mut t = Terminal::new(5, 3);
        t.feed(b"hello world");
        assert_eq!(t.row_text(0), "hello");
        assert_eq!(t.row_text(1), " worl");
        assert_eq!(t.row_text(2), "d");
    }

    #[test]
    fn scrolls_into_history() {
        let mut t = Terminal::new(10, 2);
        t.feed(b"one\r\ntwo\r\nthree");
        assert_eq!(t.scrollback_len(), 1);
        assert_eq!(t.row_text(0), "two");
        assert_eq!(t.row_text(1), "three");
        assert_eq!(t.line(0, 1)[0].ch, 'o');
    }

    #[test]
    fn cursor_moves_and_erase() {
        let mut t = Terminal::new(10, 3);
        t.feed(b"abcdef\x1b[1;3H\x1b[K");
        assert_eq!(t.row_text(0), "ab");
        t.feed(b"\x1b[2J\x1b[2;2HX");
        assert_eq!(t.row_text(0), "");
        assert_eq!(t.row_text(1), " X");
    }

    #[test]
    fn colors() {
        let mut t = Terminal::new(10, 1);
        t.feed(b"\x1b[1;31mR\x1b[38;2;1;2;3mG\x1b[0mN");
        let l = t.line(0, 0);
        assert_eq!(l[0].attrs.fg, Color::Indexed(1));
        assert!(l[0].attrs.bold);
        assert_eq!(l[1].attrs.fg, Color::Rgb(1, 2, 3));
        assert_eq!(l[2].attrs, Attrs::default());
    }

    #[test]
    fn utf8_split_and_wide() {
        let mut t = Terminal::new(10, 1);
        let s = "é中".as_bytes();
        t.feed(&s[..1]);
        t.feed(&s[1..]);
        let l = t.line(0, 0);
        assert_eq!(l[0].ch, 'é');
        assert_eq!(l[1].ch, '中');
        assert!(l[1].wide && l[2].spacer);
        assert_eq!(t.cursor(), (0, 3));
    }

    #[test]
    fn cursor_report_and_title() {
        let mut t = Terminal::new(10, 5);
        t.feed(b"\x1b[3;4H\x1b[6n\x1b]0;hi there\x07");
        assert_eq!(t.take_responses(), b"\x1b[3;4R");
        assert_eq!(t.title, "hi there");
    }

    #[test]
    fn alt_screen_restores() {
        let mut t = Terminal::new(10, 2);
        t.feed(b"main\x1b[?1049h\x1b[Halt");
        assert_eq!(t.row_text(0), "alt");
        t.feed(b"\x1b[?1049l");
        assert_eq!(t.row_text(0), "main");
    }

    #[test]
    fn scroll_region() {
        let mut t = Terminal::new(5, 4);
        t.feed(b"a\r\nb\r\nc\r\nd\x1b[2;3r\x1b[3;1H\n");
        assert_eq!(t.row_text(0), "a");
        assert_eq!(t.row_text(1), "c");
        assert_eq!(t.row_text(2), "");
        assert_eq!(t.row_text(3), "d");
    }

    #[test]
    fn selection_text_spans_history() {
        let mut t = Terminal::new(10, 2);
        t.feed(b"one\r\ntwo\r\nthree");
        assert_eq!(t.total_lines(), 3);
        assert_eq!(t.text_between((0, 1), (2, 2)), "ne\ntwo\nthr");
        assert_eq!(t.text_between((2, 2), (0, 1)), "ne\ntwo\nthr");
    }

    #[test]
    fn shell_marks() {
        let mut t = Terminal::new(10, 2);
        t.feed(b"\x1b]133;A\x07$ \x1b]133;B\x07ls\r\n\x1b]133;C\x07out\r\n\x1b]133;D;0\x07");
        let kinds: Vec<_> = t.marks.iter().map(|m| m.1.clone()).collect();
        assert_eq!(kinds, vec![Mark::PromptStart, Mark::CommandStart, Mark::OutputStart, Mark::CommandEnd(Some(0))]);
    }
}
