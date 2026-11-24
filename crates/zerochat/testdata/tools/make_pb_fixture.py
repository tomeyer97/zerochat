#!/usr/bin/env python3
"""Build the pb (benchmarks) fixture from a real pb data directory, sanitised and shrunk.

    make_pb_fixture.py PB_DATA OUT [--keep-ledgers N] [--max-tests N]

Copies pb.sqlite (SQLite backup API, read-only source), pb.toml, harness/ and, for every run, only
the small files zerochat reads: runner.log (last 4 KiB), eval/score.json, steps/manifest.json,
harness/* and the Zeroshot ledger of the N smallest runs plus every run still `running` (for the
live-matrix code). Paths, host names and secrets are rewritten with sanitize_state.py's rules;
test_results keeps at most --max-tests rows per run.
"""

import argparse
import shutil
import sqlite3
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import sanitize_state as s  # noqa: E402


def main():
    p = argparse.ArgumentParser()
    p.add_argument("pb_data")
    p.add_argument("out")
    p.add_argument("--keep-ledgers", type=int, default=4)
    p.add_argument("--max-tests", type=int, default=25)
    p.add_argument("--map", action="append", default=[])
    p.add_argument("--max-string", type=int, default=1500)
    args = p.parse_args()
    src, out = Path(args.pb_data), Path(args.out)
    maps = [tuple(m.split("=", 1)) for m in args.map] + [(str(src), "/data/pb")]
    rewrite = s.build_rewriter(maps, args.max_string)
    if out.exists():
        shutil.rmtree(out)
    out.mkdir(parents=True)
    db = out / "pb.sqlite"
    source = sqlite3.connect(f"file:{src / 'pb.sqlite'}?mode=ro", uri=True)
    con = sqlite3.connect(db)
    source.backup(con)
    source.close()
    hosts = {r[0] for r in con.execute("SELECT DISTINCT host FROM batches WHERE host IS NOT NULL")}
    for h in hosts:
        con.execute("UPDATE batches SET host='bench-host' WHERE host=?", (h,))
    con.execute(f"DELETE FROM test_results WHERE (run_id, branch, name) NOT IN (SELECT run_id, branch, name FROM (SELECT run_id, branch, name, ROW_NUMBER() OVER (PARTITION BY run_id ORDER BY branch, name) n FROM test_results) WHERE n <= {args.max_tests})")
    for table in [r[0] for r in con.execute("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'")]:
        cols = [r[1] for r in con.execute(f"PRAGMA table_info({table})") if r[2].upper() in ("TEXT", "")]
        if not cols:
            continue
        for col in cols:
            for (value,) in con.execute(f"SELECT DISTINCT {col} FROM {table} WHERE typeof({col})='text'").fetchall():
                new = s.rewrite_text_cell(value, rewrite)
                for h in hosts:
                    new = new.replace(h, "bench-host")
                if new != value:
                    con.execute(f"UPDATE {table} SET {col}=? WHERE {col}=?", (new, value))
    con.commit()
    con.execute("PRAGMA journal_mode=DELETE")
    con.execute("VACUUM")
    running = {r[0] for r in con.execute("SELECT run_id FROM runs WHERE status='running'")}
    con.close()

    # Whole: a cut pb.toml would not parse.
    (out / "pb.toml").write_text(s.build_rewriter(maps, 0)((src / "pb.toml").read_text()))
    for f in (src / "harness").rglob("*"):
        if f.is_file() and "__pycache__" not in f.parts:
            t = out / "harness" / f.relative_to(src / "harness")
            t.parent.mkdir(parents=True, exist_ok=True)
            t.write_text(s.rewrite_text_cell(f.read_text(errors="replace"), rewrite))
    ledgers = sorted((p.stat().st_size, p) for p in (src / "runs").glob("*/agent/zeroshot-state/runs/*/runs.sqlite3"))
    keep = {p for _, p in ledgers[: args.keep_ledgers]} | {p for _, p in ledgers if p.parts[-6] in running}
    for run in sorted((src / "runs").iterdir()):
        if not run.is_dir():
            continue
        dst = out / "runs" / run.name
        for rel in ["eval/score.json", "steps/manifest.json"]:
            if (run / rel).is_file():
                (dst / rel).parent.mkdir(parents=True, exist_ok=True)
                (dst / rel).write_text(s.rewrite_text_cell((run / rel).read_text(), rewrite))
        if (run / "runner.log").is_file():
            dst.mkdir(parents=True, exist_ok=True)
            (dst / "runner.log").write_text(rewrite((run / "runner.log").read_text(errors="replace")[-4096:]))
        for f in (run / "harness").glob("*") if (run / "harness").is_dir() else []:
            if f.is_file():
                (dst / "harness").mkdir(parents=True, exist_ok=True)
                (dst / "harness" / f.name).write_text(s.rewrite_text_cell(f.read_text(errors="replace"), rewrite))
        for ledger in (run / "agent/zeroshot-state/runs").glob("*/runs.sqlite3"):
            if ledger in keep:
                zid = ledger.parent.name
                state_dst = dst / "agent/zeroshot-state"
                (state_dst / "runs" / zid).mkdir(parents=True, exist_ok=True)
                s.copy_ledger(ledger, state_dst / "runs" / zid / "runs.sqlite3", rewrite)
                for name in ["workspace-recovery.json", "checkpoints/index.json"]:
                    if (ledger.parent / name).is_file():
                        s.copy_json(ledger.parent / name, state_dst / "runs" / zid / name, rewrite)
    bad = s.check(out)
    if bad:
        print("forbidden content left:\n  " + "\n  ".join(bad[:20]), file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
