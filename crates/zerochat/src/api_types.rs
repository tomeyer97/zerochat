//! The TypeScript twins of the API's Rust types: `web/src/generated/<module>.ts`, one file per
//! Rust module, generated with ts-rs from the serialised structs and enums (`#[derive(TS)]`).
//!
//! `cargo test` fails when a generated file is out of date; `make types` rewrites them
//! (`UPDATE_TYPES=1 cargo test -p zerochat api_types`). Never edit the generated files by hand:
//! change the Rust type (or its `#[ts(...)]` attribute) and regenerate. A serialised field whose
//! Rust type says less than the JSON does (a `serde_json::Value`, a `&'static str` with a fixed
//! set of values) carries a `#[ts(type = "...")]` with the TypeScript shape it has on the wire.
//!
//! Registering a type: add it to its module's `#[cfg(test)] fn ts_module` (and a value set to that
//! module's `TS_SHAPES`), never here. A NEW top-level module would need a line in `modules()`, so
//! put new types in a module already registered.

use std::any::TypeId;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use ts_rs::{Config, TS, TypeVisitor};

/// Shapes the Rust side keeps as plain JSON (`serde_json::Value`): Zeroshot's own data, passed
/// through as Zeroshot wrote it. The value sets of fields typed `&'static str` in Rust live with
/// their module (`TS_SHAPES` fragments, see `shapes`). The `#[ts(type = "...")]` attributes refer
/// to these by name.
const SHAPES: &str = r#"/** Any JSON value (`serde_json::Value`). */
export type Json = null | boolean | number | string | Json[] | { [key: string]: Json };
/** A JSON object (`serde_json::Value` that is always an object). */
export type JsonObject = { [key: string]: Json };

/** Zeroshot's `terminal` record of a finished run. */
export type RunTerminal = { status?: string; reason?: string; output?: Json };

/** Zeroshot's error of a failed execution. */
export type ExecutionError = { code?: string | null; reason?: string | null };

/** What Zeroshot recorded about a node's workspace (16.1 does not record it; the older patched Zeroshot did). */
export type WorkspaceSnapshot = {
  commit?: string;
  /** The commit a node's changes are counted from. */
  base?: string;
  files?: { path: string; status: string; additions?: number | null; deletions?: number | null }[];
  [key: string]: Json | undefined;
};

/** Where a replayed run started from, as Zeroshot recorded it. */
export type ReplayOf = { runId?: string; checkpointId?: string; node?: string; [key: string]: Json | undefined };
"#;

/// The generated file `SHAPES` is written to.
const SHAPES_MODULE: &str = "shapes";

/// `shapes.ts`'s body: `SHAPES`, then each module's value-set fragment, one blank line between.
/// The order is fixed (and tested): a fragment changes in its own module, never here.
fn shape_fragments() -> [&'static str; 5] {
    [
        SHAPES,
        crate::projection::TS_SHAPES,
        crate::features::TS_SHAPES,
        crate::benchmarks::TS_SHAPES,
        crate::features::TS_ACTION_SHAPES,
    ]
}

fn shapes() -> String {
    shape_fragments().join("\n")
}

/// One generated module: the Rust module it mirrors and its types, in file order.
pub(crate) struct Module {
    name: &'static str,
    decls: Vec<Decl>,
    /// Display tables both sides use, written after the types as `export const`s.
    consts: Vec<String>,
}

impl Module {
    pub(crate) fn new(name: &'static str, decls: Vec<Decl>) -> Module {
        Module {
            name,
            decls,
            consts: Vec::new(),
        }
    }

    /// Adds display tables (`export const ...`, each with its `// module.rs: NAME` line and doc).
    pub(crate) fn with_consts(self, consts: Vec<String>) -> Module {
        Module { consts, ..self }
    }
}

/// One generated type.
pub(crate) struct Decl {
    /// Its TypeScript name.
    name: String,
    /// Where it comes from, written above it (`<module>.rs: <Type>`).
    rust: String,
    /// `export type ...`, with its doc comment.
    text: String,
    /// The generated types it refers to.
    deps: BTreeSet<String>,
}

pub(crate) fn decl<T: TS + 'static>(cfg: &Config, rust: &str) -> Decl {
    /// Collects the names of the derived types a type refers to.
    struct Deps<'a> {
        cfg: &'a Config,
        names: BTreeSet<String>,
        seen: BTreeSet<TypeId>,
    }
    impl TypeVisitor for Deps<'_> {
        fn visit<U: TS + 'static + ?Sized>(&mut self) {
            // Only declared types (ours) have an output path; maps, lists and primitives do not.
            if U::output_path().is_some() && self.seen.insert(TypeId::of::<U>()) {
                self.names.insert(U::ident(self.cfg));
            }
        }
    }
    let mut deps = Deps {
        cfg,
        names: BTreeSet::new(),
        seen: BTreeSet::new(),
    };
    T::visit_dependencies(&mut deps);
    let docs = T::docs().map(|d| doc_comment(&d, "")).unwrap_or_default();
    Decl {
        name: T::ident(cfg),
        rust: rust.to_owned(),
        text: format!("{docs}export {}", pretty(&T::decl(cfg))),
        deps: deps.names,
    }
}

/// `ts_types!(cfg, module: TypeA, TypeB)`: a module's generated types, in file order. Used in the
/// module's own `ts_module`, where the type names are in scope.
macro_rules! ts_types {
    ($cfg:expr, $module:ident: $($ty:ident),+ $(,)?) => {
        $crate::api_types::Module::new(
            stringify!($module),
            vec![$($crate::api_types::decl::<$ty>($cfg, concat!(stringify!($module), ".rs: ", stringify!($ty)))),+],
        )
    };
}
pub(crate) use ts_types;

/// Every module the web reads types from, in generated-file order. Each lists its own types in
/// its `ts_module`; a type another one refers to must be listed there too (the test says which is
/// missing).
fn modules(cfg: &Config) -> Vec<Module> {
    vec![
        crate::pricing::ts_module(cfg),
        crate::discovery::ts_module(cfg),
        crate::context::ts_module(cfg),
        crate::source::ts_module(cfg),
        crate::projection::ts_module(cfg),
        crate::transcript::ts_module(cfg),
        crate::harness::ts_module(cfg),
        crate::benchmarks::ts_module(cfg),
        crate::pb_exec::ts_module(cfg),
        crate::features::ts_module(cfg),
    ]
}

const HEADER: &str =
    "// Generated by crates/zerochat/src/api_types.rs (`make types`). Do not edit.\n";

/// The names `shapes.ts` declares.
fn shape_names() -> BTreeSet<String> {
    shapes()
        .lines()
        .filter_map(|l| l.strip_prefix("export type "))
        .filter_map(|l| l.split([' ', '=']).next())
        .map(str::to_owned)
        .collect()
}

/// Every generated file: name -> contents.
fn render() -> BTreeMap<String, String> {
    let cfg = Config::new().with_large_int("number");
    let modules = modules(&cfg);
    let mut owner: BTreeMap<String, &str> = shape_names()
        .into_iter()
        .map(|n| (n, SHAPES_MODULE))
        .collect();
    for m in &modules {
        for d in &m.decls {
            assert!(
                owner.insert(d.name.clone(), m.name).is_none(),
                "two generated types are called {}",
                d.name
            );
        }
    }
    let mut files = BTreeMap::new();
    files.insert(format!("{SHAPES_MODULE}.ts"), format!("{HEADER}// JSON the Rust side keeps untyped (`serde_json::Value`), and value sets of its string fields.\n\n{}", shapes()));
    for m in &modules {
        let mut imports: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
        for d in &m.decls {
            // Names used in `#[ts(type = "...")]` are not visited as dependencies: find them in the text.
            let used = owner
                .keys()
                .filter(|n| *n != &d.name && mentions(&d.text, n));
            for name in d.deps.iter().chain(used) {
                let from = owner.get(name).unwrap_or_else(|| {
                    panic!(
                        "{} uses {name}, which no module's `ts_module` lists",
                        d.rust
                    )
                });
                if *from != m.name {
                    imports.entry(from).or_default().insert(name);
                }
            }
        }
        let mut out = format!(
            "{HEADER}// The TypeScript twins of crates/zerochat/src/{}.rs.\n",
            m.name
        );
        if !imports.is_empty() {
            out.push('\n');
        }
        for (from, names) in &imports {
            out.push_str(&format!(
                "import type {{ {} }} from './{from}';\n",
                names.iter().copied().collect::<Vec<_>>().join(", ")
            ));
        }
        for d in &m.decls {
            out.push_str(&format!("\n// {}\n{}\n", d.rust, d.text));
        }
        for c in &m.consts {
            out.push_str(&format!("\n{c}\n"));
        }
        files.insert(format!("{}.ts", m.name), out);
    }
    files
}

/// ts-rs writes a declaration on one line (`type A = { a: string, /**\n * doc\n */\nb: number, };`):
/// one member per line, its doc comment above it, and long unions one variant per line.
fn pretty(decl: &str) -> String {
    let Some(eq) = decl.find(" = ") else {
        return decl.to_owned();
    };
    let (head, body) = (&decl[..eq + 3], decl[eq + 3..].trim_end_matches(';'));
    if !body.starts_with('{') {
        let variants = split_top(body, '|');
        if head.len() + body.len() <= 100 || variants.len() < 2 {
            return decl.to_owned();
        }
        let lines: Vec<String> = variants.iter().map(|v| format!("  | {v}")).collect();
        return format!("{}\n{};", head.trim_end(), lines.join("\n"));
    }
    let close = matching_brace(body);
    let mut out = format!("{head}{{\n");
    for member in split_top(&body[1..close], ',') {
        let (doc, field) = match member.strip_prefix("/**").and_then(|m| m.split_once("*/")) {
            Some((doc, field)) => (Some(doc), field.trim()),
            None => (None, member.as_str()),
        };
        if let Some(doc) = doc {
            out.push_str(&doc_comment(doc, "  "));
        }
        out.push_str(&format!("  {field};\n"));
    }
    format!("{out}}}{};", &body[close + 1..])
}

/// A doc comment's text (`/**`, `*/` and the leading `*`s gone) as a JSDoc comment: one line when it fits.
fn doc_comment(doc: &str, indent: &str) -> String {
    let doc = doc.trim().trim_start_matches("/**").trim_end_matches("*/");
    let lines: Vec<&str> = doc
        .lines()
        .map(|l| l.trim().trim_start_matches('*').trim())
        .filter(|l| !l.is_empty())
        .collect();
    match lines.as_slice() {
        [] => String::new(),
        [line] => format!("{indent}/** {line} */\n"),
        lines => {
            let body: String = lines.iter().map(|l| format!("{indent} * {l}\n")).collect();
            format!("{indent}/**\n{body}{indent} */\n")
        }
    }
}

/// The index of the `}` that closes the `{` `text` starts with.
fn matching_brace(text: &str) -> usize {
    let mut depth = 0;
    for (i, c) in scan(text) {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return i;
                }
            }
            _ => {}
        }
    }
    panic!("unbalanced braces in {text}")
}

/// `text` split at the `sep`s outside brackets, strings and comments, pieces trimmed, empty ones dropped.
fn split_top(text: &str, sep: char) -> Vec<String> {
    let (mut depth, mut start, mut out) = (0i32, 0, Vec::new());
    for (i, c) in scan(text) {
        match c {
            '{' | '[' | '(' | '<' => depth += 1,
            '}' | ']' | ')' | '>' => depth -= 1,
            c if c == sep && depth == 0 => {
                out.push(text[start..i].trim().to_owned());
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(text[start..].trim().to_owned());
    out.retain(|p| !p.is_empty());
    out
}

/// The characters of `text` that are code: string literals and comments are skipped.
fn scan(text: &str) -> impl Iterator<Item = (usize, char)> + '_ {
    let mut chars = text.char_indices().peekable();
    std::iter::from_fn(move || {
        loop {
            let (i, c) = chars.next()?;
            match c {
                '"' | '\'' => {
                    while let Some((_, d)) = chars.next() {
                        match d {
                            '\\' => {
                                chars.next();
                            }
                            d if d == c => break,
                            _ => {}
                        }
                    }
                }
                '/' if chars.peek().is_some_and(|(_, d)| *d == '*') => {
                    let mut prev = ' ';
                    for (_, d) in chars.by_ref() {
                        if prev == '*' && d == '/' {
                            break;
                        }
                        prev = d;
                    }
                }
                _ => return Some((i, c)),
            }
        }
    })
}

/// `name` appears in `text` as a whole identifier.
fn mentions(text: &str, name: &str) -> bool {
    text.match_indices(name).any(|(i, _)| {
        let ident = |c: char| c.is_alphanumeric() || c == '_';
        !text[..i].ends_with(ident) && !text[i + name.len()..].starts_with(ident)
    })
}

fn generated_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/src/generated")
}

#[test]
fn generated_typescript_is_up_to_date() {
    let dir = generated_dir();
    let files = render();
    if std::env::var_os("UPDATE_TYPES").is_some() {
        std::fs::create_dir_all(&dir).unwrap();
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            if !files.contains_key(&*entry.file_name().to_string_lossy()) {
                std::fs::remove_file(entry.path()).unwrap();
            }
        }
        for (name, text) in &files {
            std::fs::write(dir.join(name), text).unwrap();
        }
        return;
    }
    let mut stale = Vec::new();
    for (name, text) in &files {
        if std::fs::read_to_string(dir.join(name)).ok().as_deref() != Some(text.as_str()) {
            stale.push(name.as_str());
        }
    }
    assert!(
        stale.is_empty(),
        "web/src/generated/ is out of date ({}): run `make types`",
        stale.join(", ")
    );
}

#[test]
fn every_registered_module_and_shape_fragment_is_collected_in_order() {
    let cfg = Config::new().with_large_int("number");
    let names: Vec<&str> = modules(&cfg).iter().map(|m| m.name).collect();
    let expected = [
        "pricing",
        "discovery",
        "context",
        "source",
        "projection",
        "transcript",
        "harness",
        "benchmarks",
        "pb_exec",
        "features",
    ];
    assert_eq!(names, expected);
    let files = render();
    for name in expected {
        assert!(
            files.contains_key(&format!("{name}.ts")),
            "{name}.ts is not generated"
        );
    }

    // shapes.ts: the generic shapes, then projection's, features' states, benchmarks', features' actions.
    let declared = |text: &str| -> Vec<String> {
        text.lines()
            .filter_map(|l| l.strip_prefix("export type "))
            .filter_map(|l| l.split([' ', '=']).next())
            .map(str::to_owned)
            .collect()
    };
    assert_eq!(
        declared(SHAPES),
        [
            "Json",
            "JsonObject",
            "RunTerminal",
            "ExecutionError",
            "WorkspaceSnapshot",
            "ReplayOf"
        ]
    );
    let ordered = [
        SHAPES,
        crate::projection::TS_SHAPES,
        crate::features::TS_SHAPES,
        crate::benchmarks::TS_SHAPES,
        crate::features::TS_ACTION_SHAPES,
    ];
    assert_eq!(shape_fragments(), ordered);
    for f in ordered {
        assert!(
            !declared(f).is_empty(),
            "a shape fragment declares no type:\n{f}"
        );
        assert!(
            f.ends_with(";\n") && !f.starts_with('\n'),
            "a shape fragment must start with text and end with one newline:\n{f}"
        );
    }
    let body = ordered.join("\n");
    assert!(
        files["shapes.ts"].ends_with(&format!("\n\n{body}")),
        "shapes.ts is not the fragments joined by blank lines"
    );
    let all: Vec<String> = ordered.iter().flat_map(|f| declared(f)).collect();
    assert_eq!(declared(&files["shapes.ts"]), all);
}
