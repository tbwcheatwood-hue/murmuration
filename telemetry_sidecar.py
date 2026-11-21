#!/usr/bin/env python3
"""GPU telemetry sidecar for Murmuration.

Run this on each LM Link machine, Windows or Linux. It has no third-party
dependencies. It polls nvidia-smi and serves JSON on GET /metrics.

    python telemetry_sidecar.py
    python telemetry_sidecar.py --host 0.0.0.0 --port 9471

Then, on the machine running Murmuration, create mesh.manifest.json:

    {
      "telemetryPort": 9471,
      "nodes": [
        { "hostname": "GamingPC", "telemetryHost": "192.168.1.20" }
      ]
    }

Or set MURMURATION_TELEMETRY_PORT. The sidecar listens on the LAN, so use it
only on a network you trust. A missing nvidia-smi returns zeros and HTTP 200.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


def nvidia_smi_candidates() -> list:
    found = []
    on_path = shutil.which("nvidia-smi")
    if on_path:
        found.append(on_path)
    if os.name == "nt":
        found.append(r"C:\Windows\System32\nvidia-smi.exe")
        found.append(r"C:\Program Files\NVIDIA Corporation\NVSMI\nvidia-smi.exe")
    else:
        found.append("/usr/bin/nvidia-smi")
        found.append("/usr/local/nvidia/bin/nvidia-smi")
    unique = []
    for path in found:
        if path not in unique:
            unique.append(path)
    return unique


def resolve_nvidia_smi():
    for path in nvidia_smi_candidates():
        if path == "nvidia-smi" or os.path.isfile(path):
            return path
    return None


def sample() -> dict:
    program = resolve_nvidia_smi()
    empty = {"vram_total_mb": 0, "vram_free_mb": 0, "gpu_util_pct": 0, "gpu_count": 0, "gpus": []}
    if not program:
        return empty
    try:
        run_kwargs = {}
        if os.name == "nt":
            run_kwargs["creationflags"] = 0x08000000
        completed = subprocess.run(
            [
                program,
                "--query-gpu=name,memory.total,memory.free,utilization.gpu",
                "--format=csv,noheader,nounits",
            ],
            capture_output=True,
            text=True,
            timeout=2,
            check=False,
            **run_kwargs,
        )
    except (OSError, subprocess.TimeoutExpired):
        return empty
    gpus = []
    for line in completed.stdout.splitlines():
        parts = [part.strip() for part in line.split(",")]
        if len(parts) < 4:
            continue
        try:
            total = float(parts[1])
            free = float(parts[2])
            util = float(parts[3])
        except ValueError:
            continue
        if total <= 0:
            continue
        gpus.append(
            {
                "name": parts[0],
                "vram_total_mb": total,
                "vram_free_mb": free,
                "gpu_util_pct": util,
            }
        )
    if not gpus:
        return empty
    return {
        "vram_total_mb": sum(gpu["vram_total_mb"] for gpu in gpus),
        "vram_free_mb": sum(gpu["vram_free_mb"] for gpu in gpus),
        "gpu_util_pct": sum(gpu["gpu_util_pct"] for gpu in gpus) / len(gpus),
        "gpu_count": len(gpus),
        "gpus": gpus,
    }


class Handler(BaseHTTPRequestHandler):
    def do_GET(self) -> None:  # noqa: N802
        if self.path.split("?", 1)[0] != "/metrics":
            self.send_response(404)
            self.end_headers()
            return
        body = json.dumps(sample()).encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, fmt: str, *args) -> None:
        return


def main() -> None:
    parser = argparse.ArgumentParser(description="Serve nvidia-smi VRAM as JSON on /metrics")
    parser.add_argument("--host", default="0.0.0.0")
    parser.add_argument("--port", type=int, default=9471)
    args = parser.parse_args()
    server = ThreadingHTTPServer((args.host, args.port), Handler)
    print(f"telemetry sidecar on http://{args.host}:{args.port}/metrics")
    server.serve_forever()


if __name__ == "__main__":
    main()
