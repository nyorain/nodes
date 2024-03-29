use super::util;
use nodes::pattern;

use std::{cmp, io, thread};
use std::sync::{Mutex, Arc};
use std::io::prelude::*;
use std::io::BufWriter;
use std::time::Duration;

use termion::event::Key;
use termion::input::TermRead;
use termion::raw::IntoRawMode;
use signal_hook::{iterator::Signals, SIGWINCH};

use rusqlite::Connection;
use scopeguard::defer;

#[derive(Clone)]
struct SelectNode {
    id: u32,
    priority: i32,
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

    // config
    cursor_off: usize,

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
            cursor_off: 20,

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
        // TODO: solution for keeping selected nodes after reload can be improved
        let selected: Vec<u32> = self.nodes.iter()
            .filter(|node| node.selected)
            .map(|node| node.id)
            .collect();

        let mut nodes = Vec::new();
        util::iter_nodes(conn, &self.args, |node| {
            // we use the whole first line as summary since we don't reload
            // the summary on every terminal resize
            let summary = node.content.lines().next().unwrap_or("").to_string();
            let tags = node.tags.iter().map(|s| s.to_string()).collect();
            nodes.push(SelectNode{
                id: node.id,
                priority: node.priority,
                summary: summary,
                selected: selected.contains(&node.id),
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
            let width = (self.termx() as usize) - idstr.len() - 3;
            let mut sumwidth = width;
            let mut tagswidth = 0;
            // TODO: don't hardcode distribution
            if width > 80 {
                sumwidth = cmp::max(60, (width as f64 * 0.7) as usize);
                tagswidth = width - sumwidth;
            }

            // let mut tags = String::new();
            let mut tags = format!("({})", node.priority);
            if tagswidth > 0 && !node.tags.is_empty() {
                tags += &("[".to_string() + &node.tags.join("][") + "]");

                // tags = util::short_string(&tags, tagswidth);
                // TODO: only show tags that can be completely shown
                // and are not cut off (maybe add [...] in the case
                // that there are remaining tags?)
            }

            // shorten, maybe terminal was resized since then
            let summary = util::short_string(&node.summary, sumwidth);

            // TODO: clear line first?
            write!(self.screen, "{}{}: {:<sw$} {:>tw$.tw$}",
                termion::cursor::Goto(x, y),
                node.id, summary, tags,
                sw = sumwidth, tw = tagswidth).unwrap();

            y += 1;
            i += 1;
        }

        // clear remaining screen
        if y < self.termy() {
            write!(self.screen, "{}{}{}{}",
                termion::cursor::Goto(x, y),
                BG_RESET, FG_RESET,
                termion::clear::AfterCursor).unwrap();
        }

        // render special state-dependent stuff
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

    pub fn correct_hover(&mut self) {
        if self.nodes.is_empty() {
            self.hover = 0;
            self.start = 0;
            return;
        }

        self.hover = cmp::min(self.nodes.len() - 1, self.hover);

        let topd = cmp::min(self.cursor_off, self.hover);
        let topd = cmp::min(topd, (self.termy() as usize) / 2);
        let top = self.start + topd;
        if self.hover < top {
            self.start = self.hover;
            self.start = self.start.saturating_sub(topd);
        }

        let botd = cmp::min(self.cursor_off, self.nodes.len() - 1 - self.hover);
        let botd = cmp::min(botd, (self.termy() as usize - 1) / 2);
        let bot = self.start + (self.termy() as usize);
        let bot = bot.saturating_sub(botd);
        if self.hover >= bot {
            self.start = self.hover + botd;
            self.start = self.start.saturating_sub((self.termy() - 1) as usize);
        }
    }

    // moves cursor down by n
    pub fn cursor_down(&mut self, n: usize) {
        self.hover += n;
        self.correct_hover();
    }

    // moves cursor up by n
    pub fn cursor_up(&mut self, n: usize) {
        self.hover = self.hover.saturating_sub(n);
        self.correct_hover();
    }

    // The bool returns whether the hovered node is returned instead
    // of the selection
    pub fn selection_or_hover(&self) -> (Vec<u32>, bool) {
        // TODO: could be done more efficiently if we keep track
        // of selected nodes in a `Vec<u32> selected`...
        let selected: Vec<u32> = self.nodes.iter()
            .filter(|node| node.selected)
            .map(|node| node.id)
            .collect();
        if selected.is_empty() && !self.nodes.is_empty() {
            (vec!(self.nodes[self.hover].id), true)
        } else {
            (selected, false)
        }
    }

    pub fn archive(&mut self, conn: &Connection) {
        let (selected, hovered) = self.selection_or_hover();
        if hovered {
            util::toggle_archived(conn, selected[0]).unwrap();
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

    pub fn resized(&mut self, size: (u16, u16)) {
        self.termsize = size;
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

    fn next_sort_mode(&mut self) {
        self.args.sort = match &self.args.sort {
            Some(util::Sort::ID) => { Some(util::Sort::Edited) },
            Some(util::Sort::Edited) => { Some(util::Sort::Priority) },
            Some(util::Sort::Priority) => { Some(util::Sort::ID) },
            None => None,
        };
    }

    fn set_hover_to_id(&mut self, id: u32) {
        for (i, node) in self.nodes.iter().enumerate() {
            if node.id == id {
                self.hover = i;
                break;
            }
        }

        self.correct_hover();
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
            Key::Char(' ') if !self.nodes.is_empty() => { // toggle selection
                self.nodes[self.hover].selected ^= true;
            },
            Key::Char('e') | Key::Char('\n') if !self.nodes.is_empty() => { // edit
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
                if !sel.is_empty() {
                    self.delete_sel = sel;
                    self.delete_hover = dhover;
                    self.state = State::Delete;
                }
            },
            Key::Char('/') => { // search
                // enter search mode
                self.state = State::Search;
            },
            Key::Char(':') => {
                self.state = State::Command;
            },
            Key::Ctrl('o') => {
                self.next_sort_mode();
                self.reload_nodes(conn);
            },
            Key::Char('J') => {
                let hover = self.nodes[self.hover].id;
                let (nodes, _) = self.selection_or_hover();
                util::priority_add(conn, &nodes, -1).unwrap();
                self.reload_nodes(conn);
                self.set_hover_to_id(hover);
            },
            Key::Char('K') => {
                let hover = self.nodes[self.hover].id;
                let (nodes, _) = self.selection_or_hover();
                util::priority_add(conn, &nodes, 1).unwrap();
                self.reload_nodes(conn);
                self.set_hover_to_id(hover);
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

        self.render();
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

    pub fn exec_cmd(&mut self, args: &[&str], conn: &Connection) {
        match args[0] {
            // TODO: technically we don't have to reload from sql.
            // we could also just add/remove the tags ourselves,
            // better performance. But otherwise that might
            // have correction issues in some cases (not representing
            // sql state)?
            "t" | "tag" if args.len() > 1 => {
                // TODO: remove pure whitespace args/tags
                let (nodes, _) = self.selection_or_hover();
                util::add_tags(conn, &nodes, &args[1..]).unwrap();
                self.reload_nodes(conn);
            },
            "ut" | "untag" if args.len() > 1 => {
                let (nodes, _) = self.selection_or_hover();
                util::remove_tags(conn, &nodes, &args[1..]).unwrap();
                self.reload_nodes(conn);
            },
            // TODO: is using 2 commands really intuitive?
            // maybe rather something like ":a true|false|both"?
            "a" => { // toggle show archived
                self.args.archived = match self.args.archived {
                    None => Some(false),
                    Some(false) => None,
                    // don't toggle it in this case, see :A
                    Some(true) => Some(true),
                };
                self.reload_nodes(conn);
            },
            "A" => { // toggle only show archived
                self.args.archived = match self.args.archived {
                    Some(true) => Some(false),
                    _ => Some(true),
                };
                self.reload_nodes(conn);
            }
            _ => (), // Invalid
        }
    }

    // TODO: better specific tagging modes (starting just via 't' in normal mode)
    // show context-sensitive suggestions, enter will confirm/use them immediately
    pub fn input_cmd(&mut self, key: Key, conn: &Connection) -> bool {
        let mut end = false;
        let mut exec = false;
        let mut change = true;
        match key {
            Key::Esc | Key::Ctrl('c') | Key::Ctrl('d')  => {
                self.command.clear();
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
            let mut command = String::new();
            std::mem::swap(&mut command, &mut self.command);
            let args: Vec<&str> = command
                .split(|c| c == ',' || c == ' ')
                .collect();
            self.exec_cmd(&args, &conn);
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

// NOTE: probably cleaner implementation using channels...
pub fn select(conn: &Connection, args: &clap::ArgMatches) -> i32 {
    let nodes: Vec<SelectNode>;

    // when scope exits the terminal was restored
    // setup terminal
    {
        let stdin = io::stdin();
        let raw = match termion::get_tty().and_then(|tty| tty.into_raw_mode()) {
            Ok(r) => r,
            Err(err) => {
                println!("Failed to transform tty into raw mode: {}", err);
                return -2;
            }
        };

        // set up screen
        let screen = termion::screen::AlternateScreen::from(raw);
        // 256K capacity in the BufWriter since we don't ever want to flush
        // before we have to (and render a partial result)
        let mut screen = BufWriter::with_capacity(1024 * 256, screen);
        if let Err(err) = write!(screen, "{}", termion::cursor::Hide) {
            println!("Failed to hide cursor in selection screen: {}", err);
            return -3;
        }

        let ms = Arc::new(Mutex::new(SelectScreen::new(&conn, &args, screen)));
        use std::sync::atomic;
        let run_size = Arc::new(atomic::AtomicBool::new(true));

        let trun_size = run_size.clone();
        let tms = ms.clone();

        // TODO: use signal again instead
        // cleanup cleanup process
        let sizet = thread::spawn(move || {
            let mut termsize = util::terminal_size();
            while trun_size.load(atomic::Ordering::SeqCst) {
                let ntermsize = util::terminal_size();
                if ntermsize != termsize {
                    termsize = ntermsize;
                    let mut s = tms.lock().unwrap();
                    s.resized(termsize);
                }

                thread::sleep(Duration::from_millis(50));
            }

            // alternative, more efficient implementation
            // problem: not sure how to terminate when joining...
            /*
            let signals = Signals::new(&[SIGWINCH]).unwrap();
            for sig in signals.forever() {
                if sig == SIGWINCH {
                    // sigsender.send(Message::Resized).unwrap();
                    let mut s = tms.lock().unwrap();
                    eprintln!("resizing");
                    s.resized(util::terminal_size());
                }
            }
            */
        });

        // make sure terminal is cleaned up
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

        let keys = stdin.keys();
        for c in keys {
            let c = c.unwrap();
            let mut s = ms.lock().unwrap();
            if !s.input(c, conn) {
                break;
            }
        }

        // we join the thread so that the terminal is converted back
        run_size.store(false, atomic::Ordering::SeqCst);
        sizet.join().unwrap();
        nodes = ms.lock().unwrap().nodes.clone();
    }

    // output selected nodes
    for node in nodes {
        if node.selected {
            println!("{}", node.id);
        }
    }

    0
}
