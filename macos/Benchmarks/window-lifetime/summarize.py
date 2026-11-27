"""Validate a window benchmark run and summarize all untrimmed samples."""
import argparse
import json
import pathlib
import statistics

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("directory", type=pathlib.Path)
args = parser.parse_args()
summary = {}
for variant in ["rust", "kept", "released"]:
    rows = json.loads((args.directory / f"{variant}.json").read_text())
    if len(rows) < 5:
        raise RuntimeError(f"{variant}: fewer than five timing samples")
    result = {"samples": rows, "memory": {}}
    for phase in ["cold", "warm"]:
        values = [row[f"{phase}_ms"] for row in rows]
        result[f"{phase}_ms"] = {"median": statistics.median(values), "max": max(values)}
    for phase in ["never-opened", "open", "after-close"]:
        result["memory"][phase] = json.loads(
            (args.directory / "memory" / f"{variant}-{phase}.json").read_text())
    for row in rows:
        trace = (args.directory / f"{variant}-{row['replicate']}.trace").read_text()
        required = ["open ", "display ", "frame "]
        if variant != "rust":
            required.append("popover-shown true")
        if variant == "kept":
            required.append("window-hidden")
        if variant == "released":
            required.extend(["popover-released", "window-released"])
        if not all(event in trace for event in required):
            raise RuntimeError(f"{variant}: missing live frame or lifecycle evidence")
    summary[variant] = result
    footprints = [value["footprint_MB"] for value in result["memory"].values()]
    print(variant, "MB:", footprints, "cold:", result["cold_ms"], "warm:", result["warm_ms"])
(args.directory / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
