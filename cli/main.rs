use rusqlite::Connection;
use clap::clap_app;

mod commands;
mod util;

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
    let matches = clap_app!(nodes =>
        (version: "0.1")
        (setting: clap::AppSettings::VersionlessSubcommands)
        (author: "nyorain [at gmail dot com]")
        (about: "Manages your node system from the command line")
        (@arg storage: -s --storage +takes_value "The storage to use")
        (@arg local: -l --local
            conflicts_with("storage")
            "Search for a local node storage in current directory")
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
                "The nodes id. Can also specify multiple nodes.
                If not given, will read from stdin")
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
                "Show only archived nodes")
            (@arg debug_condition: -d !takes_value !required +hidden
                "Debug the condition tree")
        ) (@subcommand show =>
            (about: "Shows a node")
            (alias: "s")
            (@arg id: +required index(1) {is_node} "Id of node to show")
        ) (@subcommand edit =>
            (about: "Edits a node")
            (alias: "e")
            (@arg id: +required index(1) {is_node} "Id of node to edit")
        ) (@subcommand ref =>
           (@arg ref: +required index(1) "The node reference")
           (@arg from: index(2)
                "Origin node path. Needed for 'this' storage")
           (about: "Resolves a node reference to a path")
        ) (@subcommand select =>
            (about: "Select a list of nodes, ids will be printed to stdout")
            (@arg pattern: index(1)
                "Only list nodes matching this pattern")
            (@arg num: -n --num +takes_value
                default_value("999999")
                {is_uint}
                "Maximum number of nodes to show")
            (@arg archived: -a !takes_value !required
                "Show only archived nodes")
            (@arg reverse: -r --rev !takes_value !required
                "Reverses the node/display order. Default is ascending")
        )
    ).get_matches();

    let conn: rusqlite::Connection = Connection::open("nodes.db")?;
    // XXX: this may not be desired by all users, make it configurable
    // drastically improves performance, especially on hdds
    // e.g. creation time goes down from "about a seond" to
    // "feels like immediately" on my old hdd.
    // no noticable performance difference when nodes.db is stored
    // on an ssd or ramdisk
    conn.pragma_update(None, "SYNCHRONOUS", &0).unwrap();

    match matches.subcommand() {
        ("rm", Some(s)) => commands::rm(&conn, s),
        ("edit", Some(s)) => commands::edit(&conn, s),
        ("create", Some(s)) => commands::create(&conn, s),
        ("ls", Some(s)) => commands::ls(&conn, s),
        ("select", Some(s)) => commands::select(&conn, s),
        ("show", Some(s)) => commands::show(&conn, s),
        // TODO: default action when just a node id is given
        // e.g. should `nodes 234` should show/edit that node?
        _ => commands::select(&conn, &clap::ArgMatches::default())
    };

    Ok(())
}
