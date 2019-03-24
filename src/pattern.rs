// use regex::Regex;
//
// pub struct Node<T> {
//     pub children: Vec<Node<T>>,
//     pub data: T,
// }
//
// impl<T> Node<T> {
//     pub fn new(data: T) -> Node<T> {
//         Node {
//             children: Vec::new(),
//             data
//         }
//     }
//
//     pub fn add_child(&mut self, data: T) -> usize {
//         self.children.push(Node::new(data));
//         self.children.len() - 1
//     }
// }
//
// pub enum MatchString {
//     Match(Regex),
//     String(String),
// }
//
// pub enum CondType {
//     Exists,
//     Type(String),
//     Equals(String),
//     Matches(Vec<MatchString>),
//     Smaller(String),
//     Greater(String)
// }
//
// pub struct Cond {
//     pub entry: String,
//     pub cond_type: CondType,
// }
//
// pub enum Pattern {
//     String(String),
//     Regex(Regex),
// }
//
// pub enum CondNodeType {
//     Not, // 1 child
//     And, // n children
//     Or, // n children
//     ContentMatch(Pattern),
//     TagMatch(Pattern),
// }
//
// type CondNode = tree::Node<CondNodeType>;
