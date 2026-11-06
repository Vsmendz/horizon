"""Exercise the real SDK nxlink and the console log sender on WSL loopback."""
import os
from pathlib import Path
import shutil
import socket
import struct
import subprocess
import sys
import tempfile
from concurrent.futures import ThreadPoolExecutor
import zlib

HERE = Path(__file__).resolve().parent


def exact(stream, size):
    data = stream.read(size)
    assert len(data) == size, "short nxlink upload"
    return data


def receive_upload(listener, payload):
    conn, _ = listener.accept()
    with conn, conn.makefile("rb") as stream:
        conn.settimeout(5)

        def u32():
            return struct.unpack("<I", exact(stream, 4))[0]

        assert exact(stream, u32()) == b"capture_probe.nro"
        assert u32() == len(payload)
        conn.sendall(bytes(4))
        inflater = zlib.decompressobj()
        image = bytearray()
        while not inflater.eof:
            image.extend(inflater.decompress(exact(stream, u32())))
        assert image == payload
        conn.sendall(bytes(4))
        exact(stream, u32())


def main():
    nxlink = Path(os.environ["DEVKITPRO"]) / "tools/bin/nxlink"
    with tempfile.TemporaryDirectory(prefix="horizon-nxlink-") as folder:
        work = Path(folder)
        runner = work / "run_hardware_test.py"
        shutil.copyfile(HERE / runner.name, runner)
        fixture = work / "sender.c"
        fixture.write_text(r"""
#include "nxlink_log.h"
#include <arpa/inet.h>
#include <assert.h>
#include <string.h>

int main(int argc, char **argv) {
    nxlog_start(htonl(INADDR_LOOPBACK));
    nxlog_push("BEGIN capture_probe");
    nxlog_push("STREAM_ASYNC resident=9 generated=3");
    nxlog_push("FRAME_TIMING interval_samples=2 interval_total_us=40000");
    if (argc == 1) {
        nxlog_push("RESULT=0\nRETURN 0");
        nxlog_push("END capture_probe");
    } else if (strcmp(argv[1], "drop") == 0) {
        char oversized[600];
        memset(oversized, 'x', sizeof(oversized) - 1);
        oversized[sizeof(oversized) - 1] = 0;
        nxlog_push(oversized);
        nxlog_push("RESULT=0\nRETURN 0");
        nxlog_push("END capture_probe");
    }
    NxLogResult result = nxlog_finish();
    assert(!result.error);
    return 0;
}
""")
        sender = work / "sender"
        subprocess.run(
            ["cc", "-std=c11", "-D_POSIX_C_SOURCE=200809L", "-Wall", "-Wextra", "-Werror",
             "-pthread", "-I", str(HERE), str(fixture), str(HERE / "nxlink_log.c"),
             "-o", str(sender)],
            check=True,
        )
        image = work / "capture_probe.nro"
        # nxlink transmits arbitrary bytes, so no game files are needed.
        payload = bytes(range(256)) * 128
        image.write_bytes(payload)
        for mode, expected in (([], 0), (["truncated"], 2), (["drop"], 2)):
            with socket.socket() as listener, ThreadPoolExecutor(max_workers=2) as pool:
                listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
                listener.bind(("127.0.0.1", 28280))
                listener.listen(1)
                listener.settimeout(6)
                capture = pool.submit(
                    subprocess.run,
                    [sys.executable, str(runner), str(image), "--address", "127.0.0.1",
                     "--nxlink", str(nxlink), "--timeout", "7"],
                    capture_output=True, text=True, timeout=12,
                )
                receive_upload(listener, payload)
                subprocess.run([str(sender), *mode], check=True, timeout=5)
                result = capture.result()
                assert result.returncode == expected, result.stdout + result.stderr
                print(f"PASS nxlink loopback {mode or 'complete'}: exit {expected}")
        logs = list((work / "out/hardware-logs").glob("*.log"))
        assert len(logs) == 3
        assert any("NXLINK_DROPPED=1" in log.read_text() for log in logs)
        print("PASS unique capture files and dropped-record marker")


if __name__ == "__main__":
    main()
