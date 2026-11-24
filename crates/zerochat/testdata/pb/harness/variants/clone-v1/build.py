"""clone-v1: explore -> build -> loop{ coverage-audit -> bugfinder -> triage -> done | repair }.

A graph for reference-clone tasks ("re-implement this program from its reference binary and docs"):
general across such tasks, NOT general software work - it relies on the reference as the oracle.
What it encodes (each from a measured result in this repo's runs):
  * explore first, but write a VERIFIED spec instead of a throwaway prototype: every example is
    captured by running the reference, so the spec cannot contain remembered/wrong examples;
  * the spec includes an executable case suite that every later node must keep passing;
  * check feature/dimension COVERAGE before hunting bugs (the highest-yield check measured);
  * probe generic behaviour dimensions (errors, environment, invocation name, TTY, scale,
    mode composition) that agents skip but hidden tests exercise;
  * findings and repairs work at the level of behaviour RULES, never single repros;
  * a coverage ledger drives later rounds; new findings only in rounds 1-2 (then critical only).
Nothing here names a specific program, flag or format.
"""

import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))
from adversarial_lib import (  # noqa: E402
    ERR, FINDING_FORMAT, REFERENCE, SEVERITY, STR, bind, errored, graph_doc, guarded, private_build, rec, runtime_doc,
    step, write,
)

MAX_ROUNDS = 3
STATE = {"kind": "record", "fields": {"task": ST…[fixture: 12971 more characters cut]