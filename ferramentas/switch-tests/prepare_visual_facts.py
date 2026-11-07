#!/usr/bin/env python3
"""Pack the pinned Java gameplay facts used by the Switch client."""
import json
import sys
from pathlib import Path

here = Path(__file__).resolve().parent
root = here.parents[1]
sources = [
    root / ".local/horizon-client/26.3/gameplay-facts.json",
    here / "out/preparation/visual-facts/gameplay-facts.json",
]
source = next((path for path in sources if path.is_file()), None)
if source is None:
    raise SystemExit("Run VanillaGameplayFactsExporter for Java 26.3 (see REPORT.md)")
rows = json.loads(source.read_bytes())["states"]
if any(state["id"] != i for i, state in enumerate(rows)):
    raise SystemExit("Unexpected order of exported block state IDs")
stage = Path(sys.argv[1])
packed = [[state["hardness"], state["requires_tool"], state["emission"]] for state in rows]
(stage / "visual-state-facts.json").write_text(json.dumps(packed, separators=(",", ":")), encoding="utf-8")
