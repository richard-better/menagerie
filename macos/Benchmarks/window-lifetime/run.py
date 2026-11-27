"""Benchmark only child processes launched by this invocation; never discover or click apps."""
import argparse
import fcntl
import json
import os
import pathlib
import signal
import subprocess
import sys
import time

ROOT = pathlib.Path(__file__).resolve().parents[3]
os.chdir(ROOT)
lock_file = (ROOT / "target/window-benchmark.lock").open("w")
try:
    fcntl.flock(lock_file, fcntl.LOCK_EX | fcntl.LOCK_NB)
except BlockingIOError:
    raise SystemExit("Another window benchmark is running; refusing overlapping measurements")
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("variant", choices=["rust", "retained", "kept", "released"])
parser.add_argument("--samples", type=int, default=5)
parser.add_argument("--fleet", type=pathlib.Path, default=ROOT / "target/window-bench-fleet.yaml")
parser.add_argument("--executable", type=pathlib.Path)
parser.add_argument("--output", type=pathlib.Path, default=ROOT / "target/window-results")
args = parser.parse_args()
if args.samples < 5:
    parser.error("At least five timing samples are required")
executable = args.executable or {
    "retained": ROOT / "target/Menagerie-retained.app/Contents/MacOS/Menagerie",
    "kept": ROOT / "target/Menagerie-retained.app/Contents/MacOS/Menagerie",
    "released": ROOT / "target/Menagerie.app/Contents/MacOS/Menagerie",
    "rust": ROOT / "target/rust-baseline-build/release/menagerie",
}[args.variant]
if not executable.is_file() or not args.fleet.is_file():
    parser.error("Build the executable and provide a disposable fleet first")
output = args.output.resolve()
output.mkdir(parents=True, exist_ok=True)
measure_script = pathlib.Path(__file__).with_name("measure.py")
rows = []
for replicate in range(args.samples):
    trace = output / f"{args.variant}-{replicate}.trace"
    trace.write_text("")
    env = os.environ.copy()
    env.update(
        MENAGERIE_FLEET=str(args.fleet.resolve()),
        MENAGERIE_TEST_TITLE="BENCH",
        MENAGERIE_WINDOW_TRACE=str(trace),
        MENAGERIE_KEEP_WINDOW="1" if args.variant == "kept" else "0",
    )
    log_path = output / f"{args.variant}-{replicate}.log"
    with log_path.open("w") as log:
        process = subprocess.Popen([str(executable)], env=env, start_new_session=True,
                                   stdout=log, stderr=subprocess.STDOUT)
    try:
        def command(value):
            # This PID comes exclusively from our Popen object, never a process-name search.
            pathlib.Path(str(trace) + ".command").write_text(value)
            process.send_signal(signal.SIGUSR1)

        def frames():
            return [float(line.split()[2]) for line in trace.read_text().splitlines()
                    if line.startswith("frame ")]

        def open_window():
            previous = len(frames())
            command("open")
            for _ in range(200):
                if len(frames()) > previous:
                    return frames()[-1]
                if process.poll() is not None:
                    raise RuntimeError(f"Test process exited; inspect {log_path}")
                time.sleep(0.05)
            raise RuntimeError(f"No first live frame; inspect {trace}")

        def measure(phase, settle=30):
            subprocess.run([sys.executable, str(measure_script), str(process.pid),
                            f"{args.variant}-{phase}", str(settle),
                            "--directory", str(output / "memory")], check=True)

        time.sleep(4)  # Let the fleet and first live HTTP report arrive before the open action.
        if replicate == 0:
            measure("never-opened")
        cold = open_window()
        if replicate == 0:
            measure("open")
        command("close")
        closed = time.monotonic()
        time.sleep(30)
        if replicate == 0:
            measure("after-close", 0)
            # CPU sampling extends the close. Use a separate 30-second close for warm timing.
            open_window()
            command("close")
            closed = time.monotonic()
            time.sleep(30)
        warm = open_window()
        lifecycle = trace.read_text()
        if args.variant in {"retained", "kept", "released"}:
            if "popover-shown true" not in lifecycle:
                raise RuntimeError("No confirmed visible popup in the trace")
        if args.variant == "kept" and "window-hidden" not in lifecycle:
            raise RuntimeError("The keep-alive variant did not hide its window")
        if args.variant == "released" and not all(event in lifecycle for event in
                                                  ["popover-released", "window-released"]):
            raise RuntimeError("Window or popover teardown was not confirmed")
        row = {"replicate": replicate, "pid": process.pid, "cold_ms": cold,
               "warm_ms": warm, "closed_seconds": time.monotonic() - closed}
        rows.append(row)
        (output / f"{args.variant}.json").write_text(json.dumps(rows, indent=2))
        print(args.variant, row, flush=True)
    finally:
        process.terminate()
        process.wait(timeout=10)
