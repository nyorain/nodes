use std::io;
use std::io::prelude::*;
use clap::values_t;

/// Trims the given string to the length max_length.
/// The last three chars will be "..." if the string was longer
/// than max_length.
pub fn short_string(lstr: &str, max_length: usize) -> String {
    let mut too_long = false;
    let mut s = String::new();
    let mut append = String::new();

    // TODO: can probably be done more efficiently?
    for (i, c) in lstr.chars().enumerate() {
        if i == max_length {
            too_long = true;
            break;
        } else if i >= max_length - 3 {
            append.push(c);
        } else {
            s.push(c);
        }
    }

    s.push_str(if too_long { "..." } else { append.as_str() });
    s
}

/// Returns a preview of a node contents.
/// - node: the nodes contents (only works for text)
/// - lines: the number of lines the preview should have. Should be >0
/// - width: the number of characters the preview can have at max
pub fn node_summary(node: &str, mut lines: usize, width: usize) -> String {
    let multiline = lines > 1;
    let mut ret = String::new();
    for line in node.lines() {
        if lines == 0 {
            if multiline {
                ret.push_str("[...]\n");
            }
            break;
        }

        ret.push_str(&short_string(&line, width));
        if multiline {
            ret.push_str("\n\t");
        }

        lines -= 1;
    }

    ret
}

/// Returns the current width of the terminal in characters.
pub fn terminal_width() -> u16 {
    match termion::terminal_size() {
        Ok((x,_)) => x,
        _ => 80 // guess
    }
}

/// Applies op to all input node ids.
/// If args contains argname, will interpret it as ids.
/// Otherwise will read from stdin.
pub fn operate_ids_stdin<F: FnMut(u32)>(
        args: &clap::ArgMatches, argname: &str, mut op: F) -> i32 {
    if args.is_present(argname) {
        let ids = values_t!(args, argname, u32).unwrap_or_else(|e| e.exit());
        for id in ids {
            op(id);
        }
        0
    } else {
        let mut res = 0;
        let stdin = io::stdin();
        for rline in stdin.lock().lines() {
            let line = match rline {
                Err(err) => {
                    println!("Failed to read line: {}", err);
                    res += 1;
                    continue
                }, Ok(l) => l,
            };

            let id = match line.parse::<u32>() {
                Err(e) => {
                    println!("Invalid node '{}': {}", line, e);
                    res += 1;
                    continue;
                }, Ok(n) => n,
            };

            op(id);
        }

        res
    }
}
