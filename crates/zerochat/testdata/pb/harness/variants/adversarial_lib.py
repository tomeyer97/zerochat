"""Shared pieces for the adversarial harness variants: prompts, node builders, and the review loop.

Round policy (zeroshot loops expose no iteration counter, so the round lives in a
findings LEDGER that triage writes into graph state each round and every node reads):
  * rounds 1-2: reviewer and bugfinder look for new issues;
  * round 3+:   they only re-check the ledger's OPEN findings; a NEW finding is
                reported only if it is CRITICAL;
  * only MEDIUM+ severity findings are ever reported or fixed.
"""

import json
from pathlib import Path

MODEL = "claude-haiku-4-5"
MAX_ROUNDS = 4

STR = {"type": {"kind": "string"}, "required": True}
ERR = ["crash", "malformed", "refusal", "timeout"]
STATE = {"kind": "record", "fields": {"task": STR, "reviewFeedback": STR, "bugReport": STR, "ledger": STR}}

REFERENCE = (
    "A pristine, execute-only copy of the reference program is at /opt/pb/reference/executable. "
    "It survives rebuilds (./compile.sh overwrites ./executable), so always compare against that path."
)

SEVERITY = """SEVERITY (report only MEDIUM, HIGH or CRITICAL; never report LOW):
- CRITICAL: crash, hang, or garbage output on common input; a core documented mode entirely missing; build broken.
- HIGH: wrong stdout or exit code for a documented feature or flag on ordinary input.
- MEDIUM: wrong stdout or exit code on an edge case, unusual-but-valid input, or a flag combination.
- LOW (do not report): differences only in stderr wording, help/usage text, or…[fixture: 10233 more characters cut]