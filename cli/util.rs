use std::io;
use std::io::prelude::*;
use std::process;
use std::error;
use std::fmt;

use clap::{values_t, value_t};

use rusqlite::{Connection, ToSql};
use tempfile::NamedTempFile;

#[derive(PartialEq)]
pub enum Order {
    Asc,
    Desc
}

impl Order {
    pub fn name(&self) -> &'static str {
        match self {
            Order::Asc => "ASC",
            Order::Desc => "DESC",
        }
    }
}

#[derive(Debug)]
pub enum Error {
    SQL(rusqlite::Error), // sql operation failed unexpectedly
    IO(io::Error), // io operation failed unexpectedly
    InvalidNode(u32), // node with id doesn't exist
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::SQL(err) => write!(f, "SQL Error: {}", err),
            Error::IO(err) => write!(f, "IO Error: {}", err),
            Error::InvalidNode(id) => write!(f, "Invalid node id {}", id)
        }
    }
}

impl error::Error for Error {
    fn description(&self) -> &str {
        match self {
            Error::SQL(err) => err.description(),
            Error::IO(err) => err.description(),
            Error::InvalidNode(_) => "The given node id was invalid"
        }
    }

    fn cause(&self) -> Option<&error::Error> {
        match self {
            Error::SQL(err) => Some(err),
            Error::IO(err) => Some(err),
            Error::InvalidNode(_) => None
        }
    }
}

impl From<rusqlite::Error> for Error {
    fn from(err: rusqlite::Error) -> Self {
        Error::SQL(err)
    }
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Error::IO(err)
    }
}

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
pub fn terminal_size() -> (u16, u16) {
    // problem: when stdin isn't /dev/tty
    // let tty = fs::File::open("/dev/tty").unwrap();
    // TODO: https://github.com/redox-os/termion/blob/master/src/sys/unix/size.rs
    match termion::terminal_size() {
        Ok((x,y)) => (x,y),
        _ => (80, 80) // guess
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

// Gathers the given nodes ids either via the given argument name
// or via stdin.
pub fn gather_nodes(args: &clap::ArgMatches, argname: &str) -> Vec<u32> {
    let mut nodes = Vec::new();
    operate_ids_stdin(&args, argname, |id| nodes.push(id));
    nodes
}

pub struct Node<'a> {
    pub id: u32,
    pub content: &'a str,
}

pub struct ListArgs {
    pub preorder: Order,
    pub postorder: Order,
    pub count: Option<usize>,
    pub pattern: String, // empty for no pattern
    pub archived: Option<bool>,
}

// default order (reverse = false) is ascending for both
// preorder: the order of nodes (by id) before limiting/counting
// postorder: the order of nodes after limiting, i.e. the returned order
//   different pre-/postorders are only relevent if `count` is given.
// count: the maximum number of nodes to retrieve. If not given, iterate all
// pattern: optional pattern; only nodes matching this pattern will be returned
// archived: if not none, will only retrieve matching nodes
pub fn iter_nodes<F: FnMut(&Node)>(conn: &Connection,
        args: &ListArgs, mut op: F) {

    let mut qwhere = String::new();
    let mut join = String::new();
    let mut where_add = "WHERE";

    if let Some(archived) = args.archived {
        qwhere = format!("{} {} (archived = {}) ", qwhere, where_add, archived);
        where_add = "AND";
    }

    let mut qlimit = String::new();
    if !args.pattern.is_empty() {
        // escape for sql
        let pattern = args.pattern.to_string().replace("'", "''");
        join = "LEFT JOIN tags ON nodes.id = tags.node".to_string();
        qwhere = format!("{} {}
            (content LIKE '%{p}%' OR tag LIKE '%{p}%')",
            qwhere, where_add, p = pattern);
        where_add = "AND";
    }

    if let Some(count) = args.count {
        qlimit = format!("LIMIT {}", count);
    }

    let mut query = format!("
        SELECT DISTINCT id, content
        FROM nodes
        {join}
        {where}
        ORDER BY id {order}
        {limit}",
        join = join,
        where = qwhere,
        limit = qlimit,
        order = args.preorder.name());

    if args.preorder != args.postorder {
        query = format!("
            SELECT *
            FROM ({query})
            ORDER BY id {order}",
            query = query, order = args.postorder.name());
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

pub fn extract_list_args<'a>(args: &'a clap::ArgMatches, mut reverse: bool,
            mut reverse_display: bool) -> ListArgs {
    reverse ^= args.is_present("reverse");
    reverse_display ^= args.is_present("reverse_display");

    let limit = if args.is_present("num") {
        Some(value_t!(args, "num", usize).unwrap_or_else(|e| e.exit()))
    } else {
        None
    };

    let archived = if args.is_present("only_archived") {
        Some(true)
    } else if args.is_present("archived") {
        None
    } else {
        Some(false)
    };

    ListArgs {
        preorder: if reverse { Order::Desc } else { Order::Asc },
        postorder: if reverse_display { Order::Desc } else { Order::Asc },
        pattern: args.value_of("pattern").unwrap_or("").to_string(),
        count: limit,
        archived: archived,
    }
}

/// Edits the node with the given id
pub fn edit(conn: &Connection, id: u32) -> Result<(), Error> {
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
            return Err(Error::InvalidNode(id));
        }

        return Err(e.into());
    }

    // TODO: use programs from config instead of hardcoding nvim...
    // run editor on tmp file
    let prog = vec!("nvim", &file.path().to_str().unwrap());
    process::Command::new(&prog[0]).args(prog[1..].iter())
        .stdout(termion::get_tty().unwrap())
        .stderr(termion::get_tty().unwrap())
        .status()?;

    // write back
    let mut content = String::new();
    file.into_file().read_to_string(&mut content)?;

    // update content, set last seen and edited
    let query = "
        UPDATE nodes
        SET content = ?1,
            edited = CURRENT_TIMESTAMP,
            viewed = CURRENT_TIMESTAMP
        WHERE id = ?2";
    conn.execute(query, &[&content, &id as &ToSql])?;
    Ok(())
}

pub fn set_archived(conn: &Connection, id: u32, set: bool) -> Result<(), Error> {
    let query = "
        UPDATE nodes
        SET archived = ?1
        WHERE id = ?2";
    conn.execute(query, &[&set, &id as &ToSql])?;
    Ok(())
}

// returns sql `in (ids,...)` string for the given ids
// must be called with at least one value
pub fn in_string(ids: &[u32]) -> String {
    let mut qin = "IN (".to_string();
    let mut first = true;
    for id in ids {
        if !first {
            qin += ",";
        }
        qin += &id.to_string();
        first = false;
    }

    qin += ")";
    qin
}

// TODO: check for invalid ids
// for all commands below
pub fn toggle_archived(conn: &Connection, id: u32) -> Result<(), Error> {
    let query = "
        UPDATE nodes
        SET archived = NOT archived
        WHERE id = ?";
    conn.execute(query, &[&id])?;
    Ok(())
}

pub fn toggle_archived_range(conn: &Connection, ids: &[u32]) -> Result<(), Error> {
    let query = "
        UPDATE nodes
        SET archived = NOT archived
        WHERE id ".to_string() + &in_string(ids);
    conn.execute(&query, rusqlite::NO_PARAMS)?;
    Ok(())
}

// Returns the number of nodes deleted
pub fn delete_range(conn: &Connection, ids: &[u32]) -> Result<usize, Error> {
    if ids.len() == 0 {
        return Ok(0);
    }

    let query = "
        DELETE FROM nodes
        WHERE id ".to_string() + &in_string(ids);
    Ok(conn.execute(&query, rusqlite::NO_PARAMS)?)
}

pub fn delete(conn: &Connection, id: u32) -> Result<(), Error> {
    let query = "
        DELETE FROM nodes
        WHERE id = ?";
    conn.execute(query, &[&id])?;
    Ok(())
}

pub fn add_tag(conn: &Connection, ids: &[u32], tag: &str) -> Result<(), Error> {
    let mut query = "INSERT INTO tags(node, tag) VALUES ".to_string();
    let mut comma = "";
    let rtag = tag.replace("'", "''");
    for id in ids {
        query += &format!("{}({}, '{}')", comma, id, rtag);
        comma = ", ";
    }

    conn.execute(&query, rusqlite::NO_PARAMS)?;
    Ok(())
}
