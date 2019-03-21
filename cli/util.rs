use std::io;
use std::io::prelude::*;
use std::process;

use clap::{values_t, value_t};

use rusqlite::{Connection, ToSql};
use tempfile::NamedTempFile;

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

pub struct Node<'a> {
    pub id: u32,
    pub content: &'a str,
}

/// Iterates over all nodes (ordering, limit as specified via args)
/// and calls `op` with each node.
pub fn iter_nodes<F: FnMut(&Node)>(conn: &Connection, args: &clap::ArgMatches,
        mut reverse: bool, mut reverse_display: bool, mut op: F) {
    let limit = value_t!(args, "num", u32).unwrap_or(0xFFFFFFFFu32);

    // order
    reverse ^= args.is_present("reverse");
    let mut preorder = "DESC";
    if reverse {
        preorder = "ASC";
    }

    reverse_display ^= args.is_present("reverse_display");
    let mut postorder = "DESC";
    if reverse_display {
        postorder = "ASC";
    }

    // query
    let mut query = format!("
        SELECT id, content
        FROM nodes
        ORDER BY id {order}
        LIMIT {limit}",
        order=preorder, limit=limit);

    if preorder != postorder {
        query = format!("
            SELECT *
            FROM ({query})
            ORDER BY id {order}",
            query = query, order=postorder);
    }

    let mut stmt = conn.prepare_cached(&query).unwrap();
    let mut rows = stmt.query(rusqlite::NO_PARAMS).unwrap();
    while let Some(row) = rows.next().unwrap() {
        let n = Node {
            id: row.get_unwrap(0),
            content: row.get_raw(1).as_str().unwrap(),
        };
        op(&n);
    }
}

pub fn edit(conn: &Connection, id: u32) -> bool {
    // NOTE: maybe this all can be done more efficiently with a memory map?
    // copy node content into file
    let mut file = NamedTempFile::new().unwrap();
    let r = conn.query_row(
        "SELECT content FROM nodes WHERE id = ?1", &[id],
        |row| {
            file.write(&row.get_raw(0).as_str().unwrap().as_bytes()).unwrap();
            file.seek(io::SeekFrom::Start(0)).unwrap();
            Ok(())
        }
    );

    if let Err(e) = r {
        if e == rusqlite::Error::QueryReturnedNoRows {
            println!("No such node: {}", id);
            return false;
        }

        println!("{}", e);
        return false;
    }

    // run editor on tmp file
    let prog = vec!("nvim", &file.path().to_str().unwrap());
    let r = process::Command::new(&prog[0]).args(prog[1..].iter()).status();
    if let Err(err) = r {
        println!("Failed to spawn editor: {}", err);
        return false;
    }

    // write back
    let mut content = String::new();
    file.into_file().read_to_string(&mut content).unwrap();

    // update content, set last seen and edited
    let query = "
        UPDATE nodes
        SET content = ?1,
            edited = CURRENT_TIMESTAMP,
            viewed = CURRENT_TIMESTAMP
        WHERE id = ?2";
    conn.execute(query, &[&content, &id as &ToSql]).unwrap();
    true
}
