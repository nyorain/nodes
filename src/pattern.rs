use std::str;

// Simple recursive tree structure
pub struct Node<T> {
    pub children: Vec<Node<T>>,
    pub data: T,
}

impl<T> Node<T> {
    pub fn new(data: T) -> Node<T> {
        Node {
            children: Vec::new(),
            data
        }
    }

    pub fn add_child(&mut self, data: T) -> usize {
        self.children.push(Node::new(data));
        self.children.len() - 1
    }
}

// conditional node
pub enum CondNodeType {
    Not, // 1 child
    And, // n children
    Or, // n children
    Match(String),
    ContentMatch(String),
    Tag(String),
    TagMatch(String),
}

pub type CondNode = Node<CondNodeType>;

// to sql
pub fn tosql(pattern: &CondNode) -> String {
    let mut query = String::new();
    match &pattern.data {
        CondNodeType::Not => {
            query += "(NOT ";
            query += &tosql(&pattern.children[0]);
            query += ")";
        }, CondNodeType::And | CondNodeType::Or => {
            let mut sep = "";
            query += "(";
            for c in &pattern.children {
                query += sep;
                query += &tosql(c);

                sep = if let CondNodeType::And = pattern.data {
                    " AND "
                } else {
                    " OR "
                }
            }
            query += ")";
        }, CondNodeType::ContentMatch(string) => {
            let escaped = string.replace("'", "''");
            query += "(content LIKE '%";
            query += &escaped;
            query += "%')";
        }, CondNodeType::Tag(string) => {
            let escaped = string.replace("'", "''");
            query += &format!("(EXISTS(SELECT 1 FROM tags WHERE
                node LIKE nodes.id AND tag = '{}'))",
                &escaped);
        }, CondNodeType::TagMatch(string) => {
            let escaped = string.replace("'", "''");
            query += &format!("(EXISTS(SELECT 1 FROM tags WHERE
                node LIKE nodes.id AND tag LIKE '%{}%'))",
                &escaped);
        }, CondNodeType::Match(string) => {
            let escaped = string.replace("'", "''");
            query += &format!("(content LIKE '%{0}%' OR
                EXISTS(SELECT 1 FROM tags WHERE
                node LIKE nodes.id AND tag LIKE '%{0}%'))",
                &escaped);
        }
    }

    query
}

use nom::types::CompleteStr as Input;

// parser
named!(value_string_unesc<Input, Input>, is_not!("|&()[]<>/"));
named!(value_string_esc<Input, Input>,
    delimited!(tag!("\""), is_not!("\""), tag!("\"")));
named!(value_string<Input, Input>,
    alt_complete!(value_string_esc | value_string_unesc));

named!(atom<Input, CondNode>, ws!(alt_complete!(
    // contains full tag
    map!(delimited!(
            tag!("["),
            is_not!("]"),
            tag!("]")),
        |value| CondNode {
            children: Vec::new(),
            data: CondNodeType::Tag(value.to_string()),
    }) |
    map!(preceded!(
            tag!("t"),
            delimited!(
                tag!("("),
                is_not!(")"),
                tag!(")"))),
        |value| CondNode {
            children: Vec::new(),
            data: CondNodeType::Tag(value.to_string()),
    }) |
    // containts a tag that matches string
    map!(delimited!(
            tag!("<"),
            is_not!(">"),
            tag!(">")),
        |value| CondNode {
            children: Vec::new(),
            data: CondNodeType::TagMatch(value.to_string()),
    }) |
    map!(preceded!(
            tag!("t"),
            delimited!(
                tag!("/"),
                is_not!("/"),
                tag!("/"))),
        |value| CondNode {
            children: Vec::new(),
            data: CondNodeType::TagMatch(value.to_string()),
    }) |
    // contains the given string
    map!(preceded!(
            tag!("c"),
            delimited!(
                tag!("("),
                is_not!(")"),
                tag!(")"))),
        |value| CondNode {
            children: Vec::new(),
            data: CondNodeType::ContentMatch(value.to_string()),
    }) |
    // tag or content matches string
    map!(value_string,
         |value| CondNode {
             children: Vec::new(),
             data: CondNodeType::Match(value.to_string()),
    })
)));

named!(expr<Input, CondNode>, alt_complete!(
    ws!(delimited!(tag!("("), or, tag!(")"))) |
    atom
));

named!(not<Input, CondNode>, alt_complete!(ws!(map!(
        preceded!(tag!("!"), expr),
        |expr| CondNode {
            children: vec!(expr),
            data: CondNodeType::Not
        })) |
    expr));

named!(and<Input, CondNode>, ws!(map!(
    separated_nonempty_list_complete!(tag!("&"), not),
    |mut children| {
        if children.len() == 1 {
            children.pop().unwrap()
        } else {
            CondNode {
                children,
                data: CondNodeType::And,
            }
        }
    }
)));

named!(or<Input, CondNode>, ws!(map!(
    separated_nonempty_list_complete!(tag!("|"), and),
    |mut children| {
        if children.len() == 1 {
            children.pop().unwrap()
        } else {
            CondNode {
                children,
                data: CondNodeType::Or,
            }
        }
    }
)));


pub fn parse_condition(spattern: &str) -> Result<CondNode, String> {
    let res = or(Input(&spattern));

    if res.is_err() {
        // nom::print_error(pattern.as_bytes(), res);
        return Err("Invalid pattern".to_string());
    }

    let (rest, value) = res.unwrap();
    if rest.len() > 0 {
        Err(format!("Unexpected character {}",
            rest.chars().next().unwrap()))
    } else {
        Ok(value)
    }
}

// TODO
#[cfg(test)]
mod test {
    use super::*;
}
