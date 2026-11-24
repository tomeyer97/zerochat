"""haiku-adversarial: worker -> loop{ par(reviewer, bugfinder) -> triage -> done | repair }.

Prompts and the review loop live in ../adversarial_lib.py (shared with other variants).
Edit there or here, then run `python3 build.py` to regenerate graph.json and runtime.json.
"""

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))
from adversarial_lib import REPAIR, STATE, WORKER, graph_doc, guarded, review_loop, runtime_doc, step, write  # noqa: E402

graph = graph_doc({"kind": "seq", "name": "run", "state": STATE, "promotedStatePaths": [], "children": [
    step("worker", "builtin.agent.software-worker@1", WORKER, ["task"]),
    guarded("worker_result", "worker", STATE, review_loop(STATE, REPAIR)),
]})

write(HERE, graph, runtime_doc(["worker", "reviewer", "bugfinder", "triage", "repair"]))
