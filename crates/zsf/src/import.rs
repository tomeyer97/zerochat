//! `zsf import`: a refined spec (`slices/INDEX.md` + `slices/NN-*.md`) → `plan.toml`.
//!
//! A refined spec is a feature already cut into slices by a person or a planning tool: a
//! directory with `slices/INDEX.md`, a Markdown table listing the slices, and one task file per
//! slice named `NN-<slug>.md`. zsf does not write it; the parser accepts the loose shapes such
//! documents come in.
//!
//! Slices come from the INDEX table (columns found by their header: id, title, depends on) and
//! the slice files' headers (`# NN: Title`, `**Depends on:** 01, 05 | …`, `**Title:** …`,
//! `**Description:** …`). Dependencies are read only from the depends-on column or header, and
//! only ids of slices in the plan count (`02-04` ranges too); text in parentheses is ignored,
//! so `01, 05 (after the schema change lands; ~2 days)` is `01, 05`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::model::{Plan, SlicePlan, valid_id};

#[derive(Debug)]
pub struct Imported {
    pub plan: Plan,
    pub warnings: Vec<String>,
    /// The directory holding INDEX.md and the slice files.
    pub slices_dir: PathBuf,
}

struct Row {
    id: String,
    title: Option<String>,
    file: Option<String>,
    deps: Option<String>,
}

/// Reads `<dir>/slices/INDEX.md` (or `<dir>/INDEX.md`). Spec paths are absolute.
pub fn parse_refined(dir: &Path) -> Result<Imported> {
    let slices_dir = if dir.join("slices/INDEX.md").is_file() || dir.join("slices").is_dir() {
        dir.join("slices")
    } else {
        dir.to_path_buf()
    };
    let index = slices_dir.join("INDEX.md");
    let mut warnings = Vec::new();
    let rows = match std::fs::read_to_string(&index) {
        Ok(text) => index_rows(&text),
        Err(_) => {
            warnings.push(format!(
                "no {}; slices taken from the NN-*.md files",
                index.display()
            ));
            Vec::new()
        }
    };
    let files = slice_files(&slices_dir);
    let mut rows = if rows.is_empty() {
        files
            .iter()
            .map(|(id, f)| Row {
                id: id.clone(),
                title: None,
                file: Some(f.clone()),
                deps: None,
            })
            .collect()
    } else {
        rows
    };
    if rows.is_empty() {
        bail!("no slices found in {}", slices_dir.display());
    }
    let ids: Vec<String> = rows.iter().map(|r| r.id.clone()).collect();
    let mut slices = Vec::new();
    for row in rows.iter_mut() {
        let file = row.file.clone().or_else(|| files.get(&row.id).cloned());
        let Some(file) = file else {
            bail!(
                "slice {}: no slice file (expected {}/{}-*.md)",
                row.id,
                slices_dir.display(),
                row.id
            )
        };
        let path = slices_dir.join(&file);
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("slice {}: cannot read {}", row.id, path.display()))?;
        let header = SliceHeader::parse(&text);
        let title = row
            .title
            .clone()
            .or(header.title.clone())
            .unwrap_or_else(|| row.id.clone());
        let index_deps = row.deps.as_deref().map(|d| parse_deps(d, &ids));
        let file_deps = header.deps.as_deref().map(|d| parse_deps(d, &ids));
        let deps = match (index_deps, file_deps) {
            (Some(a), Some(b)) => {
                if a != b {
                    warnings.push(format!(
                        "slice {}: INDEX.md says deps [{}], its file says [{}]; using the file",
                        row.id,
                        a.join(", "),
                        b.join(", ")
                    ));
                }
                b
            }
            (Some(a), None) => a,
            (None, Some(b)) => b,
            (None, None) => Vec::new(),
        };
        let deps: Vec<String> = deps.into_iter().filter(|d| d != &row.id).collect();
        slices.push(SlicePlan {
            id: row.id.clone(),
            title,
            spec: path,
            deps,
            commit_title: header.commit_title,
            description: header.description,
            ..Default::default()
        });
    }
    slices.sort_by_key(|s| id_key(&s.id));
    let plan = Plan { slices };
    plan.validate(&BTreeMap::new())?;
    Ok(Imported {
        plan,
        warnings,
        slices_dir,
    })
}

/// Numeric ids sort as numbers.
fn id_key(id: &str) -> (u64, String) {
    (id.parse().unwrap_or(u64::MAX), id.to_owned())
}

/// `NN-*.md` files by id.
fn slice_files(dir: &Path) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.ends_with(".md") || name == "INDEX.md" {
            continue;
        }
        if let Some((id, _)) = name.split_once('-')
            && !id.is_empty()
            && id.chars().all(|c| c.is_ascii_digit())
        {
            out.entry(id.to_owned()).or_insert(name);
        }
    }
    out
}

fn cells(line: &str) -> Vec<String> {
    let inner = line.trim().trim_start_matches('|').trim_end_matches('|');
    inner.split('|').map(|c| c.trim().to_owned()).collect()
}

fn is_separator(line: &str) -> bool {
    let line = line.trim();
    line.starts_with('|') && line.contains('-') && line.chars().all(|c| "|-: ".contains(c))
}

/// Strips `**`, backticks and link syntax: `[**Title**](x.md)` → `Title`.
fn plain(text: &str) -> String {
    let (label, _) = link(text);
    label.replace("**", "").replace('`', "").trim().to_owned()
}

/// `[label](target)` → (label, Some(target)); anything else → (text, None).
fn link(text: &str) -> (String, Option<String>) {
    let t = text.trim();
    if let (Some(open), Some(mid)) = (t.find('['), t.find("]("))
        && open < mid
        && let Some(close) = t[mid + 2..].find(')')
    {
        let label = &t[open + 1..mid];
        let target = &t[mid + 2..mid + 2 + close];
        return (
            format!("{}{}{}", &t[..open], label, &t[mid + 3 + close..]),
            Some(target.to_owned()),
        );
    }
    (t.to_owned(), None)
}

// INDEX table headers (lowercased, Markdown stripped) that name each column.
/// The slice id column.
const ID_HEADERS: &[&str] = &["id", "#", "nn", "no", "no.", "slice id"];
/// A `Slice` column: the id column when there is no other, else the title column.
const SLICE_HEADER: &str = "slice";
/// The title column: a header containing one of these words…
const TITLE_HEADER_WORDS: &[&str] = &["title"];
/// …or equal to one of these.
const TITLE_HEADERS: &[&str] = &["name", "summary"];
/// The depends-on column: a header containing one of these words…
const DEPS_HEADER_WORDS: &[&str] = &["depend"];
/// …or equal to one of these.
const DEPS_HEADERS: &[&str] = &["deps", "requires"];

/// Where the columns of an INDEX table are, from its header row.
struct Columns {
    id: usize,
    title: Option<usize>,
    deps: Option<usize>,
}

impl Columns {
    /// `None` when the table has neither an id nor a title column.
    fn find(header: &[String]) -> Option<Columns> {
        let named = |exact: &[&str], words: &[&str]| {
            header
                .iter()
                .position(|h| exact.contains(&h.as_str()) || words.iter().any(|w| h.contains(w)))
        };
        let id = named(ID_HEADERS, &[]).or_else(|| named(&[SLICE_HEADER], &[]));
        // `| # | Slice | …`: the slice column holds the title (and the link to its file).
        let title = named(TITLE_HEADERS, TITLE_HEADER_WORDS).or_else(|| {
            header
                .iter()
                .enumerate()
                .position(|(i, h)| h == SLICE_HEADER && Some(i) != id)
        });
        let deps = named(DEPS_HEADERS, DEPS_HEADER_WORDS);
        Some(Columns {
            id: id.or(title)?,
            title,
            deps,
        })
    }
}

/// The first table whose header names an id (or slice) column.
fn index_rows(text: &str) -> Vec<Row> {
    let lines: Vec<&str> = text.lines().collect();
    for i in 0..lines.len().saturating_sub(1) {
        if !lines[i].trim().starts_with('|') || !is_separator(lines[i + 1]) {
            continue;
        }
        let header: Vec<String> = cells(lines[i])
            .iter()
            .map(|c| plain(c).to_lowercase())
            .collect();
        let Some(columns) = Columns::find(&header) else {
            continue;
        };
        let body = lines[i + 2..]
            .iter()
            .take_while(|l| l.trim().starts_with('|'));
        let rows: Vec<Row> = body
            .filter_map(|line| table_row(&cells(line), &columns))
            .collect();
        if !rows.is_empty() {
            return rows;
        }
    }
    Vec::new()
}

/// One INDEX table row as a slice, or `None` when no valid id can be found in it.
fn table_row(row: &[String], columns: &Columns) -> Option<Row> {
    let cell = |c: Option<usize>| c.and_then(|c| row.get(c)).cloned();
    let id_cell = cell(Some(columns.id))?;
    let title_cell = cell(columns.title);
    let file = title_cell
        .as_deref()
        .and_then(|c| link(c).1)
        .or_else(|| link(&id_cell).1)
        .filter(|f| f.ends_with(".md"))
        .map(|f| f.rsplit('/').next().unwrap_or(&f).to_owned());
    let id = row_id(&id_cell, file.as_deref());
    if !valid_id(&id) {
        return None;
    }
    let title = title_cell
        .filter(|_| columns.title != Some(columns.id))
        .map(|c| plain(&c))
        .filter(|t| !t.is_empty());
    Some(Row {
        id,
        title,
        file,
        deps: cell(columns.deps),
    })
}

/// The slice id of a row, from its id cell and the slice file it links to (if any).
fn row_id(id_cell: &str, file: Option<&str>) -> String {
    let file_prefix = file.and_then(|f| f.split_once('-')).map(|(p, _)| p);
    let mut id = plain(id_cell);
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
    {
        id = leading_id(&id).unwrap_or_else(|| file_prefix.unwrap_or_default().to_owned());
    }
    // `5` linking to `05-docs.md` is slice `05`, like its file.
    if let Some(prefix) = file_prefix
        && let (Ok(a), Ok(b)) = (id.parse::<u64>(), prefix.parse::<u64>())
        && a == b
    {
        id = prefix.to_owned();
    }
    id
}

/// `01: Title` → `01`: the leading run of letters, digits and dots, when it has a digit (a cell
/// that is only a title has none, and the caller falls back to the file's prefix).
fn leading_id(cell: &str) -> Option<String> {
    let lead = cell
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '.'))
        .next()
        .unwrap_or("");
    lead.chars()
        .any(|c| c.is_ascii_digit())
        .then(|| lead.to_owned())
}

#[derive(Default)]
struct SliceHeader {
    title: Option<String>,
    deps: Option<String>,
    commit_title: Option<String>,
    description: Option<String>,
}

impl SliceHeader {
    fn parse(text: &str) -> SliceHeader {
        let mut header = SliceHeader::default();
        for line in text.lines() {
            let trimmed = line.trim();
            if header.title.is_none()
                && let Some(h) = trimmed.strip_prefix("# ")
            {
                let t = h.split_once(':').map(|(_, t)| t).unwrap_or(h);
                header.title = Some(plain(t)).filter(|t| !t.is_empty());
            }
            if header.deps.is_none()
                && let Some(value) = field(trimmed, "Depends on")
            {
                header.deps = Some(value);
            }
            if header.commit_title.is_none() {
                header.commit_title = field(trimmed, "Title")
                    .map(|v| plain(&v))
                    .filter(|v| !v.is_empty());
            }
            if header.description.is_none() {
                header.description = field(trimmed, "Description").filter(|v| !v.is_empty());
            }
        }
        header
    }
}

/// The value of `**Key:** value | **Other:** …` (up to the next `|` or bold field).
fn field(line: &str, key: &str) -> Option<String> {
    let marker = format!("**{key}:**");
    let start = line.find(&marker)? + marker.len();
    let rest = &line[start..];
    let end = [rest.find(" | "), rest.find("**")]
        .into_iter()
        .flatten()
        .min()
        .unwrap_or(rest.len());
    Some(rest[..end].trim().trim_end_matches('|').trim().to_owned())
}

/// Separators that make `a <sep> b` a range of ids: hyphen, en dash, `..` and the word `to`.
const RANGE_SEPARATORS: &[&str] = &["-", "\u{2013}", "..", "to"];

/// Dependency ids in one depends-on cell, keeping only ids of the plan.
pub fn parse_deps(cell: &str, ids: &[String]) -> Vec<String> {
    let text = strip_parens(&plain(cell));
    let known = |token: &str| -> Option<String> {
        let token = token.trim_end_matches('.');
        if ids.iter().any(|i| i == token) {
            return Some(token.to_owned());
        }
        // `1` for `01`.
        let n: u64 = token.parse().ok()?;
        ids.iter()
            .find(|i| i.parse::<u64>().ok() == Some(n))
            .cloned()
    };
    let tokens = tokenize(&text);
    let mut out: Vec<String> = Vec::new();
    let mut add = |id: String| {
        if !out.contains(&id) {
            out.push(id);
        }
    };
    for (i, token) in tokens.iter().enumerate() {
        let Some(id) = known(&token.text) else {
            continue;
        };
        let is_range = RANGE_SEPARATORS.contains(&token.sep.trim());
        if is_range
            && i > 0
            && let Some(lo) = known(&tokens[i - 1].text)
        {
            for mid in ids_between(&lo, &id, ids) {
                add(mid);
            }
        }
        add(id);
    }
    out
}

/// A word of a depends-on cell and the text that separates it from the previous word.
struct Token {
    sep: String,
    text: String,
}

/// Splits `01, 03-05 and 7` into words (letters, digits, `_`, `.`), each with its separator.
fn tokenize(text: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut sep = String::new();
    let mut current = String::new();
    for c in text.chars() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '.' {
            current.push(c);
        } else {
            if !current.is_empty() {
                tokens.push(Token {
                    sep: std::mem::take(&mut sep),
                    text: std::mem::take(&mut current),
                });
            }
            sep.push(c);
        }
    }
    if !current.is_empty() {
        tokens.push(Token { sep, text: current });
    }
    tokens
}

/// The plan ids strictly between the numeric ids `lo` and `hi` (`02`–`05` → `03`, `04`).
fn ids_between(lo: &str, hi: &str, ids: &[String]) -> Vec<String> {
    let (Ok(lo), Ok(hi)) = (lo.parse::<u64>(), hi.parse::<u64>()) else {
        return Vec::new();
    };
    (lo + 1..hi)
        .filter_map(|n| ids.iter().find(|x| x.parse::<u64>().ok() == Some(n)))
        .cloned()
        .collect()
}

fn strip_parens(text: &str) -> String {
    let mut depth = 0usize;
    let mut out = String::new();
    for c in text.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

/// Imports into a feature: copies the refined directory to `<feature>/context` once and writes
/// `plan.toml` with spec paths inside that copy.
pub fn import_into(
    feature: &crate::store::Feature,
    dir: &Path,
    checkpoints: &[String],
) -> Result<Imported> {
    let state = feature.load_state()?;
    if state.slices.values().any(|s| !s.attempts.is_empty()) {
        bail!(
            "{} has already started slices; edit them with `zsf edit`",
            feature.name()
        );
    }
    let dir = dir
        .canonicalize()
        .with_context(|| format!("no directory {}", dir.display()))?;
    let mut imported = parse_refined(&dir)?;
    let context = feature.dir.join("context");
    if context.exists() {
        std::fs::remove_dir_all(&context)?;
    }
    crate::jobs::copy_dir(&dir, &context)?;
    let plan_dir = feature
        .plan_path()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| feature.dir.clone());
    for slice in &mut imported.plan.slices {
        let rel = slice
            .spec
            .strip_prefix(&dir)
            .map(Path::to_path_buf)
            .unwrap_or_else(|_| slice.spec.clone());
        let copied = context.join(rel);
        slice.spec = copied
            .strip_prefix(&plan_dir)
            .map(Path::to_path_buf)
            .unwrap_or(copied);
    }
    imported.plan.mark_checkpoints(checkpoints);
    for c in checkpoints {
        if imported.plan.get(c).is_none() {
            bail!("checkpoint {c} is not a slice of the plan");
        }
    }
    feature.save_plan(&imported.plan)?;
    Ok(imported)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(n: u32) -> Vec<String> {
        (1..=n).map(|i| format!("{i:02}")).collect()
    }

    #[test]
    fn deps_only_count_plan_ids() {
        let ids = ids(12);
        assert_eq!(
            parse_deps("01, 05 (after 12 lands; ~2 days)", &ids),
            ["01", "05"]
        );
        assert_eq!(parse_deps("—", &ids), Vec::<String>::new());
        assert_eq!(parse_deps("none", &ids), Vec::<String>::new());
        assert_eq!(parse_deps("02–04, 7", &ids), ["02", "03", "04", "07"]);
        assert_eq!(parse_deps("[03](03-x.md), **06**", &ids), ["03", "06"]);
        assert_eq!(parse_deps("R10, N3, 99", &ids), Vec::<String>::new());
    }

    #[test]
    fn reads_bold_fields() {
        assert_eq!(
            field(
                "**Depends on:** 01, 05 | **Parallel with:** 03 | **Size:** M",
                "Depends on"
            )
            .as_deref(),
            Some("01, 05")
        );
        assert_eq!(
            field("**Depends on:** — | **Size:** S", "Depends on").as_deref(),
            Some("—")
        );
        assert_eq!(
            field("**Title:** `ACME 01: add it`", "Title")
                .map(|v| plain(&v))
                .as_deref(),
            Some("ACME 01: add it")
        );
    }
}
