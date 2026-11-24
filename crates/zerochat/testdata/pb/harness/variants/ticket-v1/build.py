"""ticket-v1: intake -> recon -> test-first -> build -> loop{ traceability -> adversary -> quality -> triage -> deliver | repair }.

A general software-change graph: the input is a ticket (any task text). There is no assumed
reference implementation; ground truth is assembled from what real work has:
  * the ticket -> numbered acceptance criteria, written as tests BEFORE the code;
  * the existing system -> characterization tests that pin today's behaviour where the ticket
    must not change it (and, if the ticket names a program/behaviour to match, that is the oracle);
  * the project's own tests/CI -> a baseline that must stay green;
  * invariants/properties -> the adversary's oracle instead of output diffs;
  * a human -> ambiguities are recorded as assumptions and NEEDS-DECISION items, never guessed silently.
The review loop keeps what measured well in clone-v1: coverage first, sequential review,
rule-level findings, an executable regression gate, a coverage ledger and a round policy.
"""

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))
from adversarial_lib import STR, bind, errored, graph_doc, guarded, rec, runtime_doc, step, write  # noqa: E402

MAX_ROUNDS = 3
VERIFIER_INPUTS = ["task", "traceReport", "advReport", "qualityReport", "ledger"]
STATE = {"kind": "record", "fields": {f: STR for f in VERIFIER_INPUTS}}

WORK = """Working notes live in work/ at the repository root (commit them): work/ACCEPTANCE.m…[fixture: 14841 more characters cut]