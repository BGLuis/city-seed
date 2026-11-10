#!/usr/bin/env python3
"""Runs the 3D viewer's scripted flight in Chrome and checks it against budgets.

Exits non-zero when a budget is exceeded. Needs only Python and Chrome/Chromium.
Set CHROME to choose the browser. Software renderers have no meaningful frame
rate, so frame budgets apply only when a real GPU is detected.
"""

import argparse
import http.server
import json
import os
import shutil
import subprocess
import sys
import tempfile
import threading
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
VIEWER = ROOT / "viewer"
REPORT = ROOT / "target/perf/report-3d.json"
SOFTWARE_RENDERERS = ("swiftshader", "llvmpipe", "software")

ALWAYS = [
    ("firstViewLoadMs", "primeira vista carregada", 30_000),
    ("districtBuildMs.p50", "distrito: geração + malha (p50)", 200),
    ("districtBuildMs.max", "distrito: geração + malha (máx.)", 600),
    ("uploadMs.p95", "envio à placa (p95)", 30),
    ("cpuMs.p95", "CPU do quadro em JS (p95)", 8),
    ("cpuMs.p99", "CPU do quadro em JS (p99)", 16),
    ("cityGenerateMs", "geração da cidade", 5_000),
    ("cityMeshMs", "malha da cidade", 5_000),
    ("peakTriangles", "pico de triângulos", 6_000_000),
    ("peakGpuMegabytes", "pico de memória de vídeo (MB)", 800),
]
GPU_ONLY = [
    ("renderedMs.p50", "quadro (p50)", 16.7),
    ("renderedMs.p95", "quadro (p95)", 33.4),
    ("renderedMs.p99", "quadro (p99)", 50),
]
SCALED_EXEMPT = {"peakTriangles", "peakGpuMegabytes"}


def find_chrome():
    configured = os.environ.get("CHROME")
    if configured:
        return configured
    for name in ("google-chrome-stable", "google-chrome", "chromium", "chromium-browser"):
        path = shutil.which(name)
        if path:
            return path
    sys.exit("Chrome/Chromium não encontrado; defina a variável CHROME.")


class Server(http.server.ThreadingHTTPServer):
    result = None
    finished = threading.Event()


class Handler(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=str(VIEWER), **kwargs)

    def do_POST(self):
        length = int(self.headers["Content-Length"])
        self.server.result = json.loads(self.rfile.read(length))
        self.send_response(204)
        self.end_headers()
        self.server.finished.set()

    def log_message(self, *args):
        pass


def lookup(report, path):
    value = report
    for key in path.split("."):
        value = value[key]
    return value


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--seed", type=int, default=42)
    parser.add_argument("--population", type=int, default=40_000_000)
    parser.add_argument("--region", default="JP-KT")
    parser.add_argument("--frames", type=int, default=600)
    parser.add_argument("--detail", type=int, default=24)
    parser.add_argument("--timeout", type=int, default=300)
    parser.add_argument("--budget-scale", type=float, default=1.0)
    parser.add_argument("--gl", default="swiftshader", help="backend ANGLE: swiftshader, vulkan, gl, gl-egl")
    parser.add_argument("--skip-build", action="store_true")
    args = parser.parse_args()

    if not args.skip_build:
        subprocess.run([str(ROOT / "scripts/build-viewer.sh")], check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

    server = Server(("127.0.0.1", 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    url = (
        f"http://127.0.0.1:{server.server_address[1]}/3d.html?bench=1&report=1"
        f"&seed={args.seed}&population={args.population}&region={args.region}"
        f"&frames={args.frames}&detail={args.detail}"
    )
    with tempfile.TemporaryDirectory() as profile:
        chrome = subprocess.Popen(
            [
                find_chrome(),
                "--headless=new",
                f"--user-data-dir={profile}",
                "--no-first-run",
                "--window-size=1920,1080",
                "--use-gl=angle",
                f"--use-angle={args.gl}",
                "--enable-unsafe-swiftshader",
                "--ignore-gpu-blocklist",
                "--disable-gpu-vsync",
                "--disable-frame-rate-limit",
                url,
            ],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        completed = server.finished.wait(args.timeout)
        chrome.terminate()
        try:
            chrome.wait(10)
        except subprocess.TimeoutExpired:
            chrome.kill()
    server.shutdown()

    if not completed:
        sys.exit(f"o benchmark não terminou em {args.timeout} s")
    report = server.result
    if "failed" in report:
        sys.exit(f"o benchmark falhou: {report['failed']}")

    renderer = report["renderer"]
    is_software = any(name in renderer.lower() for name in SOFTWARE_RENDERERS)
    checks = ALWAYS + ([] if is_software else GPU_ONLY)
    print(f"placa: {renderer}")
    print(f"cidade: seed {args.seed}, {args.population:,} hab., {args.region}; {report['frames']} quadros, tela {report['canvas']}")
    print(f"{'medida':<36} {'valor':>12} {'limite':>12}")

    failed = False
    measurements = []
    for path, label, budget in checks:
        limit = budget if path in SCALED_EXEMPT else budget * args.budget_scale
        value = lookup(report, path)
        passed = value <= limit
        failed |= not passed
        measurements.append({"name": path, "label": label, "value": value, "limit": limit, "passed": passed})
        print(f"{label:<36} {value:>12.2f} {limit:>12.2f}{'' if passed else '  FALHOU'}")
    if is_software:
        print(f"\nrenderização por software: quadro p50 {report['renderedMs']['p50']:.1f} ms e p95 "
              f"{report['renderedMs']['p95']:.1f} ms são só informativos.")

    REPORT.parent.mkdir(parents=True, exist_ok=True)
    REPORT.write_text(json.dumps({"arguments": vars(args), "measurements": measurements, "raw": report}, indent=2))
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
