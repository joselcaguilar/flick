#!/usr/bin/env python3
"""Parity harness skeleton for P0-02.

Real parity requires MediaPipe's Python Tasks reference and consented/CC0
fixtures. This script records why the comparison is skipped in the Phase 0
environment rather than silently producing synthetic numbers.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--fixtures", default=str(ROOT / "tools" / "fixtures" / "videos"))
    parser.add_argument("--out", default=str(ROOT / "models" / "cache" / "parity-report.json"))
    args = parser.parse_args()

    report = {
        "status": "skipped",
        "reason": "No consented/CC0 video fixtures are present, and MediaPipe reference packages were not installed for Python 3.14.",
        "needed": [
            "self-recorded or CC0 video fixtures with license notes",
            "mediapipe Python Tasks reference",
            "onnxruntime + numpy",
        ],
        "python": sys.version,
    }

    try:
        import mediapipe as mp  # type: ignore  # noqa: F401
        report["mediapipe_import"] = "ok"
    except Exception as exc:  # pragma: no cover - spike script
        report["mediapipe_import"] = f"{type(exc).__name__}: {exc}"

    out = Path(args.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
