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

type CondNode = Node<CondNodeType>;

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
                    "AND "
                } else {
                    "OR "
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
            query += "(tag = '";
            query += &escaped;
            query += "')";
        }, CondNodeType::TagMatch(string) => {
            let escaped = string.replace("'", "''");
            query += "(tag LIKE '%";
            query += &escaped;
            query += "%')";
        }, CondNodeType::Match(string) => {
            let escaped = string.replace("'", "''");
            query += "(content LIKE '%";
            query += &escaped;
            query += "%' OR tag LIKE '%";
            query += &escaped;
            query += "%')";
        }
    }

    query
}

// parser
// TODO: allow escaped values again (e.g. in [x])?
named!(value_string_unesc, is_not!("|&()[]<>/"));
named!(value_string_esc,
    delimited!(tag!("\""), take_until!("\""), tag!("\"")));
named!(value_string<&str>, map_res!(
    alt_complete!(value_string_esc | value_string_unesc),
    str::from_utf8));

named!(atom<CondNode>, alt_complete!(
    // contains full tag
    map!(delimited!(
            tag!("["),
            map_res!(is_not!("]"), str::from_utf8),
            tag!("]")),
        |value| CondNode {
            children: Vec::new(),
            data: CondNodeType::Tag(value.to_string()),
    }) |
    map!(preceded!(
            tag!("t"),
            delimited!(
                tag!("("),
                map_res!(is_not!(")"), str::from_utf8),
                tag!(")"))),
        |value| CondNode {
            children: Vec::new(),
            data: CondNodeType::Tag(value.to_string()),
    }) |
    // containts a tag that matches string
    map!(delimited!(
            tag!("<"),
            map_res!(is_not!(">"), str::from_utf8),
            tag!(">")),
        |value| CondNode {
            children: Vec::new(),
            data: CondNodeType::TagMatch(value.to_string()),
    }) |
    map!(preceded!(
            tag!("t"),
            delimited!(
                tag!("/"),
                map_res!(is_not!("/"), str::from_utf8),
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
                map_res!(is_not!(")"), str::from_utf8),
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
));

named!(expr<CondNode>, alt_complete!(
    delimited!(tag!("("), and, tag!(")")) |
    atom
));

named!(not<CondNode>, alt_complete!(map!(
        preceded!(tag!("!"), expr),
        |expr| CondNode {
            children: vec!(expr),
            data: CondNodeType::Not
        }) |
    expr));

named!(or<CondNode>, map!(
    separated_nonempty_list_complete!(tag!("|"), not),
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
));

named!(and<CondNode>, map!(
    separated_nonempty_list_complete!(tag!("&"), or),
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
));

pub fn parse_condition(pattern: &str) -> Result<CondNode, String> {
    eprintln!("pattern: {}", pattern);
    let input = pattern.as_bytes();
    let res = and(input);

    if res.is_err() {
        nom::print_error(input, res);
        return Err("Invalid pattern".to_string());
    }

    let (rest, value) = res.unwrap();
    if rest.len() > 0 {
        // TODO: performance?
        let str = match str::from_utf8(rest) {
            Ok(a) => a,
            Err(_) => return Err("Invalid condition: non-utf8 \
                input sequence".to_string()),
        };
        Err(format!("Unexpected character {}",
            str.chars().next().unwrap()))
    } else {
        Ok(value)
    }
}

// TODO
#[cfg(test)]
mod test {
    use super::*;
}
