use super::util;

use std::cmp;
use std::io;
use std::io::prelude::*;
use std::io::BufWriter;

use termion::event::Key;
use termion::screen::*;
use termion::input::TermRead;
use termion::raw::IntoRawMode;

use rusqlite::Connection;

struct SelectNode {
    id: u32,
    summary: String,
    selected: bool,
}

fn write_select_list<W: Write>(screen: &mut W, nodes: &Vec<SelectNode>,
        start: usize, current: usize, starty: u16, maxx: u16, maxy: u16) {
    let bg_current = termion::color::Bg(termion::color::LightGreen);
    let fg_selected = termion::color::Fg(termion::color::LightRed);

    let x = 1;
    let mut y = starty;
    let mut i = start;
    for node in nodes[start..].iter() {
        if y > maxy {
            break;
        }

        if i == current {
            write!(screen, "{}", bg_current).unwrap();
        }

        if node.selected {
            write!(screen, "{}", fg_selected).unwrap();
        }

        let idstr = node.id.to_string();
        write!(screen, "{}{}: {:<w$}{}{}",
            termion::cursor::Goto(x, y),
            idstr, node.summary,
            termion::color::Bg(termion::color::Reset),
            termion::color::Fg(termion::color::Reset),
            w = (maxx as usize) - idstr.len() - 2).unwrap();

        y += 1;
        i += 1;
    }
}


pub fn select(conn: &Connection, args: &clap::ArgMatches) -> i32 {
    // problem: when stdin isn't /dev/tty
    // let tty = fs::File::open("/dev/tty").unwrap();
    // TODO: https://github.com/redox-os/termion/blob/master/src/sys/unix/size.rs
    let (maxx, maxy) = match termion::terminal_size() {
        Ok((x,y)) => (x,y),
        _ => (80, 100) // guess
    };

    let mut nodes: Vec<SelectNode> = Vec::new();
    util::iter_nodes(&conn, &args, false, false, |node| {
        let summary = util::node_summary(&node.content, 1, (maxx - 8) as usize);
        nodes.push(SelectNode{
            id: node.id,
            summary: summary,
            selected: false
        });
    });

    // setup terminal
    {
        let mut start: usize = 0; // start index in node vec
        let mut current: usize = 0; // current/focused index in node vec
        let mut gpending = false;

        let stdin = io::stdin();
        let raw = match termion::get_tty().and_then(|tty| tty.into_raw_mode()) {
            Ok(r) => r,
            Err(err) => {
                println!("Failed to transform tty into raw mode: {}", err);
                return -2;
            }
        };

        let ascreen = AlternateScreen::from(raw);
        let mut screen = BufWriter::new(ascreen);
        if let Err(err) = write!(screen, "{}", termion::cursor::Hide) {
            println!("Failed to hide cursor in selection screen: {}", err);
            return -3;
        }

        let mut acount: usize = 0; // action count
        write_select_list(&mut screen, &nodes, start, current, 1, maxx, maxy);
        screen.flush().unwrap();

        for c in stdin.keys() {
            let mut reset_acount = true;
            let mut reset_gpending = true;
            match c.unwrap() {
                Key::Char('q') => {
                    break;
                }
                Key::Char('j') => {
                    acount = cmp::max(acount, 1);
                    current = cmp::min(nodes.len() - 1, current + acount);
                    if current - start >= (maxy as usize) {
                        start += current - start - (maxy as usize);
                    }
                },
                Key::Char('G') => {
                    current = nodes.len() - 1;
                    start = cmp::max((current as i32) - (maxy as i32), 0) as usize;
                },
                Key::Char('g') => {
                    if gpending {
                        start = 0;
                        current = 0;
                    } else {
                        gpending = true;
                        reset_gpending = false;
                    }
                },
                Key::Char('k') => {
                    acount = cmp::max(acount, 1);
                    current = current.saturating_sub(acount);
                    if current < start {
                        start = current;
                    }
                },
                Key::Char('\n') => {
                    nodes[current].selected ^= true;
                },
                Key::Char('e') => { // edit
                    util::edit(&conn, nodes[current].id);
                    write!(screen, "{}", termion::clear::All).unwrap();
                },
                Key::Char(c) if c.is_digit(10) => {
                    acount = acount.saturating_mul(10);
                    acount = acount.saturating_add(c.to_digit(10).unwrap() as usize);
                    reset_acount = false;
                },
                // TODO:
                // - use numbers for navigation
                // - a: archive
                // - r: remove (with confirmation?)
                // - somehow show tags/some meta field (already in preview?)
                //   should be configurable
                //   additionally? edit/show meta file
                // - should a/r be applied to all selected? or to the currently
                //   hovered? maybe like in ncmpcpp? (selected? selected : hovered)
                // - allow to open/show multiple at once?
                //   maybe allow to edit/show selected?
                // - less-like status bar or something?
                _ => (),
            }

            if reset_gpending {
                gpending = false;
            }

            if reset_acount {
                acount = 0;
            }

            // re-render whole screen
            write_select_list(&mut screen, &nodes, start, current, 1, maxx, maxy);
            screen.flush().unwrap();
        }

        write!(screen, "{}", termion::cursor::Show).unwrap();
    }

    for node in nodes {
        if node.selected {
            println!("{}", node.id);
        }
    }

    0
}
