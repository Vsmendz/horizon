"""Upload a local NRO, save its nxlink callback, and summarize the captured run."""
import argparse
import datetime
import ipaddress
import os
from pathlib import Path
import re
import socket
import subprocess
import sys
import tempfile
import threading
import time

HERE = Path(__file__).resolve().parent
PORT = 28771  # TCP log callback. The upload itself uses nxlink's other port.


def numbers(line):
    return {k: int(v) for k, v in re.findall(r"(\w+)=([0-9]+)", line)}


def summarize(path, expected=None):
    lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
    begins = [line[6:] for line in lines if line.startswith("BEGIN ")]
    name = expected or (begins[-1] if begins else None)
    warnings = [line for line in lines if line.startswith("WARN ")]
    errors = [line for line in lines if line.startswith("FAIL ")]
    frames = next((numbers(line) for line in reversed(lines) if line.startswith("FRAME_TIMING ")), {})
    shutdown = next((numbers(line) for line in reversed(lines) if line.startswith("PASS Shutdown:")), {})
    streams = [numbers(line) for line in lines if line.startswith(("STREAM ", "STREAM_ASYNC "))]
    dropped = sum(int(line.split("=")[1]) for line in lines if re.fullmatch(r"NXLINK_DROPPED=\d+", line))

    print(f"\nLog: {path}")
    print(f"Startup: {'present' if name and name in begins else 'missing'} | "
          f"warnings: {len(warnings)} | failures: {len(errors)} | dropped network records: {dropped}")
    if frames:
        print("Frame timing:", frames)
        if frames.get("interval_total_us"):
            fps = 1_000_000 * frames["interval_samples"] / frames["interval_total_us"]
            print(f"Mean presentation interval equivalent: {fps:.2f} FPS")
    if shutdown:
        print("Shutdown counters:", shutdown)
    if streams:
        totals = {k: sum(s.get(k, 0) for s in streams) for k in ("generated", "read", "evicted", "saved")}
        print(f"Streaming events: {len(streams)}. Totals: {totals}")
    for line in warnings + errors:
        print(line)

    complete = bool(name and begins == [name] and f"END {name}" in lines)
    result = [line for line in lines if line.startswith("RESULT=")]
    returned = [line for line in lines if line.startswith("RETURN ")]
    print(f"Completion marker: {'present' if complete else 'missing or mismatched'} | "
          f"{', '.join(result + returned) or 'no application result'}")
    clean = complete and result == ["RESULT=0"] and returned == ["RETURN 0"] and not errors and not dropped
    print("Complete application log. Hardware behavior still needs user confirmation."
          if clean else "Incomplete or failed application log. See SD fallback and transport diagnostics.")
    return clean


def run_process(argv, timeout, emit):
    child = subprocess.Popen(argv, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    errors = []

    def copy_output():
        try:
            while data := child.stdout.read1(8192):
                emit(data)
        except OSError as error:
            errors.append(error)
            child.terminate()

    pump = threading.Thread(target=copy_output)
    pump.start()
    try:
        return child.wait(timeout=timeout)
    finally:
        if child.poll() is None:
            child.terminate()
            try:
                child.wait(timeout=3)
            except subprocess.TimeoutExpired:
                child.kill()
                child.wait()
        pump.join()
        child.stdout.close()
        if errors:
            raise errors[0]


def windows_callback(artifact, ip, timeout, emit):
    """Keep only the callback on the LAN side of WSL NAT, for this run."""
    def linux_path(path):
        return subprocess.check_output(
            ["wsl", "--exec", "wslpath", "-a", "-u", str(path)], encoding="utf-8", timeout=10
        ).strip()

    sdk_nxlink = linux_path(HERE.parent / "devkitpro/tools/bin/nxlink")
    target = linux_path(artifact)
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as route:
        route.connect((ip, PORT))
        local_ip = route.getsockname()[0]
    deadline = time.monotonic() + timeout

    def remaining():
        seconds = deadline - time.monotonic()
        if seconds <= 0:
            raise TimeoutError("nxlink session deadline expired")
        return seconds

    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
        listener.bind((local_ip, PORT))
        listener.listen(1)
        emit(f"Windows nxlink callback listening on {local_ip}:{PORT} for {ip}\n".encode())
        upload_limit = max(1, int(remaining()))
        status = run_process(
            ["wsl", "--exec", "timeout", "--kill-after=2", str(upload_limit),
             sdk_nxlink, "-a", ip, target],
            upload_limit + 2,
            emit,
        )
        if status:
            return status
        while True:
            listener.settimeout(remaining())
            client, peer = listener.accept()
            with client:
                if peer[0] != ip:
                    continue
                emit(f"Connected callback from {peer[0]}\n".encode())
                while True:
                    client.settimeout(remaining())
                    data = client.recv(8192)
                    if not data:
                        return 0
                    emit(data)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("artifact", nargs="?", help="test name or NRO path")
    parser.add_argument("--address", default="192.168.1.5")
    parser.add_argument("--timeout", type=int, default=900, help="session limit in seconds")
    parser.add_argument("--nxlink", help="use this executable with -s")
    parser.add_argument("--analyze", type=Path, help="summarize an existing SD or captured log")
    args = parser.parse_args()
    if args.analyze is not None:
        if not args.analyze.is_file():
            parser.error("log file does not exist")
        return 0 if summarize(args.analyze) else 2
    if not args.artifact or args.timeout <= 0:
        parser.error("supply an NRO/test name and a positive timeout")
    try:
        ip = str(ipaddress.IPv4Address(args.address))
    except ipaddress.AddressValueError:
        parser.error("--address must be the Switch IPv4 address")
    artifact = Path(args.artifact)
    if not artifact.is_file() and artifact.parent == Path("."):
        artifact = HERE / "out" / artifact.with_suffix(".nro")
    artifact = artifact.resolve()
    if artifact.suffix.lower() != ".nro" or not artifact.is_file():
        parser.error(f"NRO does not exist: {artifact}")

    logs = HERE / "out/hardware-logs"
    logs.mkdir(parents=True, exist_ok=True)
    stamp = datetime.datetime.now().strftime("%Y%m%d-%H%M%S")
    fd, log = tempfile.mkstemp(prefix=f"{artifact.stem}-{stamp}-", suffix=".log", dir=logs)
    log = Path(log)
    print(f"Capturing to {log}", flush=True)
    with os.fdopen(fd, "wb", buffering=0) as capture:
        def emit(data):
            capture.write(data)
            sys.stdout.buffer.write(data)
            sys.stdout.buffer.flush()

        try:
            if os.name == "nt" and args.nxlink is None:
                status = windows_callback(artifact, ip, args.timeout, emit)
            else:
                nxlink = args.nxlink or str(HERE.parent / "devkitpro/tools/bin/nxlink")
                prefix = [] if os.name == "nt" else ["stdbuf", "-oL", "-eL"]
                status = run_process([*prefix, nxlink, "-a", ip, str(artifact), "-s"], args.timeout, emit)
        except KeyboardInterrupt:
            status = 130
            emit(b"\nCapture interrupted. Partial log retained.\n")
        except (TimeoutError, subprocess.TimeoutExpired):
            status = 124
            emit(b"\nCapture timed out. Check callback reachability and the SD log.\n")
        except (OSError, subprocess.CalledProcessError) as error:
            status = 1
            emit(f"\nCapture failed: {error}\n".encode())
    clean = summarize(log, artifact.stem)
    return status or (0 if clean else 2)


if __name__ == "__main__":
    sys.exit(main())
