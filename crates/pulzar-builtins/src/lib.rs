#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Builtin {
    Map,
    Filter,
    Lines,
    Cat,
    Pwd,
}

pub fn lookup(name: &str) -> Option<Builtin> {
    match name {
        "map" => Some(Builtin::Map),
        "filter" => Some(Builtin::Filter),
        "lines" => Some(Builtin::Lines),
        "cat" => Some(Builtin::Cat),
        "pwd" => Some(Builtin::Pwd),
        _ => None,
    }
}
