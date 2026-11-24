#!/usr/bin/env python3
"""Generate the synthetic Zeroshot ledger fixtures: testdata/ledgers/synthetic/<name>.json.

Recorded runs (testdata/recorded) show what real Zeroshot writes; these cover what a handful of
real runs cannot: every execution state, failure reason and class, provider-error diagnosis,
pressure kind, retries, steer records, replays and resumes, usage and cost sources. The event
shapes are the fork's (observe fields `at`, `provider`, `workspace`, `logKind`, `costUsd`,
`model`), copied from real ledgers; `stock: true` scenarios drop those fields as stock Zeroshot
does. Run `python3 make_synthetic.py` and commit the JSON; the golden tests load it into SQLite.

Each file: {name, runId, where: local|observe, tags, stored: {admitted, snapshot}, events: [...],
recovery: {...}|null, controllerPid: n|null, checkpoints: bool, sessions: {...}}.
"""

import json
import sys
from pathlib import Path

OUT = Path(__file__).resolve().parent.parent / "ledgers" / "synthetic"
T0 = 1_795_000_000_000          # 2026-11-18T08:26:40Z
NOW = T0 + 3_600_000            # the fixed "now" of the golden tests (tests/goldens/world.rs NOW_MS)
SCENARIOS = {}


def run_id(i, ts):
    h = f"{ts:012x}"
    return f"{h[:8]}-{h[8:12]}-7{i:03x}-8{i:03x}-{i:012x}"


def graph_change(rounds=2, worker_attempts=2, timeout=45_000):
    return {"profile": "openengine.graph.full/v1", "root": {"kind": "seq", "name": "run", "children": [
        {"kind": "loop", "name": "change_loop", "maxIterations": rounds, "body": {"kind": "seq", "name": "iteration", "children": [
            {"kind": "step", "name": "code", "worker": "builtin.agent.worker@1", "instructions": "Implement the task.", "attempts": worker_attempts, "timeoutMs": timeout},
            {"kind": "par", "name": "reviews", "children": [
                {"kind": "verifier", "name": "acceptance", "worker": "builtin.agent.reviewer@1", "attempts": 1},
                {"kind": "verifier", "name": "code_review", "worker": "builtin.agent.reviewer@1", "attempts": 1},
            ]},
        ]}},
        {"kind": "fail", "name": "change_attempts_exhausted", "reason": "change_attempts_exhausted"},
    ]}}


def graph_single(timeout=45_000, attempts=1):
    return {"profile": "openengine.graph.full/v1", "root": {"kind": "seq", "name": "run", "children": [
        {"kind": "step", "name": "worker", "worker": "builtin.agent.worker@1", "instructions": "Do the task.", "attempts": attempts, "timeoutMs": timeout},
    ]}}


def graph_map():
    return {"profile": "openengine.graph.full/v1", "root": {"kind": "seq", "name": "run", "children": [
        {"kind": "map", "name": "per_file", "over": "files", "body": {"kind": "step", "name": "fix", "worker": "builtin.agent.worker@1", "attempts": 1}},
        {"kind": "step", "name": "summarize", "worker": "builtin.agent.worker@1", "attempts": 1},
    ]}}


def runtime(harness="claude", model="claude-haiku-4-5", nodes=("code", "acceptance", "code_review", "worker"), connections=True):
    rt = {"harness": harness, "provider": {"claude": "anthropic", "codex": "openai", "copilot": "github"}[harness], "model": model, "nodes": {}}
    for n in nodes:
        rt["nodes"][n] = {"kind": "agent", "model": model}
        if connections:
            rt["nodes"][n]["connections"] = {"anthropic": ["CLAUDE_CODE_OAUTH_TOKEN"]} if harness == "claude" else {"openai": ["OPENAI_API_KEY"]}
    return rt


class Run:
    def __init__(self, name, i, graph, rt, stock=False, where="local", title=None, tags=()):
        self.name, self.i, self.stock, self.where = name, i, stock, where
        self.start = T0 + i * 60_000
        self.id = run_id(i, self.start)
        self.t = self.start
        self.events = []
        self.exec_n = 0
        self.graph, self.rt = graph, rt
        self.title = title or name.replace("-", " ")
        self.tags = list(tags)
        self.recovery = {"workspace": f"/work/{name}/repo"}
        self.controller = None
        self.checkpoints = True
        self.sessions = {}
        self.admitted_extra = {}
        self.phase = "finished"
        self.snapshot_terminal = None

    def tick(self, ms=1_000):
        self.t += ms
        return self.t

    def ev(self, body):
        self.events.append(body)

    def log(self, execution, line, stream="output", kind=None, ms=1_000):
        e = {"kind": "safe_log", "execution": execution, "timestamp": self.tick(ms), "stream": stream, "line": line}
        if kind and not self.stock:
            e["logKind"] = kind
        self.ev(e)

    def start_node(self, node, attempt=1, map_indices=(), model=None, harness=None, session=None, inp=None):
        self.exec_n += 1
        n = self.exec_n
        e = {"kind": "node_started", "reference": {"runId": self.id, "node": node, "nodeInstance": 1, "execution": n},
             "occurrence": {"node": node, "mapIndices": list(map_indices)}, "attempt": attempt, "input": inp or {"task": "Add slugify() to textkit."}}
        if not self.stock:
            e["at"] = self.tick(500)
            e["provider"] = {"harness": harness or self.rt.get("harness", "claude"), "model": model or self.rt.get("model"), "effort": "medium"}
            e["workspace"] = {"commit": f"{n:040x}"}
        self.ev(e)
        if session and not self.stock:
            self.log(n, json.dumps({"harness": harness or self.rt.get("harness", "claude"), "sessionId": session}), "system", "session", 200)
        return n

    def finish(self, n, node, outcome, files=None):
        e = {"kind": "node_completed", "completion": {"reference": {"runId": self.id, "node": node, "nodeInstance": 1, "execution": n}, "outcome": outcome}}
        if not self.stock:
            e["at"] = self.tick(500)
            e["workspace"] = {"commit": f"{n + 100:040x}", "base": f"{n:040x}", "files": files or [{"path": "textkit/core.py", "status": "modified", "additions": 12, "deletions": 1}]}
        self.ev(e)

    def usage(self, n, inp=1200, out=800, cr=40_000, cw=3_000, cost="0.0412", model=None):
        e = {"kind": "token_usage_observed", "execution": n, "usage": {"inputTokens": inp, "outputTokens": out, "cacheReadInputTokens": cr, "cacheCreationInputTokens": cw}}
        if not self.stock and cost is not None:
            e["costUsd"] = cost
            e["model"] = model or self.rt.get("model")
        self.ev(e)

    def work(self, n, steps=2):
        """A realistic turn: thinking, a tool call and its result, some text."""
        self.log(n, "I'll look at textkit/core.py first.", "system", "thinking")
        for k in range(steps):
            tid = f"toolu_{self.i:02d}{n:02d}{k:02d}"
            self.log(n, json.dumps({"id": tid, "name": "Bash", "input": {"command": f"python3 -m unittest -v  # step {k}"}}), "system", "tool_use")
            self.log(n, json.dumps({"toolUseId": tid, "content": f"Ran {k + 3} tests\n\nOK", "isError": False, "exitCode": 0}), "system", "tool_result")
        self.log(n, "Done: slugify() added with tests; ", "output", "text", 300)
        self.log(n, "all tests pass.", "output", "text", 300)

    def terminal(self, status, reason=None, context=None):
        r = {"status": status}
        if reason:
            r["reason"] = reason
        if status == "succeeded":
            r["output"] = None
        e = {"kind": "terminal", "result": r}
        if context:
            e["result"]["context"] = context
        self.ev(e)

    def verdict(self, verdict, message=None):
        return {"status": "verifier", "output": None, "signals": {"verdict": verdict}, "diagnostic": {"message": message} if message else None, "artifacts": []}

    def to_json(self):
        terminal = next((e["result"] for e in reversed(self.events) if e["kind"] == "terminal"), None)
        snapshot = {"runId": self.id, "title": self.title, "phase": self.phase, "cursor": f"v2:{len(self.events)}",
                    "terminal": self.snapshot_terminal if self.snapshot_terminal is not None else terminal, "forceStopRequested": any(e["kind"] == "force_stop_requested" for e in self.events)}
        admitted = {"title": self.title, "graph": self.graph, "initialInput": {"task": "Add slugify() to textkit."}, "runtime": self.rt,
                    "source": {"repository": "example/textkit", "branch": "main", "revision": "0" * 40}, **self.admitted_extra}
        return {"name": self.name, "runId": self.id, "where": self.where, "tags": self.tags, "stock": self.stock,
                "stored": {"admitted": admitted, "snapshot": snapshot},
                "events": [{"seq": k + 1, "body": e} for k, e in enumerate(self.events)],
                "recovery": self.recovery, "controllerPid": self.controller, "checkpoints": self.checkpoints, "sessions": self.sessions}


def scenario(fn):
    SCENARIOS[fn.__name__.replace("_", "-")] = fn
    return fn


def prelude(r):
    r.log(None, "Preparing execution environment", "system")
    r.ev({"kind": "run_started"})


def happy(r, accept=("accepted", "approved")):
    prelude(r)
    n = r.start_node("code", session=f"5e55{r.i:04d}-0000-4000-8000-000000000001")
    r.log(n, "Claude rate limit: status=allowed, type=five_hour, resetsAt=2026-11-18T13:00:00Z", "system", "rate_limit")
    r.work(n)
    r.usage(n)
    r.finish(n, "code", {"status": "verified", "output": None, "artifacts": []})
    a = r.start_node("acceptance")
    b = r.start_node("code_review", model="claude-sonnet-4-5")
    r.work(a, 1)
    r.usage(a, cost="0.0101")
    r.work(b, 1)
    r.usage(b, cost="0.0302", model="claude-sonnet-4-5")
    r.finish(a, "acceptance", r.verdict(accept[0]))
    r.finish(b, "code_review", r.verdict(accept[1]))
    return n


@scenario
def succeeded(i):
    r = Run("succeeded", i, graph_change(), runtime(), tags=["exec:succeeded", "exec:accepted", "cost:provider", "usage:ledger", "features:all"])
    happy(r)
    r.ev({"kind": "primary_graph_completed", "result": {"status": "succeeded", "output": None}, "auxiliary_pending": False})
    r.ev({"kind": "run_token_usage_observed", "usage": {"inputTokens": 3, "outputTokens": 150, "cacheReadInputTokens": 0, "cacheCreationInputTokens": 9000}})
    r.terminal("succeeded")
    return r


@scenario
def succeeded_stock(i):
    r = Run("succeeded-stock", i, graph_change(), runtime(), stock=True, tags=["stock", "times:approximate", "cost:estimate"])
    happy(r)
    r.terminal("succeeded")
    return r


@scenario
def work_exhausted(i):
    r = Run("work-exhausted", i, graph_change(rounds=2), runtime(), tags=["exec:rejected", "failure:work", "feedback", "exit-node"])
    prelude(r)
    for rnd in (1, 2):
        n = r.start_node("code")
        r.work(n, 1)
        r.usage(n)
        r.finish(n, "code", {"status": "verified", "output": None, "artifacts": []})
        a = r.start_node("acceptance")
        b = r.start_node("code_review")
        r.finish(a, "acceptance", r.verdict("rejected", f"Round {rnd}: slugify() keeps a trailing '-' for 'a b!'."))
        r.finish(b, "code_review", r.verdict("approved"))
    r.terminal("failed", "change_attempts_exhausted")
    return r


@scenario
def force_stopped(i):
    r = Run("force-stopped", i, graph_change(), runtime(), tags=["exec:interrupted", "failure:stopped", "refusal-as-stop"])
    prelude(r)
    n = r.start_node("code")
    r.work(n, 1)
    r.usage(n)
    r.finish(n, "code", {"status": "verified", "output": None, "artifacts": []})
    a = r.start_node("acceptance")
    b = r.start_node("code_review")
    r.log(a, "Reading the diff.", "output", "text")
    r.finish(b, "code_review", {"status": "error", "code": "refusal", "reason": "cancelled"})
    r.ev({"kind": "force_stop_requested"})
    r.log(None, "Force stop requested", "system")
    r.terminal("failed", "force_stopped")
    return r


@scenario
def stalled(i):
    r = Run("stalled", i, graph_change(), runtime(), tags=["status:stalled", "failure:orchestration", "controller:dead"])
    r.phase = "running"
    prelude(r)
    n = r.start_node("code")
    r.work(n, 1)
    r.controller = 4_194_303  # above the kernel's pid_max: never a live process
    return r


@scenario
def stalled_disk(i):
    r = Run("stalled-disk", i, graph_change(), runtime(), tags=["status:stalled", "runtime-cause:disk", "fix"])
    r.phase = "running"
    prelude(r)
    n = r.start_node("code")
    r.work(n, 1)
    r.log(None, "Runtime failed: write checkpoint: No space left on device (os error 28)", "error")
    return r


@scenario
def stalled_live_usage(i):
    r = Run("stalled-live-usage", i, graph_change(), runtime(), tags=["usage:session_log", "usage-live", "transcript:claude-native"])
    r.phase = "running"
    prelude(r)
    n = r.start_node("code", session="5e55ab1e-0000-4000-8000-00000000c1a0")
    r.work(n, 1)
    r.sessions["claude"] = {"execution": n, "sessionId": "5e55ab1e-0000-4000-8000-00000000c1a0"}
    return r


def runtime_terminal(name, i, reason, tags, primary=None, line=None):
    r = Run(name, i, graph_change(), runtime(), tags=tags)
    prelude(r)
    n = r.start_node("code")
    r.work(n, 1)
    if line:
        r.log(None, line, "error")
    r.terminal("failed", reason, {"primary": {"message": primary}} if primary else None)
    return r


@scenario
def runtime_lost(i):
    return runtime_terminal("runtime-lost", i, "runtime_lost", ["failure:orchestration", "runtime_lost"])


@scenario
def runtime_failed_storage(i):
    return runtime_terminal("runtime-failed-storage", i, "runtime_failed", ["runtime-cause:storage-bound"], primary="event payload exceeds the storage bound (1048576 bytes)")


@scenario
def runtime_failed_permission(i):
    return runtime_terminal("runtime-failed-permission", i, "runtime_failed", ["runtime-cause:permission"], line="Runtime failed: open /work/x/.git/index.lock: Permission denied")


@scenario
def runtime_failed_checkpoint(i):
    return runtime_terminal("runtime-failed-checkpoint", i, "runtime_unavailable", ["runtime-cause:checkpoint"], primary="restic backup failed: repository is locked")


@scenario
def runtime_failed_unknown(i):
    return runtime_terminal("runtime-failed-unknown", i, "runtime_failed", ["runtime-cause:none"], primary="Run failed: internal error")


@scenario
def checkout_unavailable(i):
    r = Run("checkout-unavailable", i, graph_change(), runtime(), tags=["failure:orchestration", "no-executions"])
    prelude(r)
    r.log(None, "git clone failed: repository not found", "error")
    r.terminal("failed", "source_checkout_unavailable")
    return r


@scenario
def delivery_failed(i):
    r = Run("delivery-failed", i, graph_change(), runtime(), tags=["failure:delivery"])
    happy(r)
    r.terminal("failed", "delivery_failed")
    return r


@scenario
def attempts_exhausted(i):
    r = Run("attempts-exhausted", i, graph_change(worker_attempts=2), runtime(), tags=["failure:agent", "attempts", "retries-of-a-visit"])
    prelude(r)
    for attempt in (1, 2):
        n = r.start_node("code", attempt=attempt)
        r.log(n, "Node code failed: crash after 12.3s: node execution failed: provider failure: something odd", "error")
        r.finish(n, "code", {"status": "error", "code": "crash", "reason": "provider_failure"})
    r.terminal("failed", "attempts_exhausted")
    return r


DIAGNOSES = {
    "credentials": ("API Error: 401 {\"type\":\"error\",\"error\":{\"type\":\"authentication_error\",\"message\":\"OAuth access token is invalid\"}}", "crash", None),
    "permission": ("API Error: 403 permission_error: There's an issue with the selected model (claude-opus-4-1-20250805). It may not exist or you may not have access to it.", "crash", None),
    "model": ("API Error: 404 {\"type\":\"error\",\"error\":{\"type\":\"not_found_error\",\"message\":\"model: claude-haiku-9-9\"}} issue with the selected model (claude-haiku-9-9)", "crash", None),
    "usage-limit": ("Claude AI usage limit reached|You've hit your limit · resets 9:30am (UTC)", "crash", None),
    "rate-limit": ("API Error: 429 {\"type\":\"error\",\"error\":{\"type\":\"rate_limit_error\"}}", "crash", None),
    "provider-down": ("API Error: 529 {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}", "crash", None),
    "stream-cut": ("StreamNoEventsError: stream ended unexpectedly", "crash", None),
    "provider-timeout": ("APIConnectionTimeoutError: Request timed out.", "crash", None),
    "malformed-stream": ("Unexpected end of stream: malformed API response", "crash", None),
    "context": ("API Error: 400 prompt is too long: 210000 tokens > 200000 maximum", "crash", None),
    "disk": ("ENOSPC: no space left on device, write", "crash", None),
    "network": ("Error: read ECONNRESET", "crash", None),
    "missing-cli": ("spawn claude: executable file not found in $PATH", "crash", None),
    "code-authentication-required": ("needs login", "error", "authentication_required"),
    "code-input-required": ("waiting for input", "error", "interactive_input_required"),
    "code-policy-denied": ("tool blocked", "error", "policy_denied"),
    "code-malformed-result": ("result did not match the schema", "malformed", "malformed_result"),
    "code-timeout": ("Node worker failed: timeout after 45.4s: node execution failed", "timeout", None),
    "code-crash": ("process exited with status 1", "crash", None),
    "code-refusal": ("I can't help with that.", "refusal", None),
    "code-other": ("something unexpected", "weird_code", None),
}


def make_diag(kind):
    def build(i):
        line, code, reason = DIAGNOSES[kind]
        harness = "claude"
        r = Run(f"diag-{kind}", i, graph_single(), runtime(nodes=("worker",)), tags=[f"diagnosis:{kind}", "exec:failed", "failure:agent-or-setup"])
        prelude(r)
        n = r.start_node("worker", harness=harness)
        r.work(n, 1)
        for part in line.split("|"):
            r.log(n, part, "error")
        r.finish(n, "worker", {"status": "error", "code": code, "reason": reason})
        r.terminal("failed", "worker_failed", {"recorded": [{"message": f"Node worker failed: {code} after 3.0s: node execution failed: provider failure: {line.split('|')[-1]}; provider process exited with status 1"}]})
        return r
    return build


for k in DIAGNOSES:
    SCENARIOS[f"diag-{k}"] = make_diag(k)


@scenario
def graph_ended_other(i):
    r = Run("graph-ended-other", i, graph_change(), runtime(), tags=["failure:work", "generic-reason"])
    happy(r)
    r.terminal("failed", "e2e_blocked")
    return r


@scenario
def closing_postmortem(i):
    r = Run("closing-postmortem", i, graph_change(), runtime(), tags=["closing", "background-agents", "postmortem", "deltas"])
    r.phase = "running"
    r.controller = None
    prelude(r)
    n = r.start_node("code")
    r.log(n, "Node code failed: crash after 2.0s: node execution failed: provider failure: API Error: 500 Internal Server Error", "error")
    r.finish(n, "code", {"status": "error", "code": "crash", "reason": "provider_failure"})
    r.ev({"kind": "primary_graph_completed", "result": {"status": "failed", "reason": "code_failed"}, "auxiliary_pending": True})
    r.log(None, "Active Memory postmortem started", "system", ms=10_000)
    for chunk in ["[postmortem] Rev", "[postmortem] iewing", "[postmortem]  the run"]:
        r.log(None, chunk, "output", "text", 200)
    r.log(None, "[postmortem] Claude API retry 1/10: overloaded_error", "system")
    r.log(None, "[curator] Claude rate limit: status=allowed_warning, type=seven_day, utilization=85%, resetsAt=2026-11-20T00:00:00Z", "system")
    r.log(None, "[curator] findings rejected: duplicate", "error")
    r.log(None, "Active Memory postmortem completed", "system")
    r.log(None, "Active Memory curator failed: findings rejected", "system")
    r.ev({"kind": "run_token_usage_observed", "usage": {"inputTokens": 10, "outputTokens": 500, "cacheReadInputTokens": 1000, "cacheCreationInputTokens": 2000}})
    r.snapshot_terminal = None
    return r


@scenario
def running_live(i):
    r = Run("running-live", i, graph_change(rounds=2, worker_attempts=2, timeout=45_000), runtime(), where="observe",
            tags=["status:running", "pressure:timeout", "pressure:attempts", "pressure:rounds", "pressure:retry", "pressure:rate_limit", "usage:pending"])
    r.phase = "running"
    r.start = NOW - 400_000
    r.t = r.start
    r.id = run_id(i, r.start)
    prelude(r)
    n = r.start_node("code")
    r.usage(n)
    r.finish(n, "code", {"status": "verified", "output": None, "artifacts": []})
    a = r.start_node("acceptance")
    r.finish(a, "acceptance", r.verdict("rejected", "needs tests"))
    b = r.start_node("code_review")
    r.finish(b, "code_review", r.verdict("approved"))
    r.t = NOW - 40_000
    n2 = r.start_node("code", attempt=1)
    r.log(n2, "Claude API retry 1/10: rate_limit", "system", "retry")
    r.log(n2, "doing work", "output", "text")
    r.log(n2, "Claude API retry 2/10: overloaded_error", "system", "retry")
    r.log(n2, "Claude rate limit: status=allowed_warning, type=five_hour, utilization=85%, resetsAt=2026-11-18T13:00:00Z", "system", "rate_limit")
    r.log(n2, "Claude rate limit: status=allowed_warning, type=seven_day, utilization=33.5%, resetsAt=2026-11-20T00:00:00Z", "system", "rate_limit")
    r.log(n2, "Claude rate limit: status=rejected, type=seven_day_opus, resetsAt=2026-11-20T00:00:00Z", "system", "rate_limit")
    r.finish(n2, "code", {"status": "error", "code": "crash", "reason": "provider_failure"})
    n3 = r.start_node("code", attempt=2)
    r.log(n3, "Claude API retry 3/10: unknown", "system", "retry")
    r.log(n3, "usage limit reached for this hour", "output")
    return r


@scenario
def running_stock(i):
    r = Run("running-stock", i, graph_single(), runtime(nodes=("worker",)), stock=True, where="observe", tags=["status:running", "stock", "transcript:unclassified"])
    r.phase = "running"
    r.start = NOW - 100_000
    r.t = r.start
    r.id = run_id(i, r.start)
    prelude(r)
    n = r.start_node("worker")
    r.log(n, "Thinking about it", "system")
    r.log(n, "Here is ", "output")
    r.log(n, "the plan.", "output")
    r.log(n, "Error: API Error: 503 Service Unavailable", "output")
    r.log(n, "Claude API retry 1/10: rate_limit", "system")
    return r


@scenario
def admitted_only(i):
    r = Run("admitted-only", i, graph_single(), runtime(nodes=("worker",)), where="observe", tags=["status:admitted", "no-events"])
    r.phase = "admitted"
    r.recovery = None
    return r


@scenario
def unknown_phase(i):
    r = Run("unknown-phase", i, graph_single(), runtime(nodes=("worker",)), where="observe", tags=["status:unknown"])
    r.phase = ""
    r.log(None, "hello", "system")
    return r


@scenario
def voided(i):
    r = Run("voided", i, graph_change(), runtime(), tags=["exec:voided"])
    prelude(r)
    n = r.start_node("code")
    r.ev({"kind": "execution_voided", "reference": {"runId": r.id, "node": "code", "nodeInstance": 1, "execution": n}, "reason": "superseded_by_checkpoint_restore"})
    m = r.start_node("code")
    r.work(m, 1)
    r.usage(m)
    r.finish(m, "code", {"status": "verified", "output": None, "artifacts": []})
    r.terminal("succeeded")
    return r


@scenario
def map_parallel(i):
    r = Run("map-parallel", i, graph_map(), runtime(nodes=("fix", "summarize")), tags=["map-indices", "parallel"])
    prelude(r)
    a = r.start_node("fix", map_indices=[0])
    b = r.start_node("fix", map_indices=[1])
    r.work(a, 1)
    r.work(b, 1)
    r.usage(a)
    r.usage(b)
    r.finish(a, "fix", {"status": "verified", "output": {"file": "a.py"}, "artifacts": []})
    r.finish(b, "fix", {"status": "verified", "output": {"file": "b.py"}, "artifacts": []})
    s = r.start_node("summarize")
    r.log(s, "```json\n{\"summary\": \"fixed 2 files\"}\n```", "output", "text")
    r.log(s, json.dumps({"id": "toolu_so", "name": "StructuredOutput", "input": {"summary": "fixed 2 files"}}), "system", "tool_use")
    r.log(s, json.dumps({"toolUseId": "toolu_so", "content": "Structured output provided successfully"}), "system", "tool_result")
    r.finish(s, "summarize", {"status": "verified", "output": {"summary": "fixed 2 files"}, "artifacts": []})
    r.terminal("succeeded")
    return r


@scenario
def steered(i):
    r = Run("steered", i, graph_single(), runtime(nodes=("worker",)), tags=["fork-only", "steer:delivered", "steer:queued", "steer:undelivered"])
    prelude(r)
    n = r.start_node("worker")
    r.log(n, "Running sleep 20.", "output", "text")
    r.log(n, json.dumps({"message": "Write BANANA instead of APPLE.", "status": "delivered"}), "system", "steer")
    r.log(n, json.dumps({"message": "Also add a test.", "status": "queued"}), "system", "steer")
    r.log(n, json.dumps({"message": "Too late.", "status": "undelivered"}), "system", "steer")
    r.log(n, "Understood: writing BANANA.", "output", "text")
    r.usage(n)
    r.finish(n, "worker", {"status": "verified", "output": None, "artifacts": []})
    r.terminal("succeeded")
    return r


@scenario
def replay_child(i):
    r = Run("replay-child", i, graph_change(), runtime(), tags=["fork-only", "replay", "reused", "prior_execution"])
    parent = SCENARIOS_BUILT["replay-parent"]
    r.admitted_extra["replayOf"] = {"runId": parent.id, "fromCheckpoint": "ckpt-0002", "changed": ["runtime"]}
    r.recovery["resumedFrom"] = parent.id
    prelude(r)
    r.exec_n = 1
    r.ev({"kind": "prior_execution", "execution": {"execution": 1, "node_instance": 1, "attempt": 1, "occurrence": {"node": "code", "mapIndices": []},
                                                    "input": {"task": "Add slugify() to textkit."}, "state": {"Settled": {"outcome": {"status": "verified", "output": None, "artifacts": []}}}}})
    a = r.start_node("acceptance")
    b = r.start_node("code_review")
    r.finish(a, "acceptance", r.verdict("accepted"))
    r.finish(b, "code_review", r.verdict("approved"))
    r.usage(a, cost="0.01")
    r.terminal("succeeded")
    return r


@scenario
def replay_parent(i):
    r = Run("replay-parent", i, graph_change(), runtime(), tags=["fork-only", "replays"])
    happy(r, accept=("rejected", "approved"))
    r.terminal("failed", "change_attempts_exhausted")
    return r


@scenario
def resumed_original(i):
    r = Run("resumed-original", i, graph_change(), runtime(), tags=["resume", "lost-before-resume", "runtime_lost"])
    prelude(r)
    n = r.start_node("code")
    r.work(n, 2)
    r.t += 600_000
    r.terminal("failed", "runtime_lost")
    return r


@scenario
def stalled_then_closed(i):
    """A run whose controller died while the agent worked, closed minutes later when it was
    resumed: Zeroshot then fails the open execution (`crash: run runtime_lost`) and closes the run,
    stamped at the close. Its tokens come from Claude's session log (no session line, as Zeroshot
    16.1 writes it: found by time); the successor's session starts in the same workspace after the
    agent was last active."""
    r = Run("stalled-then-closed", i, graph_change(), runtime(), tags=["resume", "lost-before-resume", "runtime_lost", "usage:session_log"])
    prelude(r)
    n = r.start_node("code")
    node_start = r.t
    r.work(n, 2)
    r.t = node_start + 170_000
    r.log(n, "Node code failed: crash: run runtime_lost", "error", ms=0)
    r.finish(n, "code", {"status": "error", "code": "crash", "reason": "declared_failure"})
    r.log(None, "Run failed: runtime_lost", "error", ms=0)
    r.terminal("failed", "runtime_lost")
    r.sessions["claude"] = {"execution": n, "sessionId": "5e55c105-0000-4000-8000-0000000000c1"}
    r.sessions["claude-successor"] = {"execution": n, "sessionId": "5e55c105-0000-4000-8000-0000000000c2", "start": node_start + 175_000}
    return r


@scenario
def tool_output_rate_limit(i):
    """A live run whose tool output and agent text are full of rate-limit words (an agent testing
    an HTTP client): no pressure from them; Zeroshot's own rate-limit notice still is one."""
    r = Run("tool-output-rate-limit", i, graph_single(timeout=3_600_000), runtime(nodes=("worker",)), where="observe",
            tags=["status:running", "pressure:rate_limit"])
    r.phase = "running"
    r.start = NOW - 60_000
    r.t = r.start
    r.id = run_id(i, r.start)
    prelude(r)
    n = r.start_node("worker")
    echo = "echo \"HTTP 429 rate_limit_error: You have hit your usage limit. Rate limit exceeded, retry-after 60\""
    r.log(n, "Claude tool call: Bash id=toolu_013Q1Cc1gwLu5V3gMSPuHaJu status=unknown\n" + json.dumps({"command": echo}), "system")
    r.log(n, "Claude tool completed: Bash id=toolu_013Q1Cc1gwLu5V3gMSPuHaJu status=success\n"
          "HTTP 429 rate_limit_error: You have hit your usage limit. Rate limit exceeded, retry-after 60", "system")
    r.log(n, "The client now handles 429 Too Many Requests and overloaded_error with a retry.", "output")
    r.log(n, "Claude API retry 1/3: rate_limit (quoted by the agent)", "output")
    r.log(n, "Claude rate limit: status=rejected, type=five_hour, resetsAt=2026-11-18T13:00:00Z", "system")
    return r


@scenario
def diag_api_hang(i):
    """The lab's chaos-hang (model request 16 accepted and never answered) as the fork records it:
    the worker works, a tool result goes back to the model, no answer comes, and its 2 minute
    timeout ends it. The provider's fault, not the node's limit."""
    r = Run("diag-api-hang", i, graph_single(timeout=120_000), runtime(nodes=("worker",)),
            tags=["diagnosis:api-hang", "exec:failed", "failure:provider"])
    prelude(r)
    n = r.start_node("worker", session=f"5e55{i:04d}-0000-4000-8000-00000000a4a6")
    r.work(n, 2)
    tid = f"toolu_{i:02d}{n:02d}99"
    r.log(n, json.dumps({"id": tid, "name": "Read", "input": {"file_path": "/work/diag-api-hang/repo/textkit/core.py"}}), "system", "tool_use", 2_000)
    r.log(n, "Claude rate limit: status=allowed, type=seven_day, resetsAt=2026-11-25T02:00:00Z", "system", "rate_limit", 50)
    r.log(n, json.dumps({"toolUseId": tid, "content": "1\tdef count_words(text: str) -> int:", "isError": False}), "system", "tool_result", 50)
    r.usage(n)
    # Nothing more from the model until Zeroshot's node timeout.
    r.log(n, "Node worker failed: timeout after 120.0s", "error", ms=95_000)
    r.finish(n, "worker", {"status": "error", "code": "timeout", "reason": "declared_failure"})
    r.terminal("failed", "worker_failed", {"recorded": [{"message": "Node worker failed: timeout after 120.0s"}]})
    return r


@scenario
def resumed_successor(i):
    r = Run("resumed-successor", i, graph_change(), runtime(), tags=["resume", "successor"])
    orig = SCENARIOS_BUILT["resumed-original"]
    r.recovery["resumedFrom"] = orig.id
    happy(r)
    r.terminal("succeeded")
    return r


@scenario
def codex_estimate(i):
    r = Run("codex-estimate", i, graph_single(), runtime(harness="codex", model="gpt-5-codex", nodes=("worker",)), tags=["harness:codex", "cost:estimate"])
    prelude(r)
    n = r.start_node("worker")
    r.log(n, "Working.", "output", "text")
    r.ev({"kind": "token_usage_observed", "execution": n, "usage": {"inputTokens": 50_000, "outputTokens": 3_000, "cacheReadInputTokens": 40_000}})
    r.finish(n, "worker", {"status": "verified", "output": None, "artifacts": []})
    r.terminal("succeeded")
    return r


@scenario
def copilot_session(i):
    r = Run("copilot-session", i, graph_single(), runtime(harness="copilot", model="gpt-5", nodes=("worker",)), tags=["harness:copilot", "transcript:copilot"])
    prelude(r)
    n = r.start_node("worker", harness="copilot", model="gpt-5", session="c0b1107-session-0001")
    r.log(n, "Working.", "output", "text")
    r.finish(n, "worker", {"status": "verified", "output": None, "artifacts": []})
    r.terminal("succeeded")
    r.sessions["copilot"] = {"execution": n, "sessionId": "c0b1107-session-0001"}
    return r


@scenario
def no_checkpoints(i):
    r = Run("no-checkpoints", i, graph_single(), runtime(nodes=("worker",)), tags=["actions:no-replay"])
    r.checkpoints = False
    prelude(r)
    n = r.start_node("worker")
    r.log(n, "Node worker failed: timeout after 45.4s: node execution failed", "error")
    r.finish(n, "worker", {"status": "error", "code": "timeout", "reason": None})
    r.terminal("failed", "worker_failed")
    return r


@scenario
def lane_mixed(i):
    """Zeroshot 16.1 (stock) with per-node lanes: a Codex worker on OpenAI, Claude reviewers on
    the run's default lane (Anthropic). The worker fails on an unknown model id."""
    rt = runtime(nodes=("code", "acceptance", "code_review"))
    rt["nodes"]["code"] = {"kind": "agent", "model": "gpt-5.1-codex-does-not-exist", "lane": {"harness": "codex", "provider": "openai"},
                           "connections": {"openai": ["OPENAI_API_KEY"]}}
    r = Run("lane-mixed", i, graph_change(rounds=1), rt, stock=True, tags=["stock", "lane:mixed", "harness:codex", "failure:setup"])
    prelude(r)
    n = r.start_node("code")
    r.log(n, "Working.", "output", "text")
    r.usage(n, cost=None)
    r.log(n, "Node code failed: crash after 2.1s: node execution failed: Codex provider failure: unknown model gpt-5.1-codex-does-not-exist", "error")
    r.finish(n, "code", {"status": "error", "code": "crash", "reason": None})
    r.terminal("failed", "code_failed")
    return r


@scenario
def lane_unknown_harness(i):
    """A harness zerochat does not know (`hermes`) on a provider it does not know: shown as they
    are, never as Claude; the reviewer is rate limited by "the provider"."""
    rt = runtime(model="hermes-4-405b", nodes=("worker",), connections=False)
    rt["harness"], rt["provider"] = "hermes", "nous"
    r = Run("lane-unknown-harness", i, graph_single(), rt, stock=True, tags=["stock", "harness:unknown", "failure:agent"])
    prelude(r)
    n = r.start_node("worker")
    r.log(n, "Working.", "output", "text")
    r.usage(n, cost=None)
    r.log(n, "Node worker failed: crash after 3.0s: node execution failed: Hermes provider failure: API Error: 429 rate limit exceeded", "error")
    r.finish(n, "worker", {"status": "error", "code": "crash", "reason": None})
    r.terminal("failed", "worker_failed")
    return r


def iso(ms):
    import datetime
    return datetime.datetime.fromtimestamp(ms / 1000, datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.") + f"{ms % 1000:03d}Z"


def claude_session(session_id, cwd, start):
    """A Claude Code session log (`~/.claude/projects/<slug>/<id>.jsonl`), shapes copied from
    real logs: the prompt, thinking, text, tool use and results (one an error), a steer the
    person sent, an interruption, a synthetic API-error message, a subagent's sidechain and usage
    repeated on every block of one message (counted once)."""
    t = [start]

    def at(step=1_000):
        t[0] += step
        return iso(t[0])

    def usage(i, o, cr, cw):
        return {"input_tokens": i, "output_tokens": o, "cache_read_input_tokens": cr, "cache_creation_input_tokens": cw}

    def assistant(msg_id, block, u, side=False, **extra):
        return {"parentUuid": None, "isSidechain": side, "type": "assistant", "sessionId": session_id, "cwd": cwd, "timestamp": at(),
                "message": {"model": "claude-haiku-4-5", "id": msg_id, "type": "message", "role": "assistant", "content": [block], "usage": u}, **extra}

    def user(content, side=False):
        return {"parentUuid": None, "isSidechain": side, "type": "user", "sessionId": session_id, "cwd": cwd, "timestamp": at(), "message": {"role": "user", "content": content}}

    u1 = usage(3, 120, 12_000, 4_000)
    lines = [
        {"type": "queue-operation", "operation": "enqueue", "timestamp": at(0), "sessionId": session_id},
        user("Add slugify() to textkit.\n\nReturn the result in the required format."),
        {"type": "attachment", "attachment": {"type": "environment"}, "timestamp": at(10), "sessionId": session_id},
        assistant("msg_01", {"type": "thinking", "thinking": "Look at textkit/core.py first.", "signature": "sig"}, u1),
        assistant("msg_01", {"type": "text", "text": "I'll read the module."}, u1),
        assistant("msg_01", {"type": "tool_use", "id": "toolu_c1", "name": "Read", "input": {"file_path": cwd + "/textkit/core.py"}}, u1),
        user([{"tool_use_id": "toolu_c1", "type": "tool_result", "content": "def title(text):\n    return text.title()\n", "is_error": False}]),
        assistant("msg_02", {"type": "tool_use", "id": "toolu_c2", "name": "Bash", "input": {"command": "python3 -m unittest -v", "description": "Run the tests"}}, usage(2, 60, 16_000, 500)),
        user([{"tool_use_id": "toolu_c2", "type": "tool_result", "content": [{"type": "text", "text": "FAILED (failures=1)"}], "is_error": True}]),
        user("Message from the user, sent while you were working. They read your messages live: answer in a message:\nUse re.sub, not a loop."),
        user([{"type": "text", "text": "[Request interrupted by user]"}]),
        user("[zeroshot] Continue: the structured result is still missing."),
        assistant("msg_03", {"type": "text", "text": "API Error: 529 {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\"}}"}, usage(0, 0, 0, 0), isApiErrorMessage=True),
        user("Search the repo for callers of title().", side=True),
        assistant("msg_s1", {"type": "tool_use", "id": "toolu_s1", "name": "Grep", "input": {"pattern": "title("}}, usage(5, 30, 1_000, 0), side=True),
        user([{"tool_use_id": "toolu_s1", "type": "tool_result", "content": "textkit/__init__.py:1"}], side=True),
        {"type": "system", "subtype": "compact_boundary", "content": "Conversation compacted", "timestamp": at(), "sessionId": session_id},
        assistant("msg_04", {"type": "text", "text": "Done: slugify() uses re.sub and the tests pass."}, usage(2, 90, 20_000, 300)),
        assistant("msg_05", {"type": "text", "text": "There's an issue with the selected model (claude-haiku-9-9)."}, usage(0, 0, 0, 0)),
    ]
    return "\n".join(json.dumps(l) for l in lines) + "\n"


def copilot_session(session_id, cwd, start):
    """A GitHub Copilot CLI session (`session-state/<id>/events.jsonl`), every event kind the
    reader handles plus ones it ignores."""
    t = [start]

    def ev(kind, data, **extra):
        t[0] += 700
        return {"type": kind, "data": data, "id": f"ev-{t[0]}", "timestamp": iso(t[0]), "parentId": None, **extra}

    lines = [
        ev("session.start", {"sessionId": session_id, "version": 1, "producer": "copilot-agent", "copilotVersion": "1.0.68", "startTime": iso(start), "context": {"cwd": cwd}}),
        ev("session.model_change", {"newModel": "gpt-5"}),
        ev("system.message", {"role": "system", "content": "You are GitHub Copilot"}),
        ev("user.message", {"content": "Execute this graph node: add slugify() to textkit.", "delivery": "idle"}),
        ev("assistant.turn_start", {"turnId": "0"}),
        ev("assistant.message", {"content": "", "reasoningText": "Look around first.", "toolRequests": [{"toolCallId": "t1", "name": "bash", "arguments": {"command": "git status"}}]}),
        ev("session.usage_record", {"usage": {"model": "gpt-5", "inputTokens": 1200, "outputTokens": 80, "cacheReadTokens": 400, "cost": 1.0, "copilotUsage": {"totalNanoAiu": 2_500_000_000}}}),
        ev("tool.execution_start", {"toolCallId": "t1", "toolName": "bash", "arguments": {"command": "git status", "description": "Inspect"}}),
        ev("permission.requested", {"requestId": "p1", "permissionRequest": {"kind": "shell", "toolCallId": "t1", "fullCommandText": "git status"}}),
        ev("permission.completed", {"requestId": "p1", "toolCallId": "t1", "result": {"kind": "approved"}}),
        ev("tool.execution_complete", {"toolCallId": "t1", "success": True, "result": {"content": "On branch main"}, "shellExecution": {"exitCode": 0}}),
        ev("tool.execution_start", {"toolCallId": "t2", "toolName": "view", "arguments": {"path": cwd + "/missing.md"}}),
        ev("permission.requested", {"requestId": "p2", "permissionRequest": {"kind": "read", "toolCallId": "t2"}}),
        ev("permission.completed", {"requestId": "p2", "toolCallId": "t2", "result": {"kind": "denied-by-rules"}}),
        ev("tool.execution_complete", {"toolCallId": "t2", "success": False, "error": {"message": "Path does not exist"}}),
        ev("subagent.started", {"toolCallId": "t3", "agentName": "general-purpose", "model": "claude-haiku-4.5"}, agentId="t3"),
        ev("tool.execution_start", {"toolCallId": "t4", "toolName": "grep", "arguments": {"pattern": "title"}}, agentId="t3"),
        ev("tool.execution_complete", {"toolCallId": "t4", "success": True, "result": {"content": "textkit/core.py:1"}}, agentId="t3"),
        ev("subagent.completed", {"toolCallId": "t3", "agentName": "general-purpose", "totalToolCalls": 1}, agentId="t3"),
        ev("session.compaction_start", {"currentTokens": 160000}),
        ev("session.compaction_complete", {"success": True, "preCompactionTokens": 142641, "postCompactionTokens": 57200, "summaryContent": "<overview>Added slugify</overview>"}),
        ev("user.message", {"content": "Message from the user, sent while you were working. They read your messages live: answer in a message:\nUse re.sub.", "delivery": "steering"}),
        ev("user.message", {"content": "[structured-output-enforce] Answer with the schema.", "delivery": "idle"}),
        ev("system.notification", {"content": "<system_notification>x</system_notification>", "kind": {"type": "shell_completed", "shellId": "tests", "exitCode": 1, "description": "Run the tests"}}),
        ev("system.notification", {"content": "<system_notification>y</system_notification>", "kind": {"type": "agent_completed", "agentId": "helper", "status": "completed", "description": "Find callers"}}),
        ev("session.error", {"errorType": "query", "message": "API Error: 503 Service Unavailable"}),
        ev("hook.start", {"hookType": "preToolUse"}),
        ev("assistant.message", {"content": "{\"response\":{\"ok\":true}}"}),
        ev("assistant.turn_end", {"turnId": "0"}),
        ev("session.shutdown", {"shutdownType": "routine", "totalPremiumRequests": 1}),
    ]
    return "\n".join(json.dumps(l) for l in lines) + "\n"


def write_sessions():
    """Session logs for the scenarios that name one, under ledgers/home/ (the tests' HOME)."""
    home = OUT.parent / "home"
    if home.exists():
        import shutil
        shutil.rmtree(home)
    for name, r in SCENARIOS_BUILT.items():
        ws = r.recovery["workspace"] if r.recovery else None
        for kind, info in r.sessions.items():
            start = info.get("start") or next(e["at"] for e in r.events if e["kind"] == "node_started" and e["reference"]["execution"] == info["execution"])
            if kind.startswith("claude"):
                d = home / "dot-claude/projects" / "".join(c if c.isascii() and c.isalnum() else "-" for c in ws)
                d.mkdir(parents=True, exist_ok=True)
                (d / f"{info['sessionId']}.jsonl").write_text(claude_session(info["sessionId"], ws, start))
            else:
                d = home / "dot-copilot/session-state" / info["sessionId"]
                d.mkdir(parents=True, exist_ok=True)
                (d / "events.jsonl").write_text(copilot_session(info["sessionId"], ws, start))


SCENARIOS_BUILT = {}
ORDER = ["replay-parent", "resumed-original"]
# Scenarios added later take the next indices (run ids and start times), so the earlier ones keep
# theirs.
LATER = ["lane-mixed", "lane-unknown-harness", "stalled-then-closed", "tool-output-rate-limit", "diag-api-hang"]


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    for old in OUT.glob("*.json"):
        old.unlink()
    names = ORDER + [n for n in SCENARIOS if n not in ORDER]
    for idx, name in enumerate(sorted(names, key=lambda n: (n not in ORDER, n in LATER, LATER.index(n) if n in LATER else 0, n))):
        r = SCENARIOS[name](idx + 1)
        SCENARIOS_BUILT[name] = r
    # Lineage the other way round: the original names its successor and its replay.
    SCENARIOS_BUILT["resumed-original"].recovery["successorRunId"] = SCENARIOS_BUILT["resumed-successor"].id
    # Any existing run will do as its successor: what matters is that it was resumed.
    SCENARIOS_BUILT["stalled-then-closed"].recovery["successorRunId"] = SCENARIOS_BUILT["resumed-successor"].id
    SCENARIOS_BUILT["replay-parent"].recovery["replays"] = [SCENARIOS_BUILT["replay-child"].id]
    SCENARIOS_BUILT["replay-parent"].recovery["successorRunId"] = SCENARIOS_BUILT["replay-child"].id
    SCENARIOS_BUILT["succeeded"].sessions["claude"] = {"execution": 1, "sessionId": f"5e55{SCENARIOS_BUILT['succeeded'].i:04d}-0000-4000-8000-000000000001"}
    write_sessions()
    for name, r in SCENARIOS_BUILT.items():
        (OUT / f"{name}.json").write_text(json.dumps(r.to_json(), indent=1, ensure_ascii=False) + "\n")
    print(f"{len(SCENARIOS_BUILT)} scenarios -> {OUT}", file=sys.stderr)


if __name__ == "__main__":
    main()
