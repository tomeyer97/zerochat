"""haiku-prototype-plan: prototype -> planner -> worker(plan) -> loop{ par(reviewer, bugfinder) -> triage -> done | repair }.

Idea: a fast throwaway prototype discovers the real requirements (formats, edge cases, what is
hard). A planner studies it against the reference, critiques it, and writes a self-contained
implementation plan, then removes the prototype. The worker re-implements from scratch with
that plan, and the same adversarial review loop as haiku-adversarial takes it from there.

The review loop and its prompts are shared with haiku-adversarial (../adversarial_lib.py), so
the only difference between the two experiments is the prototype -> planner stage.
"""

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))
from adversarial_lib import (  # noqa: E402
    REFERENCE, REPAIR, STR, graph_doc, guarded, private_build, rec, review_loop, runtime_doc, step, write,
)

STATE = {"kind": "record", "fields": {"task": STR, "plan": STR, "reviewFeedback": STR, "bugReport": STR, "ledger": STR}}

# Archive the prototype outside the workspace and restore the workspace to the task's original
# files (tested in a task container: tree ends identical to the initial commit, docs kept,
# ./executable is again the symlink to the reference).
RESET = (
    "mkdir -p /tmp/prototype && tar -C /workspace --exclude=./.git --exclude=./executable -cf - . | tar -C /tmp/prototype -xf - "
    "&& cd /workspace && ROOT=$(git rev-list --max…[fixture: 5509 more characters cut]