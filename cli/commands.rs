use super::util;

use std::io::prelude::*;
use std::io;
use std::process;

use rusqlite::{Connection, ToSql};
use clap::value_t;
use tempfile::NamedTempFile;

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
    let limit = value_t!(args, "num", u32).unwrap_or(0xFFFFFFFFu32);

    // order
    let mut preorder = "DESC";
    if args.is_present("reverse") {
        preorder = "ASC";
    }

    let mut postorder = "ASC";
    if args.is_present("reverse_display") {
        postorder = "DESC";
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
    let width = util::terminal_width() as usize - 8; // tab width
    while let Some(row) = rows.next().unwrap() {
        let id: u32 = row.get(0).unwrap();
        let c = row.get_raw(1).as_str().unwrap();

        let summary = util::node_summary(&c, lines as usize, width);
        if lines == 1 {
            println!("{}:\t{}", id, summary)
        } else {
            println!("{}:\t{}", id, summary);
        }
    }

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

    0
}

pub fn edit(conn: &Connection, args: &clap::ArgMatches) -> i32 {
    let id = value_t!(args, "id", u32).unwrap_or_else(|e| e.exit());

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
            return -1;
        }

        println!("{}", e);
        return -2;
    }

    // run editor on tmp file
    let prog = vec!("nvim", &file.path().to_str().unwrap());
    let r = process::Command::new(&prog[0]).args(prog[1..].iter()).status();
    if let Err(err) = r {
        println!("Failed to spawn editor: {}", err);
        return -3;
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

    0
}
