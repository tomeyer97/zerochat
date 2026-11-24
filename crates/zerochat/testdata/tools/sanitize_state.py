#!/usr/bin/env python3
"""Copy Zeroshot runs (and their Claude Code session logs) into a test fixture, sanitised.

    sanitize_state.py SRC_STATE DST_STATE RUN_ID... [--map FROM=TO]... [--claude-home DIR]
                      [--dst-home DIR] [--max-string N]

For every run: `runs/<id>/runs.sqlite3` (a consistent copy through SQLite's backup API, then
VACUUMed in rollback-journal mode), `workspace-recovery.json` and `checkpoints/index.json` when
present. Never the controller pid file, socket, lock or runtime directory: a fixture must not
look alive on the machine that runs the tests.

Every string in every text column and JSON file goes through the same rewriting:
  1. the `--map` pairs (longest first), then $HOME -> /home/user and the user name -> user;
  2. secrets: `sk-ant-…`, `ghp_…`, `github_pat_…`, `*_TOKEN=`/`*_KEY=` values -> REDACTED;
  3. strings longer than --max-string (default 6000) keep their head and say how much was cut.
With --claude-home, the session logs of each run's workspace (Claude Code's
`~/.claude/projects/<slug>/*.jsonl`, plus `subagents/`) are copied under
`--dst-home/dot-claude/projects/<slug of the rewritten workspace>/` (a dot directory would be
gitignored; the tests rename `dot-claude` to `.claude`), rewritten the same way.

The script fails if anything in the output still matches a forbidden pattern (a home path, a
temporary agent directory, a token), so a fixture cannot leak by accident.
"""

import argparse
import json
import os
import re
import shutil
import sqlite3
import sys
from pathlib import Path

SECRETS = [
    (re.compile(r"/tmp/claude-\d+/[^/\s\"']+/[0-9a-f-]{36}/scratchpad"), "/tmp/scratch"),
    (re.compile(r"/tmp/claude-\d+(/[^/\s\"']+)?"), "/tmp/agent"),
    (re.compile(r"-tmp-claude-\d+-[A-Za-z0-9-]+"), "-tmp-agent"),
    (re.compile(r"sk-ant-[A-Za-z0-9_\-]{8,}"), "sk-ant-REDACTED"),
    (re.compile(r"gh[pousr]_[A-Za-z0-9]{20,}"), "ghp_REDACTED"),
    (re.compile(r"github_pat_[A-Za-z0-9_]{20,}"), "github_pat_REDACTED"),
    (re.compile(r"\b([A-Z][A-Z0-9_]*(?:TOKEN|SECRET|API_KEY))=([^\s\"']+)"), r"\1=REDACTED"),
]


def build_rewriter(maps, max_string):
    home = str(Path.home())
    user = os.environ.get("USER") or Path.home().name
    pairs = sorted(maps, key=lambda p: -len(p[0])) + [(home, "/home/user")]
    user_re = re.compile(rf"\b{re.escape(user)}\b") if user and user not in ("user", "root") else None

    def rewrite(text):
        for a, b in pairs:
            if a:
                text = text.replace(a, b)
        if user_re:
            text = user_re.sub("user", text)
        for pattern, repl in SECRETS:
            text = pattern.sub(repl, text)
        if max_string and len(text) > max_string:
            text = text[:max_string] + f"…[fixture: {len(text) - max_string} more characters cut]"
        return text

    return rewrite


def rewrite_json(value, rewrite):
    if isinstance(value, str):
        return rewrite(value)
    if isinstance(value, list):
        return [rewrite_json(v, rewrite) for v in value]
    if isinstance(value, dict):
        return {rewrite(k) if isinstance(k, str) else k: rewrite_json(v, rewrite) for k, v in value.items()}
    return value


def rewrite_text_cell(text, rewrite):
    """A JSON document is rewritten value by value (so cutting long strings keeps it valid JSON);
    anything else as plain text."""
    try:
        value = json.loads(text)
    except (ValueError, TypeError):
        return rewrite(text)
    if isinstance(value, (dict, list)):
        return json.dumps(rewrite_json(value, rewrite), ensure_ascii=False, separators=(",", ":"))
    return rewrite(text)


def copy_ledger(src, dst, rewrite):
    source = sqlite3.connect(f"file:{src}?mode=ro", uri=True)
    target = sqlite3.connect(dst)
    source.backup(target)
    source.close()
    tables = [r[0] for r in target.execute("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'")]
    for table in tables:
        columns = [r[1] for r in target.execute(f"PRAGMA table_info({table})") if r[2].upper() == "TEXT"]
        rows = target.execute(f"SELECT rowid, {', '.join(columns)} FROM {table}").fetchall() if columns else []
        for row in rows:
            new = [rewrite_text_cell(v, rewrite) if isinstance(v, str) else v for v in row[1:]]
            if new != list(row[1:]):
                target.execute(f"UPDATE {table} SET {', '.join(c + '=?' for c in columns)} WHERE rowid=?", (*new, row[0]))
    target.commit()
    target.execute("PRAGMA journal_mode=DELETE")
    target.execute("VACUUM")
    target.close()


def copy_json(src, dst, rewrite):
    dst.parent.mkdir(parents=True, exist_ok=True)
    value = json.loads(src.read_text())
    dst.write_text(json.dumps(rewrite_json(value, rewrite), indent=2, ensure_ascii=False) + "\n")


def copy_jsonl(src, dst, rewrite):
    dst.parent.mkdir(parents=True, exist_ok=True)
    out = []
    for line in src.read_text(errors="replace").splitlines():
        out.append(rewrite_text_cell(line, rewrite) if line.strip() else line)
    dst.write_text("\n".join(out) + "\n")


def slug(path):
    return "".join(c if c.isascii() and c.isalnum() else "-" for c in path)


FORBIDDEN = [re.compile(p) for p in [r"/home/(?!user\b)[a-z]", r"/tmp/claude-", r"sk-ant-(?!REDACTED)", r"cloudcoder", r"gh[pousr]_[A-Za-z0-9]{20,}"]]


def check(root):
    bad = []
    for path in Path(root).rglob("*"):
        if not path.is_file():
            continue
        data = path.read_bytes().decode("utf-8", errors="replace")
        for pattern in FORBIDDEN:
            m = pattern.search(data)
            if m:
                bad.append(f"{path}: {pattern.pattern} near {data[max(0, m.start() - 40):m.end() + 40]!r}")
    return bad


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("src_state")
    p.add_argument("dst_state")
    p.add_argument("runs", nargs="+")
    p.add_argument("--map", action="append", default=[], help="FROM=TO string rewrite (applied first, longest first)")
    p.add_argument("--claude-home", help="home directory whose .claude/projects holds the runs' session logs")
    p.add_argument("--dst-home", help="fixture home directory for the session logs")
    p.add_argument("--max-string", type=int, default=6000)
    args = p.parse_args()
    maps = [tuple(m.split("=", 1)) for m in args.map]
    rewrite = build_rewriter(maps, args.max_string)
    src, dst = Path(args.src_state), Path(args.dst_state)
    for run in args.runs:
        run_src, run_dst = src / "runs" / run, dst / "runs" / run
        if run_dst.exists():
            shutil.rmtree(run_dst)
        run_dst.mkdir(parents=True)
        copy_ledger(run_src / "runs.sqlite3", run_dst / "runs.sqlite3", rewrite)
        for name in ["workspace-recovery.json", "checkpoints/index.json"]:
            if (run_src / name).is_file():
                copy_json(run_src / name, run_dst / name, rewrite)
        if args.claude_home and args.dst_home and (run_src / "workspace-recovery.json").is_file():
            workspace = json.loads((run_src / "workspace-recovery.json").read_text()).get("workspace")
            project = Path(args.claude_home) / ".claude/projects" / slug(workspace or "")
            if workspace and project.is_dir():
                target = Path(args.dst_home) / "dot-claude/projects" / slug(rewrite(workspace))
                for log in project.rglob("*.jsonl"):
                    copy_jsonl(log, target / log.relative_to(project), rewrite)
    roots = [dst] + ([Path(args.dst_home)] if args.dst_home else [])
    bad = [b for r in roots if r.exists() for b in check(r)]
    if bad:
        print("forbidden content left in the fixture:\n  " + "\n  ".join(bad[:20]), file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
