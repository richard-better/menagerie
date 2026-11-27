"""Capture physical footprint and eleven one-second CPU deltas for one PID."""
import argparse
import datetime
import json
import pathlib
import re
import statistics
import subprocess
import time

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("pid", type=int)
parser.add_argument("phase")
parser.add_argument("settle", type=float, nargs="?", default=30)
parser.add_argument("--directory", type=pathlib.Path, default=pathlib.Path("target/benchmarks"))
args = parser.parse_args()
args.directory.mkdir(parents=True, exist_ok=True)
print(f"Settling {args.phase}, PID {args.pid}, {args.settle} seconds", flush=True)
time.sleep(args.settle)
base = args.directory / args.phase
footprint = subprocess.check_output(["footprint", "-p", str(args.pid)], stderr=subprocess.STDOUT).decode()
base.with_suffix(".footprint.txt").write_text(footprint)
top = subprocess.check_output([
    "top", "-l", "12", "-s", "1", "-pid", str(args.pid), "-stats", "pid,command,cpu,mem"
]).decode()
base.with_suffix(".top.txt").write_text(top)
match = re.search(r"phys_footprint:\s*([\d.]+)\s*(KB|MB|GB)", footprint)
if match is None:
    raise RuntimeError("No physical footprint in footprint output")
cpus = [float(value) for value in re.findall(rf"^{args.pid}\s+\S+\s+([\d.]+)", top, re.M)][1:]
if len(cpus) != 11:
    raise RuntimeError(f"Expected eleven CPU deltas, received {len(cpus)}")
result = {
    "phase": args.phase,
    "pid": args.pid,
    "timestamp": datetime.datetime.now(datetime.timezone.utc).isoformat(),
    "footprint_MB": float(match[1]) * {"KB": 1 / 1024, "MB": 1, "GB": 1024}[match[2]],
    "cpu_samples": cpus,
    "cpu_mean": statistics.mean(cpus),
    "cpu_median": statistics.median(cpus),
    "cpu_max": max(cpus),
}
base.with_suffix(".json").write_text(json.dumps(result, indent=2))
print(json.dumps(result), flush=True)
