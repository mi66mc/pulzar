use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinId {
    Help,
    FsRead,
    FsPwd,
    FsLs,
    Cd,
    EnvGet,
    EnvSet,
    EnvUnset,
    EnvList,
    Map,
    Filter,
    Lines,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinStyle {
    CommandOnly,
    FunctionOnly,
    Both,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueKind {
    None,
    Any,
    String,
    List,
    Function,
    Object,
    Path,
    EnvName,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltinArgSpec {
    pub name: &'static str,
    pub kind: ValueKind,
    pub optional: bool,
    pub variadic: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltinSpec {
    pub id: BuiltinId,
    pub path: &'static [&'static str],
    pub aliases: &'static [&'static str],
    pub description: &'static str,
    pub style: BuiltinStyle,
    pub input: ValueKind,
    pub output: ValueKind,
    pub args: &'static [BuiltinArgSpec],
    pub mutates_shell: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct BuiltinMatch {
    pub spec: &'static BuiltinSpec,
    pub consumed: usize,
}

const NO_ARGS: &[BuiltinArgSpec] = &[];
const PATH_ARG: &[BuiltinArgSpec] = &[BuiltinArgSpec {
    name: "path",
    kind: ValueKind::Path,
    optional: false,
    variadic: false,
}];
const OPTIONAL_PATH_ARG: &[BuiltinArgSpec] = &[BuiltinArgSpec {
    name: "path",
    kind: ValueKind::Path,
    optional: true,
    variadic: false,
}];
const ENV_NAME_ARG: &[BuiltinArgSpec] = &[BuiltinArgSpec {
    name: "name",
    kind: ValueKind::EnvName,
    optional: false,
    variadic: false,
}];
const ENV_SET_ARGS: &[BuiltinArgSpec] = &[
    BuiltinArgSpec {
        name: "name",
        kind: ValueKind::EnvName,
        optional: false,
        variadic: false,
    },
    BuiltinArgSpec {
        name: "value",
        kind: ValueKind::Any,
        optional: false,
        variadic: false,
    },
];
const MAP_FILTER_ARGS: &[BuiltinArgSpec] = &[
    BuiltinArgSpec {
        name: "items",
        kind: ValueKind::List,
        optional: false,
        variadic: false,
    },
    BuiltinArgSpec {
        name: "callback",
        kind: ValueKind::Function,
        optional: false,
        variadic: false,
    },
];
const LINES_ARGS: &[BuiltinArgSpec] = &[BuiltinArgSpec {
    name: "text",
    kind: ValueKind::String,
    optional: false,
    variadic: false,
}];
const HELP_ARGS: &[BuiltinArgSpec] = &[BuiltinArgSpec {
    name: "builtin_path",
    kind: ValueKind::String,
    optional: true,
    variadic: true,
}];

const SPECS: &[BuiltinSpec] = &[
    BuiltinSpec {
        id: BuiltinId::Help,
        path: &["help"],
        aliases: &[],
        description: "Show builtin help and available commands.",
        style: BuiltinStyle::Both,
        input: ValueKind::None,
        output: ValueKind::String,
        args: HELP_ARGS,
        mutates_shell: false,
    },
    BuiltinSpec {
        id: BuiltinId::FsRead,
        path: &["fs", "read"],
        aliases: &["cat"],
        description: "Read a file into a string.",
        style: BuiltinStyle::Both,
        input: ValueKind::None,
        output: ValueKind::String,
        args: PATH_ARG,
        mutates_shell: false,
    },
    BuiltinSpec {
        id: BuiltinId::FsPwd,
        path: &["fs", "pwd"],
        aliases: &["pwd"],
        description: "Return the current working directory.",
        style: BuiltinStyle::Both,
        input: ValueKind::None,
        output: ValueKind::String,
        args: NO_ARGS,
        mutates_shell: false,
    },
    BuiltinSpec {
        id: BuiltinId::FsLs,
        path: &["fs", "ls"],
        aliases: &["ls"],
        description: "List directory entries.",
        style: BuiltinStyle::Both,
        input: ValueKind::None,
        output: ValueKind::List,
        args: OPTIONAL_PATH_ARG,
        mutates_shell: false,
    },
    BuiltinSpec {
        id: BuiltinId::Cd,
        path: &["cd"],
        aliases: &[],
        description: "Change the live shell working directory.",
        style: BuiltinStyle::Both,
        input: ValueKind::None,
        output: ValueKind::String,
        args: OPTIONAL_PATH_ARG,
        mutates_shell: true,
    },
    BuiltinSpec {
        id: BuiltinId::EnvGet,
        path: &["env", "get"],
        aliases: &[],
        description: "Read an environment variable from the live shell session.",
        style: BuiltinStyle::Both,
        input: ValueKind::None,
        output: ValueKind::Any,
        args: ENV_NAME_ARG,
        mutates_shell: false,
    },
    BuiltinSpec {
        id: BuiltinId::EnvSet,
        path: &["env", "set"],
        aliases: &[],
        description: "Set an environment variable in the live shell session.",
        style: BuiltinStyle::Both,
        input: ValueKind::None,
        output: ValueKind::String,
        args: ENV_SET_ARGS,
        mutates_shell: true,
    },
    BuiltinSpec {
        id: BuiltinId::EnvUnset,
        path: &["env", "unset"],
        aliases: &[],
        description: "Remove an environment variable from the live shell session.",
        style: BuiltinStyle::Both,
        input: ValueKind::None,
        output: ValueKind::None,
        args: ENV_NAME_ARG,
        mutates_shell: true,
    },
    BuiltinSpec {
        id: BuiltinId::EnvList,
        path: &["env", "list"],
        aliases: &[],
        description: "Return the current environment as an object.",
        style: BuiltinStyle::Both,
        input: ValueKind::None,
        output: ValueKind::Object,
        args: NO_ARGS,
        mutates_shell: false,
    },
    BuiltinSpec {
        id: BuiltinId::Map,
        path: &["map"],
        aliases: &[],
        description: "Transform each item in a list with a callback.",
        style: BuiltinStyle::Both,
        input: ValueKind::List,
        output: ValueKind::List,
        args: MAP_FILTER_ARGS,
        mutates_shell: false,
    },
    BuiltinSpec {
        id: BuiltinId::Filter,
        path: &["filter"],
        aliases: &[],
        description: "Keep list items where the callback returns truthy.",
        style: BuiltinStyle::Both,
        input: ValueKind::List,
        output: ValueKind::List,
        args: MAP_FILTER_ARGS,
        mutates_shell: false,
    },
    BuiltinSpec {
        id: BuiltinId::Lines,
        path: &["lines"],
        aliases: &[],
        description: "Split a string into a list of lines.",
        style: BuiltinStyle::Both,
        input: ValueKind::String,
        output: ValueKind::List,
        args: LINES_ARGS,
        mutates_shell: false,
    },
];

pub fn specs() -> &'static [BuiltinSpec] {
    SPECS
}

pub fn resolve_segments(segments: &[&str]) -> Option<BuiltinMatch> {
    let mut best: Option<BuiltinMatch> = None;

    for spec in SPECS {
        if segments.starts_with(spec.path) {
            update_best(&mut best, spec, spec.path.len());
        }

        for alias in spec.aliases {
            let alias_segments: Vec<_> = alias.split_whitespace().collect();
            if segments.starts_with(&alias_segments) {
                update_best(&mut best, spec, alias_segments.len());
            }
        }
    }

    best
}

pub fn find_spec_by_id(id: BuiltinId) -> &'static BuiltinSpec {
    SPECS
        .iter()
        .find(|spec| spec.id == id)
        .expect("builtin id must exist in registry")
}

pub fn render_help(target: Option<&BuiltinSpec>) -> String {
    match target {
        Some(spec) => {
            let usage = format_usage(spec);
            let aliases = if spec.aliases.is_empty() {
                "aliases: none".to_string()
            } else {
                format!("aliases: {}", spec.aliases.join(", "))
            };
            format!(
                "{usage}\n{}\nstyle: {}\ninput: {}\noutput: {}\n{}\nmutates shell: {}",
                spec.description,
                spec.style,
                spec.input,
                spec.output,
                aliases,
                if spec.mutates_shell { "yes" } else { "no" }
            )
        }
        None => {
            let mut lines = vec!["available builtins:".to_string()];
            for spec in SPECS {
                lines.push(format!(
                    "  {:<16} {}",
                    spec.path.join(" "),
                    spec.description
                ));
            }
            lines.join("\n")
        }
    }
}

pub fn format_usage(spec: &BuiltinSpec) -> String {
    let mut parts = vec![spec.path.join(" ")];
    for arg in spec.args {
        if arg.optional {
            parts.push(if arg.variadic {
                format!("[{}:{}...]", arg.name, arg.kind)
            } else {
                format!("[{}:{}]", arg.name, arg.kind)
            });
        } else {
            parts.push(if arg.variadic {
                format!("<{}:{}...>", arg.name, arg.kind)
            } else {
                format!("<{}:{}>", arg.name, arg.kind)
            });
        }
    }
    format!("usage: {}", parts.join(" "))
}

fn update_best(best: &mut Option<BuiltinMatch>, spec: &'static BuiltinSpec, consumed: usize) {
    match best {
        Some(current) if current.consumed >= consumed => {}
        _ => *best = Some(BuiltinMatch { spec, consumed }),
    }
}

impl fmt::Display for BuiltinStyle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BuiltinStyle::CommandOnly => write!(f, "command"),
            BuiltinStyle::FunctionOnly => write!(f, "function"),
            BuiltinStyle::Both => write!(f, "command/function"),
        }
    }
}

impl fmt::Display for ValueKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValueKind::None => write!(f, "none"),
            ValueKind::Any => write!(f, "any"),
            ValueKind::String => write!(f, "string"),
            ValueKind::List => write!(f, "list"),
            ValueKind::Function => write!(f, "function"),
            ValueKind::Object => write!(f, "object"),
            ValueKind::Path => write!(f, "path"),
            ValueKind::EnvName => write!(f, "env-name"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{BuiltinId, render_help, resolve_segments};

    #[test]
    fn resolves_longest_builtin_path() {
        let matched = resolve_segments(&["fs", "read", "LICENSE"]).expect("match");
        assert_eq!(matched.spec.id, BuiltinId::FsRead);
        assert_eq!(matched.consumed, 2);
    }

    #[test]
    fn resolves_aliases() {
        let matched = resolve_segments(&["cat", "LICENSE"]).expect("match");
        assert_eq!(matched.spec.id, BuiltinId::FsRead);
        assert_eq!(matched.consumed, 1);
    }

    #[test]
    fn renders_help() {
        let text = render_help(None);
        assert!(text.contains("fs read"));
        assert!(text.contains("env set"));
    }
}
