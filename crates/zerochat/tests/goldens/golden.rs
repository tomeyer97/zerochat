//! The one golden-file mechanism (tests/GOLDENS.md): a value is pretty-printed JSON (object keys
//! sorted, so the file is stable) and compared with `testdata/golden/<name>.json`. A mismatch
//! fails with the first differing lines and writes the actual output next to the golden as
//! `<name>.actual.json` (gitignored). `UPDATE_GOLDENS=1` rewrites the goldens instead.

use std::path::{Path, PathBuf};

use serde_json::Value;

pub fn golden_dir(manifest_dir: &str) -> PathBuf {
    Path::new(manifest_dir).join("testdata/golden")
}

fn updating() -> bool {
    std::env::var_os("UPDATE_GOLDENS").is_some_and(|v| !v.is_empty() && v != "0")
}

/// Sorted keys, two-space indent, trailing newline.
pub fn render(value: &Value) -> String {
    fn sort(v: &Value) -> Value {
        match v {
            Value::Object(map) => {
                let mut keys: Vec<&String> = map.keys().collect();
                keys.sort();
                Value::Object(
                    keys.into_iter()
                        .map(|k| (k.clone(), sort(&map[k])))
                        .collect(),
                )
            }
            Value::Array(items) => Value::Array(items.iter().map(sort).collect()),
            other => other.clone(),
        }
    }
    serde_json::to_string_pretty(&sort(value)).unwrap() + "\n"
}

/// Compares `value` with the golden `name` (a relative path without extension).
pub fn check_json_in(dir: &Path, name: &str, value: &Value) {
    check_text_in(dir, &format!("{name}.json"), &render(value));
}

pub fn check_text_in(dir: &Path, file: &str, actual: &str) {
    let path = dir.join(file);
    let actual_path = actual_path(&path);
    if updating() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, actual).unwrap();
        let _ = std::fs::remove_file(&actual_path);
        return;
    }
    let expected = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(_) => {
            let _ = std::fs::write(&actual_path, actual);
            panic!(
                "golden {file}: no golden file yet ({}). Review {} and run with UPDATE_GOLDENS=1 to accept it.",
                path.display(),
                actual_path.display()
            );
        }
    };
    if expected == actual {
        let _ = std::fs::remove_file(&actual_path);
        return;
    }
    let _ = std::fs::write(&actual_path, actual);
    panic!(
        "golden {file} changed:\n{}\nactual output: {}\n(if the change is intended: UPDATE_GOLDENS=1 cargo test, then review the diff)",
        first_differences(&expected, actual, 24),
        actual_path.display()
    );
}

fn actual_path(path: &Path) -> PathBuf {
    let name = path.file_name().unwrap().to_string_lossy();
    let (stem, ext) = name.rsplit_once('.').unwrap_or((&name, "txt"));
    path.with_file_name(format!("{stem}.actual.{ext}"))
}

/// A short line diff: the first `max` differing lines with a line of context.
pub fn first_differences(expected: &str, actual: &str, max: usize) -> String {
    let e: Vec<&str> = expected.lines().collect();
    let a: Vec<&str> = actual.lines().collect();
    let mut out = Vec::new();
    let mut shown = 0;
    let n = e.len().max(a.len());
    let mut i = 0;
    while i < n && shown < max {
        let (x, y) = (e.get(i), a.get(i));
        if x != y {
            if let Some(prev) = i.checked_sub(1).and_then(|p| e.get(p)) {
                out.push(format!("   {:>5}  {prev}", i));
            }
            if let Some(x) = x {
                out.push(format!(" - {:>5}  {x}", i + 1));
            }
            if let Some(y) = y {
                out.push(format!(" + {:>5}  {y}", i + 1));
            }
            shown += 1;
        }
        i += 1;
    }
    if e.len() != a.len() {
        out.push(format!(
            "   ({} lines expected, {} actual)",
            e.len(),
            a.len()
        ));
    }
    out.join("\n")
}

/// Replaces every occurrence of each `from` inside every string (and object key).
pub fn replace_strings(value: &mut Value, pairs: &[(String, String)]) {
    match value {
        Value::String(s) => {
            for (from, to) in pairs {
                if !from.is_empty() && s.contains(from.as_str()) {
                    *s = s.replace(from.as_str(), to);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|v| replace_strings(v, pairs)),
        Value::Object(map) => {
            let old = std::mem::take(map);
            for (mut k, mut v) in old {
                for (from, to) in pairs {
                    if !from.is_empty() && k.contains(from.as_str()) {
                        k = k.replace(from.as_str(), to);
                    }
                }
                replace_strings(&mut v, pairs);
                map.insert(k, v);
            }
        }
        _ => {}
    }
}
