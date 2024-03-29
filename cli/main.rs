use rusqlite::Connection;
use clap::clap_app;
use nodes::Config;

mod commands;
mod util;
mod select;

fn is_uint(v: String) -> Result<(), String> {
    if let Err(_) = v.parse::<u64>() {
        Err(format!("Could not parse '{}' as unsigned number", v))
    } else {
        Ok(())
    }
}

fn is_node(v: String) -> Result<(), String> {
    // TODO: re-add handling of those
    // would require new table though probably
    /*
    if v == "le" || v == "lc" || v == "lv" || v == "l" {
        return Ok(());
    }
    */

    is_uint(v)
}

fn main() -> rusqlite::Result<()> {
    // TODO:
    // - archived
    let matches = clap_app!(nodes =>
        (version: "0.1")
        (setting: clap::AppSettings::VersionlessSubcommands)
        (author: "nyorain [at gmail dot com]")
        (about: "Manages your node system from the command line")
        (@arg storage: -s --storage +takes_value "The storage to use")
        (@subcommand create =>
            (about: "Creates a new node")
            (alias: "c")
            (@arg tags: -t --tag +takes_value !required ... +use_delimiter
                "Tag the node")
            (@arg content: -c --content +takes_value !required
                "Write this content into the node instead of open an editor")
        ) (@subcommand rm =>
            (about: "Removes a node (by id)")
            (@arg id: +multiple index(1) {is_node}
                "The node ids. Can also specify multiple nodes. \
                If not given, will read from stdin")
        ) (@subcommand select =>
            (about: "Select a list of nodes, ids will be printed to stdout")
            (alias: "s")
            (@arg pattern: index(1)
                "Only list nodes matching this pattern")
            (@arg num: -n --num +takes_value
                default_value("999999")
                {is_uint}
                "Maximum number of nodes to show")
            (@arg archived: -a !takes_value !required
                "Show only archived nodes")
            (@arg only_archived: -A !takes_value !required
                "Only show archived nodes")
            (@arg reverse: -r --rev !takes_value !required
                "Reverses the node/display order. Default is ascending")
            (@arg sort: -s --sort +takes_value !required
                "How to initially sort the nodes: id | priority | edited")
        ) (@subcommand ls =>
            (about: "Lists existing notes")
            (@arg pattern: index(1)
                "Only list nodes matching this pattern")
            (@arg num: -n --num +takes_value
                default_value("10")
                {is_uint}
                "Maximum number of nodes to show")
            (@arg lines: -l --lines +takes_value
                {is_uint}
                "How many lines to show at maximum from a node")
            (@arg full: -f --full conflicts_with("lines") "Print full nodes")
            (@arg reverse: -R --rev !takes_value !required
                "Reverses the node order (before counting). Default is descending")
            (@arg reverse_display: -r --revdisplay !takes_value !required
                "Reverses the display order. Default is ascending")
            (@arg archived: -a !takes_value !required
                "Include archived nodes")
            (@arg only_archived: -A !takes_value !required
                "Only show archived nodes")
            (@arg sort: -s --sort +takes_value !required
                "How to sort the nodes: id | priority | edited")
        ) (@subcommand output =>
            (about: "Output the content of a node")
            (alias: "o")
            (@arg id: +required index(1) {is_node} "Id of node to show")
        ) (@subcommand edit =>
            (about: "Edits a node")
            (alias: "e")
            (@arg id: --id +required index(1) {is_node} "Id of node to edit")
        ) (@subcommand addtag =>
            (about: "Adds a tag to a node")
            (alias: "at")
            (@arg tag: +required index(1) "The tag to add")
            (@arg id: +multiple index(2) {is_node}
                "The node ids. Can also specify multiple nodes. \
                If not given, will read from stdin")
        ) (@subcommand rmtag =>
            (about: "Adds a tag to a node")
            (alias: "rt")
            (@arg tag: +required index(1) "The tag to remove")
            (@arg id: +multiple index(2) {is_node}
                "The node ids. Can also specify multiple nodes. \
                If not given, will read from stdin")
        ) (@subcommand archive =>
           (about: "Toggles the archived state of a node")
           (alias: "a")
           (@arg id: +multiple index(1) {is_node}
                "The node ids. Can also specify multiple nodes. \
                If not given, will read from stdin")
        )
    ).get_matches();

    let config = Config::load_default().expect("Error loading config");
    let mut storage_path = match matches.value_of("storage") {
        Some(name) => match config.storage_folder(name) {
            Some(path) => path.clone(),
            None => {
                println!("Storage '{}' unknown", name);
                std::process::exit(1);
            }
        }, None => config.default_storage_folder().clone(),
    };
    storage_path.push("nodes.db");

    let conn: rusqlite::Connection = Connection::open(storage_path)?;
    // XXX: this may not be desired by all users, make it configurable
    // drastically improves performance, especially on hdds
    // e.g. creation time goes down from "about a seond" to
    // "feels like immediately" on my old hdd.
    // no noticable performance difference when nodes.db is stored
    // on an ssd or ramdisk
    conn.pragma_update(None, "SYNCHRONOUS", &0).unwrap();

    // TODO: if database is empty, create tables
    // maybe only check whether or not file already exists?
    // and how to upgrade to a new schema? store version?

    let r = match matches.subcommand() {
        ("rm", Some(s)) => commands::rm(&conn, s),
        ("edit", Some(s)) => commands::edit(&conn, s),
        ("create", Some(s)) => commands::create(&conn, s),
        ("ls", Some(s)) => commands::ls(&conn, s),
        ("select", Some(s)) => select::select(&conn, s),
        ("output", Some(s)) => commands::output(&conn, s),
        ("addtag", Some(s)) => commands::add_tag(&conn, s),
        ("rmtag", Some(s)) => commands::remove_tag(&conn, s),
        ("archive", Some(s)) => commands::archive(&conn, s),
        _ => select::select(&conn, &clap::ArgMatches::default())
    };

    std::process::exit(r);
}
