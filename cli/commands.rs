use super::util;

use std::io::prelude::*;
use std::process;
use tempfile::NamedTempFile;

use rusqlite::Connection;
use clap::value_t;

pub fn rm(conn: &Connection, args: &clap::ArgMatches) -> i32 {
    let nodes = util::gather_nodes(&args, "id");
    if nodes.is_empty() {
        println!("No valid ids given");
        return -1;
    }

    match util::delete_range(&conn, &nodes) {
        Ok(num) => (nodes.len() - num) as i32,
        Err(err) => {
            eprintln!("{}", err);
            -2
        }
    }
}

pub fn ls(conn: &Connection, args: &clap::ArgMatches) -> i32 {
    // number of lines to output as node preview
    let mut lines = value_t!(args, "lines", u32).unwrap_or(1);
    if args.is_present("full") {
        lines = 0xFFFFFFFFu32;
    }

    // number of nodes to show
    let width = util::terminal_size().0 as usize;
    let args = util::extract_list_args(&args, true, false);
    util::iter_nodes(&conn, &args, |node| {
        let summary = util::node_summary(&node.content, lines as usize, width);
        if lines == 1 {
            println!("{}:\t{}", node.id, summary)
        } else {
            println!("{}:\t{}", node.id, summary);
        }
    });

    0
}

pub fn create(conn: &Connection, args: &clap::ArgMatches) -> i32 {
    match util::create(&conn, args.value_of("content")) {
        Ok(id) => {
            println!("{}", id);
            0
        }
        Err(err) => {
            eprintln!("{}", err);
            -2
        }
    }
}

pub fn output(conn: &Connection, args: &clap::ArgMatches) -> i32 {
    let id = value_t!(args, "id", u32).unwrap_or_else(|e| e.exit());
    let r = conn.query_row(
        "SELECT content FROM nodes WHERE id = ?1", &[id],
        |row| {
            println!("{}", &row.get_raw(0).as_str().unwrap());
            Ok(())
        }
    );

    if let Err(e) = r {
        if e == rusqlite::Error::QueryReturnedNoRows {
            println!("No such node: {}", id);
            return -1;
        }

        println!("{}", e);
        return -2;
    }

    // Strictly speaking we should use a transaction here, but it's
    // not really a problem in the end
    let query = "
        UPDATE nodes
        SET viewed = CURRENT_TIMESTAMP
        WHERE id = ?2";
    conn.execute(query, &[&id]).unwrap();

    0
}

pub fn edit(conn: &Connection, args: &clap::ArgMatches) -> i32 {
    let id = value_t!(args, "id", u32).unwrap_or_else(|e| e.exit());
    if let Err(e) = util::edit(&conn, id) {
        eprintln!("{}", e);
        return -6;
    }
    0
}

pub fn add_tag(conn: &Connection, args: &clap::ArgMatches) -> i32 {
    let tag = args.value_of("tag").unwrap();
    let nodes = util::gather_nodes(&args, "id");
    if nodes.is_empty() {
        println!("No valid ids given");
        return -1;
    }

    match util::add_tag(&conn, &nodes, &tag) {
        Ok(_) => 0,
        Err(err) => {
            eprintln!("{}", err);
            -2
        }
    }
}
