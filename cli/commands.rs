use super::util;

use std::io::prelude::*;
use std::process;
use tempfile::NamedTempFile;

use rusqlite::Connection;
use clap::value_t;

pub fn rm(conn: &Connection, args: &clap::ArgMatches) -> i32 {
    let mut query: String = "
        DELETE FROM nodes
        WHERE id IN (".to_string();
    let mut count = 0;
    let errors = util::operate_ids_stdin(args, "id", |id| {
        if count != 0 {
            query += ",";
        }

        query += &id.to_string();
        count += 1;
    });

    if count == 0 {
        println!("No valid ids given");
        return -1;
    }

    query += ")";
    let res = conn.execute(&query, rusqlite::NO_PARAMS).unwrap();
    (count - res) as i32 + errors
}

pub fn ls(conn: &Connection, args: &clap::ArgMatches) -> i32 {
    // number of lines to output as node preview
    let mut lines = value_t!(args, "lines", u32).unwrap_or(1);
    if args.is_present("full") {
        lines = 0xFFFFFFFFu32;
    }

    // number of nodes to show
    let width = util::terminal_size().0 as usize;
    util::iter_nodes_args(&conn, &args, false, true, |node| {
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
    let mut content = String::new();
    if let Some(fcontent) = args.value_of("content") {
        content = fcontent.to_string();
    } else {
        let file = NamedTempFile::new().unwrap();
        let path = file.path();
        let prog = vec!("nvim", &path.to_str().unwrap());
        let r = process::Command::new(&prog[0]).args(prog[1..].iter()).status();
        if let Err(err) = r {
            println!("Failed to spawn editor: {}", err);
            return -2;
        }

        file.into_file().read_to_string(&mut content).unwrap();
    }

    if content.is_empty() {
        println!("No content given, no node created");
        return -1;
    }

    let query = "
        INSERT INTO nodes(content)
        VALUES (?1)";
    conn.execute(query, &[content]).unwrap();

    // output id
    println!("{}", conn.last_insert_rowid());
    0
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
    if util::edit(&conn, id) { 0 } else { -1 }
}
