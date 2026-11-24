//! A unified diff of two texts by lines: how a slice's task changed from one attempt to the next.

/// Lines of context around each change.
const CONTEXT: usize = 3;
/// The largest LCS table (lines of the old middle × lines of the new middle) worth filling; a
/// bigger change is shown as all old lines removed and all new lines added.
const MAX_LCS_CELLS: usize = 4_000_000;

/// One line of the diff: its kind (`' '` kept, `'-'` removed, `'+'` added) and where it is in
/// the old and the new text.
#[derive(Clone, Copy)]
struct Op {
    kind: char,
    old: usize,
    new: usize,
}

/// A unified diff of two texts by lines (LCS), three lines of context; `""` when equal.
pub fn line_diff(old: &str, new: &str, old_name: &str, new_name: &str) -> String {
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();
    let ops = diff_ops(&old_lines, &new_lines);
    if ops.iter().all(|op| op.kind == ' ') {
        return String::new();
    }
    let mut out = format!("--- a/{old_name}\n+++ b/{new_name}\n");
    for hunk in hunks(&ops) {
        let old_count = hunk.iter().filter(|op| op.kind != '+').count();
        let new_count = hunk.iter().filter(|op| op.kind != '-').count();
        let old_start = hunk
            .iter()
            .find(|op| op.kind != '+')
            .map(|op| op.old + 1)
            .unwrap_or(hunk[0].old);
        let new_start = hunk
            .iter()
            .find(|op| op.kind != '-')
            .map(|op| op.new + 1)
            .unwrap_or(hunk[0].new);
        out.push_str(&format!(
            "@@ -{old_start},{old_count} +{new_start},{new_count} @@\n"
        ));
        for op in hunk {
            let line = if op.kind == '-' {
                old_lines[op.old]
            } else {
                new_lines[op.new]
            };
            out.push(op.kind);
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// Every line of both texts as kept, removed or added, in order.
fn diff_ops(old: &[&str], new: &[&str]) -> Vec<Op> {
    // Common prefix and suffix first: tasks differ mostly by appended notes.
    let mut prefix = 0;
    while prefix < old.len() && prefix < new.len() && old[prefix] == new[prefix] {
        prefix += 1;
    }
    let mut suffix = 0;
    while suffix < old.len() - prefix
        && suffix < new.len() - prefix
        && old[old.len() - 1 - suffix] == new[new.len() - 1 - suffix]
    {
        suffix += 1;
    }
    let old_middle = &old[prefix..old.len() - suffix];
    let new_middle = &new[prefix..new.len() - suffix];

    let mut ops: Vec<Op> = (0..prefix)
        .map(|i| Op {
            kind: ' ',
            old: i,
            new: i,
        })
        .collect();
    if old_middle.len() * new_middle.len() <= MAX_LCS_CELLS {
        ops.extend(lcs_ops(old_middle, new_middle, prefix));
    } else {
        ops.extend((0..old_middle.len()).map(|i| Op {
            kind: '-',
            old: prefix + i,
            new: prefix,
        }));
        ops.extend((0..new_middle.len()).map(|j| Op {
            kind: '+',
            old: prefix + old_middle.len(),
            new: prefix + j,
        }));
    }
    ops.extend((0..suffix).map(|k| Op {
        kind: ' ',
        old: old.len() - suffix + k,
        new: new.len() - suffix + k,
    }));
    ops
}

/// The ops of a longest-common-subsequence diff of `old` and `new`, which start at line
/// `offset` of both texts.
fn lcs_ops(old: &[&str], new: &[&str], offset: usize) -> Vec<Op> {
    let (n, m) = (old.len(), new.len());
    // `table[i][j]`: the LCS length of `old[i..]` and `new[j..]`, stored flat.
    let mut table = vec![0u32; (n + 1) * (m + 1)];
    let at = |i: usize, j: usize| i * (m + 1) + j;
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            table[at(i, j)] = if old[i] == new[j] {
                table[at(i + 1, j + 1)] + 1
            } else {
                table[at(i + 1, j)].max(table[at(i, j + 1)])
            };
        }
    }
    let mut ops = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < n || j < m {
        let op = |kind| Op {
            kind,
            old: offset + i,
            new: offset + j,
        };
        if i < n && j < m && old[i] == new[j] {
            ops.push(op(' '));
            i += 1;
            j += 1;
        } else if i < n && (j == m || table[at(i + 1, j)] >= table[at(i, j + 1)]) {
            ops.push(op('-'));
            i += 1;
        } else {
            ops.push(op('+'));
            j += 1;
        }
    }
    ops
}

/// The changed ops with [`CONTEXT`] lines around them, changes closer than two contexts apart
/// in one hunk.
fn hunks(ops: &[Op]) -> Vec<&[Op]> {
    let changed: Vec<usize> = ops
        .iter()
        .enumerate()
        .filter(|(_, op)| op.kind != ' ')
        .map(|(i, _)| i)
        .collect();
    let mut out = Vec::new();
    let mut k = 0;
    while k < changed.len() {
        let start = changed[k].saturating_sub(CONTEXT);
        let mut end = changed[k];
        while k < changed.len() && changed[k] <= end + 2 * CONTEXT {
            end = changed[k];
            k += 1;
        }
        let end = (end + CONTEXT + 1).min(ops.len());
        out.push(&ops[start..end]);
    }
    out
}
