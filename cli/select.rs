use super::util;

use std::cmp;
use std::io;
use std::io::prelude::*;
use std::io::BufWriter;

use termion::event::Key;
use termion::input::Keys;
use termion::screen::*;
use termion::input::TermRead;
use termion::raw::IntoRawMode;

use rusqlite::Connection;

struct SelectNode {
    id: u32,
    summary: String,
    selected: bool,
}

struct SelectScreen<'a> {
    conn: &'a Connection,
    args: util::ListArgs, // invariant: pattern always Some
    nodes: Vec<SelectNode>,
    hover: usize, // index of node the cursor is over
    start: usize, // in of first node currently displayed
    termsize: (u16, u16), // TODO: handle SIGWINCH as resize handler
}

const FG_RESET: termion::color::Fg<termion::color::Reset> =
    termion::color::Fg(termion::color::Reset);
const BG_RESET: termion::color::Bg<termion::color::Reset> =
    termion::color::Bg(termion::color::Reset);

impl<'a> SelectScreen<'a> {
    pub fn new(conn: &'a Connection, args: &clap::ArgMatches) -> SelectScreen<'a> {
        let mut s = SelectScreen {
            conn: &conn,
            args: util::extract_list_args(&args, true, true),
            nodes: Vec::new(),
            hover: 0,
            start: 0,
            termsize: util::terminal_size()
        };

        s.reload_nodes();
        s
    }

    pub fn reload_nodes(&mut self) {
        let termsize = util::terminal_size();
        let width = (termsize.0 - 8) as usize;

        let mut nodes = Vec::new();
        util::iter_nodes(&self.conn, &self.args, |node| {
            let summary = util::node_summary(&node.content, 1, width);
            nodes.push(SelectNode{
                id: node.id,
                summary: summary,
                selected: false
            });
        });
        self.nodes = nodes;
    }

    pub fn write_nodes<W: Write>(&mut self, screen: &mut W) {
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
                write!(screen, "{}", bg_current).unwrap();
            } else {
                write!(screen, "{}", BG_RESET).unwrap();
            }

            if node.selected {
                write!(screen, "{}", fg_selected).unwrap();
            } else {
                write!(screen, "{}", FG_RESET).unwrap();
            }

            let idstr = node.id.to_string();

            let width = (self.termx() as usize) - idstr.len() - 2;
            write!(screen, "{}{}{}: {:<w$}",
                termion::cursor::Goto(x, y),
                termion::clear::CurrentLine,
                idstr, node.summary, w = width).unwrap();

            y += 1;
            i += 1;
        }

        if y < self.termy() {
            write!(screen, "{}{}{}{}",
                termion::cursor::Goto(x, y + 1),
                BG_RESET, FG_RESET,
                termion::clear::AfterCursor).unwrap();
        }
    }

    pub fn termx(&self) -> u16 {
        self.termsize.0
    }

    pub fn termy(&self) -> u16 {
        self.termsize.1
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

    pub fn archive(&mut self) {
        let selected: Vec<u32> = self.nodes.iter()
            .filter(|node| node.selected)
            .map(|node| node.id)
            .collect();
        if selected.is_empty() {
            let id = self.nodes[self.hover].id;
            util::toggle_archived(&self.conn, id).unwrap();
            self.nodes.remove(self.hover);
            return;
        }

        util::toggle_archived_range(&self.conn, &selected).unwrap();
        self.nodes.retain(|node| !node.selected);
    }

    pub fn run_normal<R: Read, W: Write>(&mut self, screen: &mut W, keys: &mut Keys<R>) {
        let mut gpending = false;
        let mut acount: usize = 0; // action count

        // initial render
        self.write_nodes(screen);
        screen.flush().unwrap();

        // react to input
        loop {
            let c = keys.next().unwrap();

            let mut reset_acount = true;
            let mut reset_gpending = true;
            let mut changed = true;
            match c.unwrap() {
                Key::Char('q') => { // quit
                    break;
                }
                Key::Char('j') | Key::Down => { // down
                    self.cursor_down(cmp::max(acount, 1));
                },
                Key::Char('k') | Key::Up => { // up
                    self.cursor_up(cmp::max(acount, 1));
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
                    if gpending {
                        self.start = 0;
                        self.hover = 0;
                    } else {
                        gpending = true;
                        reset_gpending = false;
                    }
                },
                Key::Char(' ') => { // toggle selection
                    self.nodes[self.hover].selected ^= true;
                },
                Key::Char('e') | Key::Char('\n') => { // edit
                    util::edit(&self.conn, self.nodes[self.hover].id).unwrap();
                    write!(screen, "{}", termion::clear::All).unwrap();
                },
                Key::Char(c) if c.is_digit(10) => { // number for action count
                    acount = acount.saturating_mul(10);
                    acount = acount.saturating_add(c.to_digit(10).unwrap() as usize);
                    reset_acount = false;
                },
                Key::Char('/') => { // search
                    // TODO: reset pattern on search?
                    // center search mode
                    self.run_search(screen, keys);
                },
                Key::Char('a') => { // archive
                    self.archive();
                },
                Key::Char('d') | Key::Delete => {
                    self.run_delete(screen, keys);
                },
                Key::Char('u') => {
                    self.termsize = util::terminal_size();
                    self.reload_nodes();
                }
                // TODO:
                // - page down/up
                // - somehow show tags/some meta field (already in preview?)
                //   should be configurable
                //   additionally? edit/show meta file
                // - should a/r be applied to all selected? or to the currently
                //   hovered? maybe like in ncmpcpp? (selected? selected : hovered)
                // - allow to open/show multiple at once?
                //   maybe allow to edit/show selected?
                // - less-like status bar or something?
                // - "u": undo?
                _ => changed = false,
            }

            if reset_gpending {
                gpending = false;
            }

            if reset_acount {
                acount = 0;
            }

            // re-render whole screen
            if changed {
                self.write_nodes(screen);
                screen.flush().unwrap();
            }
        }
    }

    fn render_search<W: Write>(&self, screen: &mut W) {
        write!(screen, "{}{}{}{}/{}",
            termion::cursor::Goto(1, self.termy()),
            termion::color::Fg(termion::color::Reset),
            termion::color::Bg(termion::color::Reset),
            termion::clear::CurrentLine,
            self.args.pattern).unwrap();
    }

    pub fn run_search<R: Read, W: Write>(&mut self, screen: &mut W, keys: &mut Keys<R>) {
        self.render_search(screen);
        screen.flush().unwrap();

        for c in keys {
            let mut changed = true;
            let mut end = false;

            // TODO: cursor
            // maybe general utility for line input?
            match c.unwrap() {
                Key::Esc | Key::Ctrl('c') | Key::Ctrl('d') => {
                    end = true;
                    self.args.pattern.clear();
                },
                Key::Char('\n') => {
                    end = true;
                    changed = false;
                },
                Key::Backspace => {
                    if self.args.pattern.pop().is_none() {
                        end = true;
                        changed = false;
                    }
                },
                Key::Char(c) => {
                    self.args.pattern.push(c);
                },
                _ => changed = false,
            }

            if changed {
                // TODO: we could theoretically track them/jump to
                // nearest node
                self.hover = 0;
                self.start = 0;

                write!(screen, "{}", termion::clear::All).unwrap();
                self.reload_nodes();
                self.write_nodes(screen);
                self.render_search(screen);
                screen.flush().unwrap();
            }

            if end {
                write!(screen, "{}", termion::clear::All).unwrap();
                break;
            }
        }
    }

    pub fn run_delete<R: Read, W: Write>(&mut self, screen: &mut W, keys: &mut Keys<R>) {
        // TODO: could be done more efficiently if we keep track
        // of selected nodes in a `Vec<u32> selected`...
        let mut delete_hover = false;
        let mut nodes_description = "selected nodes".to_string();
        let mut selected: Vec<u32> = self.nodes.iter()
            .filter(|node| node.selected)
            .map(|node| node.id)
            .collect();
        if selected.is_empty() {
            let id = self.nodes[self.hover].id;
            nodes_description = format!("node {}", id);
            selected = vec!(id);
            delete_hover = true;
        }

        // render delete confirmation
        write!(screen, "{}{}{}{}Delete {}? [y/n]",
            termion::cursor::Goto(1, self.termy()),
            termion::color::Fg(termion::color::LightRed),
            termion::color::Bg(termion::color::Reset),
            termion::clear::CurrentLine,
            nodes_description).unwrap();
        screen.flush().unwrap();

        for c in keys {
            match c.unwrap() {
                Key::Char('n') |
                    Key::Char('N') |
                    Key::Esc |
                    Key::Ctrl('d') |
                    Key::Ctrl('c') => {
                        break;
                },
                Key::Char('y') | Key::Char('Y') => {
                    util::delete_range(&self.conn, &selected).unwrap();
                    if delete_hover {
                        self.nodes.remove(self.hover);
                    } else {
                        self.nodes.retain(|node| !node.selected);
                    }
                    break;
                },
                _ => (),
            }
        }
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

    let mut s = SelectScreen::new(&conn, &args);

    {
        let ascreen = AlternateScreen::from(raw);
        let mut screen = BufWriter::new(ascreen);
        if let Err(err) = write!(screen, "{}", termion::cursor::Hide) {
            println!("Failed to hide cursor in selection screen: {}", err);
            return -3;
        }

        // run interactive select/edit screen
        s.run_normal(&mut screen, &mut stdin.keys());

        // show cursor again
        write!(screen, "{}", termion::cursor::Show).unwrap();
    }

    // output selected nodes
    for node in s.nodes {
        if node.selected {
            println!("{}", node.id);
        }
    }

    0
}
