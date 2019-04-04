use super::util;
use nodes::pattern;

use std::{cmp, io, thread};
use std::sync::{Mutex, Arc};
use std::io::prelude::*;
use std::io::BufWriter;

use termion::event::Key;
use termion::input::Keys;
use termion::screen::*;
use termion::input::TermRead;
use termion::raw::IntoRawMode;

use signal_hook::{iterator::Signals, SIGWINCH};
use rusqlite::Connection;
use scopeguard::defer;

struct SelectNode {
    id: u32,
    summary: String,
    selected: bool,
    tags: Vec<String>,
}

enum State {
    Normal,
    Search,
    Command,
    Delete,
}

struct SelectScreen<W: Write> {
    args: util::ListArgs, // invariant: pattern always Some
    nodes: Vec<SelectNode>,
    hover: usize, // index of node the cursor is over
    start: usize, // in of first node currently displayed
    termsize: (u16, u16), // TODO: handle SIGWINCH as resize handler
    pattern: String, // current search filter
    screen: W,
    state: State,

    // state stuff
    delete_hover: bool,
    delete_sel: Vec<u32>,
    command: String,
    action_count: usize,
    gpending: bool,
}

const FG_RESET: termion::color::Fg<termion::color::Reset> =
    termion::color::Fg(termion::color::Reset);
const BG_RESET: termion::color::Bg<termion::color::Reset> =
    termion::color::Bg(termion::color::Reset);

impl<W: Write> SelectScreen<W> {
    pub fn new(conn: &Connection, args: &clap::ArgMatches, screen: W)
            -> SelectScreen<W> {

        let mut s = SelectScreen {
            args: util::extract_list_args(&args, true, true),
            nodes: Vec::new(),
            hover: 0,
            start: 0,
            termsize: util::terminal_size(),
            pattern: String::new(),
            state: State::Normal,
            screen: screen,

            delete_hover: false,
            delete_sel: Vec::new(),
            command: String::new(),
            action_count: 0,
            gpending: false,
        };

        // initial load and render
        s.reload_nodes(conn);
        s.render();
        s
    }

    pub fn reload_nodes(&mut self, conn: &Connection) {
        let termsize = util::terminal_size();
        let width = (termsize.0 - 8) as usize;

        let mut nodes = Vec::new();
        util::iter_nodes(conn, &self.args, |node| {
            let summary = util::node_summary(&node.content, 1, width);
            let tags = node.tags.iter().map(|s| s.to_string()).collect();
            nodes.push(SelectNode{
                id: node.id,
                summary: summary,
                selected: false,
                tags: tags,
            });
        });
        self.nodes = nodes;
    }

    pub fn reparse_pattern(&mut self) -> bool {
        if self.pattern.is_empty() {
            let changed = self.args.pattern.is_some();
            self.args.pattern = None;
            return changed;
        }

        match pattern::parse_condition(&self.pattern) {
            Err(_) => {
                // TODO: log invalid pattern? show it somewhere?
                // some kind of visual feedback? maybe merk it red?
                false
            }, Ok(pattern) => {
                self.args.pattern = Some(pattern);
                true
            }
        }
    }

    // renders without flush
    pub fn render_nf(&mut self) {
        let bg_current = termion::color::Bg(termion::color::LightGreen);
        let fg_selected = termion::color::Fg(termion::color::LightRed);
        let x = 1;

        let mut y = 1;
        let mut i = self.start;
        for node in self.nodes[self.start..].iter() {
            if y > self.termy() {
                break;
            }

            if i == self.hover {
                write!(self.screen, "{}", bg_current).unwrap();
            } else {
                write!(self.screen, "{}", BG_RESET).unwrap();
            }

            if node.selected {
                write!(self.screen, "{}", fg_selected).unwrap();
            } else {
                write!(self.screen, "{}", FG_RESET).unwrap();
            }

            let idstr = node.id.to_string();
            let width = (self.termx() as usize) - idstr.len() - 2;
            let mut sumwidth = width;
            let mut tagswidth = 0;
            // TODO: don't hardcode distribution
            if width > 80 {
                sumwidth = cmp::max(60, (width as f64 * 0.7) as usize);
                tagswidth = width - sumwidth;
            }

            let mut tags = String::new();
            if !node.tags.is_empty() {
                tags = "[".to_string() + &node.tags.join("][") + "]";
            }

            write!(self.screen, "{}{}{}: {:<sw$}  {:>tw$}",
                termion::cursor::Goto(x, y),
                termion::clear::CurrentLine,
                node.id, node.summary, tags,
                sw = sumwidth - 2, tw = tagswidth).unwrap();

            y += 1;
            i += 1;
        }

        if y < self.termy() {
            write!(self.screen, "{}{}{}{}",
                termion::cursor::Goto(x, y + 1),
                BG_RESET, FG_RESET,
                termion::clear::AfterCursor).unwrap();
        }

        match self.state {
            State::Command => self.render_command(),
            State::Delete => self.render_delete(),
            State::Search => self.render_search(),
            _ => (),
        };
    }

    pub fn render(&mut self) {
        self.render_nf();
        self.screen.flush().unwrap();
    }

    pub fn termx(&self) -> u16 {
        self.termsize.0
    }

    pub fn termy(&self) -> u16 {
        self.termsize.1
    }

    pub fn clear_selection(&mut self) {
        for node in &mut self.nodes {
            node.selected = false;
        }
    }

    // moves cursor down by n
    pub fn cursor_down(&mut self, n: usize) {
        self.hover = cmp::min(self.nodes.len() - 1, self.hover + n);
        if self.hover - self.start >= (self.termy() as usize) {
            self.start = self.hover - ((self.termy() - 1) as usize);
        }
    }

    // moves cursor up by n
    pub fn cursor_up(&mut self, n: usize) {
        self.hover = self.hover.saturating_sub(n);
        if self.hover < self.start {
            self.start = self.hover;
        }
    }

    pub fn selection_or_hover(&self) -> (Vec<u32>, bool) {
        // TODO: could be done more efficiently if we keep track
        // of selected nodes in a `Vec<u32> selected`...
        let selected: Vec<u32> = self.nodes.iter()
            .filter(|node| node.selected)
            .map(|node| node.id)
            .collect();
        if selected.is_empty() {
            (vec!(self.nodes[self.hover].id), true)
        } else {
            (selected, false)
        }
    }

    pub fn archive(&mut self, conn: &Connection) {
        let selected: Vec<u32> = self.nodes.iter()
            .filter(|node| node.selected)
            .map(|node| node.id)
            .collect();
        if selected.is_empty() {
            let id = self.nodes[self.hover].id;
            util::toggle_archived(conn, id).unwrap();
            if self.args.archived.is_some() {
                self.nodes.remove(self.hover);
            }
            return;
        }

        util::toggle_archived_range(conn, &selected).unwrap();
        if self.args.archived.is_some() {
            self.nodes.retain(|node| !node.selected);
        }
    }

    pub fn resized(&mut self) {
        self.termsize = util::terminal_size();
        self.render();
    }

    // Returns whether another iteration should be done, i.e. returns
    // false when screen should exit
    pub fn input(&mut self, key: Key, conn: &Connection) -> bool {
        match self.state {
            State::Normal => self.input_normal(key, conn),
            State::Search => self.input_search(key, conn),
            State::Command => self.input_cmd(key, conn),
            State::Delete => self.input_delete(key, conn),
        }
    }

    pub fn input_normal(&mut self, key: Key, conn: &Connection) -> bool {
        let mut reset_acount = true;
        let mut reset_gpending = true;
        let mut changed = true;
        match key {
            Key::Char('q') => { // quit
                return false;
            }
            Key::Char('j') | Key::Down => { // down
                self.cursor_down(cmp::max(self.action_count, 1));
            },
            Key::Char('k') | Key::Up => { // up
                self.cursor_up(cmp::max(self.action_count, 1));
            },
            Key::Char('G') | Key::End => { // end of list
                self.hover = self.nodes.len() - 1;
                self.start = self.hover.saturating_sub(
                    (self.termy() - 1) as usize);
            },
            Key::Home => { // beginning of list, like gg
                self.start = 0;
                self.hover = 0;
            },
            Key::Char('g') => { // beginning of list; gg detection
                if self.gpending {
                    self.start = 0;
                    self.hover = 0;
                } else {
                    self.gpending = true;
                    reset_gpending = false;
                    changed = false;
                }
            },
            Key::Char(' ') => { // toggle selection
                self.nodes[self.hover].selected ^= true;
            },
            Key::Char('e') | Key::Char('\n') => { // edit
                write!(self.screen, "{}", termion::screen::ToMainScreen).unwrap();
                util::edit(conn, self.nodes[self.hover].id).unwrap();
                write!(self.screen, "{}{}{}",
                    termion::screen::ToAlternateScreen,
                    termion::clear::All,
                    termion::cursor::Hide).unwrap();
                self.reload_nodes(conn);
            },
            Key::Char('c') => {
                write!(self.screen, "{}", termion::screen::ToMainScreen).unwrap();
                // TODO: display error/id in some kind of status line
                // could display it with timeout (like 1 or 2 seconds)
                // we wouldn't need an extra thread for that, enough to
                // check on user input
                match util::create(conn, None) {
                    Ok(_) => (),
                    Err(err) => {
                        eprintln!("{}", err);
                    }
                }
                write!(self.screen, "{}{}{}",
                    termion::screen::ToAlternateScreen,
                    termion::clear::All,
                    termion::cursor::Hide).unwrap();
                self.reload_nodes(conn);
            },
            Key::Char(c) if c.is_digit(10) => { // number for action count
                let digit = c.to_digit(10).unwrap() as usize;
                self.action_count = self.action_count.saturating_mul(10);
                self.action_count = self.action_count.saturating_add(digit);
                reset_acount = false;
                changed = false;
            },
            Key::Char('a') => { // archive
                self.archive(conn);
            },
            Key::Char('r') => { // reload
                self.termsize = util::terminal_size();
                self.reload_nodes(conn);
            },
            Key::Char('s') => { // clear selection
                self.clear_selection();
            },
            Key::Char('d') | Key::Delete => {
                // enter delete mode (confirmation)
                let (sel, dhover) = self.selection_or_hover();
                self.delete_sel = sel;
                self.delete_hover = dhover;
                self.state = State::Delete;
            },
            Key::Char('/') => { // search
                // enter search mode
                self.state = State::Search;
            },
            Key::Char(':') => {
                self.state = State::Command;
            },
            // TODO:
            // - page down/up
            // - allow to open/show multiple at once?
            //   maybe allow to edit/show selected?
            // - "u": undo?
            _ => changed = false,
        }

        if reset_gpending {
            self.gpending = false;
        }

        if reset_acount {
            self.action_count = 0;
        }

        // re-render whole screen
        if changed {
            self.render();
        }

        true
    }

    fn render_search(&mut self) {
        write!(self.screen, "{}{}{}{}/{}",
            termion::cursor::Goto(1, self.termy()),
            termion::clear::CurrentLine,
            termion::color::Fg(termion::color::Reset),
            termion::color::Bg(termion::color::Reset),
            self.pattern).unwrap();
    }

    pub fn input_search(&mut self, key: Key, conn: &Connection) -> bool {
        let mut changed = true;
        let mut end = false;

        // TODO: cursor
        // maybe general utility for line input?
        match key {
            Key::Esc | Key::Ctrl('c') | Key::Ctrl('d') => {
                end = true;
                self.pattern.clear();
            },
            Key::Char('\n') => {
                end = true;
                changed = false;
            },
            Key::Backspace => {
                if self.pattern.pop().is_none() {
                    end = true;
                    changed = false;
                }
            },
            Key::Char(c) => {
                self.pattern.push(c);
            },
            _ => changed = false,
        }

        if changed {
            if self.reparse_pattern() {
                // TODO: we could theoretically track them and jump to
                // nearest node that still exists for new pattern
                self.hover = 0;
                self.start = 0;
                self.reload_nodes(conn);
            }
        }

        if end {
            // switch back to normal mode
            self.state = State::Normal;
        }

        if changed || end {
            self.render();
        }

        true
    }

    pub fn render_delete(&mut self) {
        let mut nodestxt = "selected nodes".to_string();
        if self.delete_sel.len() == 1 {
            nodestxt = format!("node {}", self.delete_sel[0]);
        }

        write!(self.screen, "{}{}{}{}Delete {}? [y/n]",
            termion::cursor::Goto(1, self.termy()),
            termion::clear::CurrentLine,
            termion::color::Fg(termion::color::LightRed),
            termion::color::Bg(termion::color::Reset),
            nodestxt).unwrap();
    }

    pub fn input_delete(&mut self, key: Key, conn: &Connection) -> bool {
        let mut end = false;
        match key {
            Key::Char('n') |
                Key::Char('N') |
                Key::Esc |
                Key::Ctrl('d') |
                Key::Ctrl('c') => {
                    end = true;
            },
            Key::Char('y') | Key::Char('Y') => {
                end = true;
                util::delete_range(conn, &self.delete_sel).unwrap();
                if self.delete_hover {
                    self.nodes.remove(self.hover);
                } else {
                    self.nodes.retain(|node| !node.selected);
                }
            },
            _ => (),
        }

        if end {
            self.state = State::Normal;
        }

        true
    }

    fn render_command(&mut self) {
        write!(self.screen, "{}{}{}{}:{}",
            termion::clear::CurrentLine,
            termion::cursor::Goto(1, self.termy()),
            termion::color::Fg(termion::color::Reset),
            termion::color::Bg(termion::color::Reset),
            self.command).unwrap();
    }

    // TODO: better specific tagging modes (starting just via 't' in normal mode)
    // show context-sensitive suggestions, enter will confirm/use them immediately
    pub fn input_cmd(&mut self, key: Key, conn: &Connection) -> bool {
        let mut end = false;
        let mut exec = false;
        let mut change = true;
        match key {
            Key::Esc | Key::Ctrl('c') | Key::Ctrl('d')  => {
                end = true;
            },
            Key::Char('\n') => {
                end = true;
                exec = true;
            },
            Key::Backspace => {
                if self.command.pop().is_none() {
                    end = true;
                }
            },
            Key::Char(c) => {
                self.command.push(c);
            },
            _ => change = false,
        }

        if exec {
            // handle command
            let args: Vec<&str> = self.command
                .split(|c| c == ',' || c == ' ')
                .collect();
            match args[0] {
                // TODO: technically we don't have to reload from sql.
                // we could also just add/remove the tags ourselves,
                // better performance. But otherwise that might
                // have correction issues in some cases (not representing
                // sql state)?
                "tag" if args.len() > 1 => {
                    let (nodes, _) = self.selection_or_hover();
                    util::add_tags(conn, &nodes, &args[1..]).unwrap();
                    self.reload_nodes(conn);
                },
                "untag" if args.len() > 1 => {
                    let (nodes, _) = self.selection_or_hover();
                    util::remove_tags(conn, &nodes, &args[1..]).unwrap();
                    self.reload_nodes(conn);
                },
                _ => (), // Invalid
            }
            self.command = String::new();
        }

        if end {
            self.state = State::Normal;
        }

        if change || exec || end {
            self.render();
        }

        true
    }
}

pub fn select(conn: &Connection, args: &clap::ArgMatches) -> i32 {
    // setup terminal
    let stdin = io::stdin();
    let raw = match termion::get_tty().and_then(|tty| tty.into_raw_mode()) {
        Ok(r) => r,
        Err(err) => {
            println!("Failed to transform tty into raw mode: {}", err);
            return -2;
        }
    };

    // set up screen
    // let ascreen = AlternateScreen::from(raw);
    // let mut screen = BufWriter::new(ascreen);
    let mut screen = BufWriter::new(raw);
    if let Err(err) = write!(screen, "{}", termion::cursor::Hide) {
        println!("Failed to hide cursor in selection screen: {}", err);
        return -3;
    }

    let ms = Arc::new(Mutex::new(SelectScreen::new(&conn, &args, screen)));

    // signal handler
    {
        defer!{{
            let mut screen = ms.lock().unwrap();
            write!(screen.screen, "{}{}{}{}",
                termion::clear::All,
                termion::cursor::Goto(1, 1),
                termion::cursor::Show,
                termion::screen::ToMainScreen,
            ).unwrap();
            screen.screen.flush().unwrap();
        }};

        let tms = ms.clone();
        let t = thread::spawn(move || {
            let signals = Signals::new(&[SIGWINCH]).unwrap();
            for sig in signals.forever() {
                if sig == SIGWINCH {
                    // sigsender.send(Message::Resized).unwrap();
                    let mut s = tms.lock().unwrap();
                    eprintln!("resizing");
                    s.resized();
                }
            }
        });

        let keys = stdin.keys();
        for c in keys {
            let c = c.unwrap();
            let mut s = ms.lock().unwrap();
            if !s.input(c, conn) {
                break;
            }
        }
    }

    // output selected nodes
    let mut s = ms.lock().unwrap();
    for node in &s.nodes {
        if node.selected {
            println!("{}", node.id);
        }
    }

    0
}
