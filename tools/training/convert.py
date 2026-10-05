#!/usr/bin/env python3
"""Fetch, inspect, and document Flick Phase 0 model candidates.

The fetch/inspect path intentionally uses only the Python standard library so it
can run without downloading wheels. Gesture conversion uses the project-local
uv environment pinned to Homebrew Python 3.12 plus Apache/MIT-licensed tools.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import shutil
import subprocess
import sys
import urllib.request
import zipfile
from dataclasses import dataclass
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
CACHE = ROOT / "models" / "cache"
DOWNLOADS = CACHE / "downloads"


@dataclass(frozen=True)
class Download:
    name: str
    url: str
    target: Path


DOWNLOADS_TO_FETCH = [
    Download(
        "palm",
        "https://huggingface.co/opencv/palm_detection_mediapipe/resolve/233e619dcea1759bf6de707b9b904fe30881ea55/palm_detection_mediapipe_2023feb.onnx",
        DOWNLOADS / "palm_detection_mediapipe_2023feb.onnx",
    ),
    Download(
        "hand_landmark",
        "https://huggingface.co/opencv/handpose_estimation_mediapipe/resolve/4b2a0b446e5cf2f11fb6b2c7251091c035d2c1f7/handpose_estimation_mediapipe_2023feb.onnx",
        DOWNLOADS / "handpose_estimation_mediapipe_2023feb.onnx",
    ),
    Download(
        "blazeface_short_range",
        "https://huggingface.co/unity/inference-engine-blaze-face/resolve/6b3dcfdd87c36a157477329821bab552eddc8f79/models/blaze_face_short_range.onnx",
        DOWNLOADS / "blaze_face_short_range.onnx",
    ),
    Download(
        "dinov2_small_uint8",
        "https://huggingface.co/onnx-community/dinov2-small/resolve/8b1f705a3a7f6f062f6bdd21986c1583d3ef105d/onnx/model_uint8.onnx",
        DOWNLOADS / "dinov2-small-model_uint8.onnx",
    ),
    Download(
        "gesture_recognizer_task",
        "https://storage.googleapis.com/mediapipe-models/gesture_recognizer/gesture_recognizer/float16/1/gesture_recognizer.task",
        DOWNLOADS / "gesture_recognizer.task",
    ),
    Download(
        "qaihub_gesture_onnx_float",
        "https://qaihub-public-assets.s3.us-west-2.amazonaws.com/qai-hub-models/models/mediapipe_hand_gesture/releases/v0.63.0/mediapipe_hand_gesture-onnx-float.zip",
        DOWNLOADS / "qualcomm_mediapipe_hand_gesture_onnx_float.zip",
    ),
]


DTYPES = {
    1: "float32",
    2: "uint8",
    3: "int8",
    4: "uint16",
    5: "int16",
    6: "int32",
    7: "int64",
    9: "bool",
    10: "float16",
    11: "float64",
    12: "uint32",
    13: "uint64",
    16: "bfloat16",
}


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def fetch_one(item: Download) -> dict[str, object]:
    item.target.parent.mkdir(parents=True, exist_ok=True)
    if not item.target.exists() or item.target.stat().st_size == 0:
        req = urllib.request.Request(item.url, headers={"User-Agent": "flick-p0-spike/1.0"})
        with urllib.request.urlopen(req, timeout=120) as r, item.target.open("wb") as f:
            shutil.copyfileobj(r, f)
    return {
        "name": item.name,
        "url": item.url,
        "file": str(item.target.relative_to(ROOT)),
        "size": item.target.stat().st_size,
        "sha256": sha256(item.target),
    }


def extract_known_archives() -> None:
    task = DOWNLOADS / "gesture_recognizer.task"
    if task.exists():
        with zipfile.ZipFile(task) as z:
            out = CACHE / "gesture_task"
            out.mkdir(parents=True, exist_ok=True)
            for member in z.infolist():
                z.extract(member, out)
        for nested in [CACHE / "gesture_task" / "hand_gesture_recognizer.task", CACHE / "gesture_task" / "hand_landmarker.task"]:
            if nested.exists():
                with zipfile.ZipFile(nested) as z:
                    out = nested.with_suffix("")
                    out.mkdir(parents=True, exist_ok=True)
                    for member in z.infolist():
                        z.extract(member, out)

    qzip = DOWNLOADS / "qualcomm_mediapipe_hand_gesture_onnx_float.zip"
    if qzip.exists():
        out = CACHE / "qualcomm_hand_gesture"
        out.mkdir(parents=True, exist_ok=True)
        with zipfile.ZipFile(qzip) as z:
            for member in z.infolist():
                if not member.is_dir():
                    z.extract(member, out)


def stage_canonical_cache_files() -> None:
    copies = {
        DOWNLOADS / "palm_detection_mediapipe_2023feb.onnx": CACHE / "palm_detection_full.onnx",
        DOWNLOADS / "handpose_estimation_mediapipe_2023feb.onnx": CACHE / "hand_landmark_full.onnx",
        DOWNLOADS / "blaze_face_short_range.onnx": CACHE / "face_detection_short.onnx",
        DOWNLOADS / "dinov2-small-model_uint8.onnx": CACHE / "scene_embedder_dinov2_small_uint8.onnx",
        CACHE / "qualcomm_hand_gesture" / "mediapipe_hand_gesture-onnx-float" / "canned_gesture_classifier.onnx": CACHE / "canned_gesture_classifier_qaihub.onnx",
        CACHE / "qualcomm_hand_gesture" / "mediapipe_hand_gesture-onnx-float" / "canned_gesture_classifier.data": CACHE / "canned_gesture_classifier_qaihub.data",
        # The ONNX file refers to this original external-data filename.
        CACHE / "qualcomm_hand_gesture" / "mediapipe_hand_gesture-onnx-float" / "canned_gesture_classifier.data": CACHE / "canned_gesture_classifier.data",
    }
    for src, dst in copies.items():
        if src.exists():
            dst.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(src, dst)


def read_varint(buf: bytes, i: int) -> tuple[int, int]:
    value = 0
    shift = 0
    while True:
        byte = buf[i]
        i += 1
        value |= (byte & 0x7F) << shift
        if byte < 128:
            return value, i
        shift += 7


def iter_fields(buf: bytes):
    i = 0
    while i < len(buf):
        key, i = read_varint(buf, i)
        number, wire = key >> 3, key & 7
        if wire == 0:
            value, i = read_varint(buf, i)
            yield number, wire, value
        elif wire == 1:
            yield number, wire, buf[i : i + 8]
            i += 8
        elif wire == 2:
            length, i = read_varint(buf, i)
            yield number, wire, buf[i : i + length]
            i += length
        elif wire == 5:
            yield number, wire, buf[i : i + 4]
            i += 4
        else:
            raise ValueError(f"unsupported protobuf wire type {wire}")


def messages(buf: bytes, field: int) -> list[bytes]:
    return [v for n, w, v in iter_fields(buf) if n == field and w == 2]


def first_message(buf: bytes, field: int) -> bytes | None:
    values = messages(buf, field)
    return values[0] if values else None


def first_string(buf: bytes, field: int) -> str:
    for n, w, v in iter_fields(buf):
        if n == field and w == 2:
            return v.decode("utf-8", "replace")
    return ""


def first_varint(buf: bytes, field: int) -> int | None:
    for n, w, v in iter_fields(buf):
        if n == field and w == 0:
            return int(v)
    return None


def parse_shape(shape_msg: bytes) -> list[str]:
    dims: list[str] = []
    for dim in messages(shape_msg, 1):
        value = None
        param = None
        for n, w, v in iter_fields(dim):
            if n == 1 and w == 0:
                value = str(v)
            elif n == 2 and w == 2:
                param = v.decode("utf-8", "replace")
        dims.append(value if value is not None else param if param else "?")
    return dims


def inspect_onnx_fallback(path: Path) -> dict[str, object]:
    model = path.read_bytes()
    graph = first_message(model, 7)
    if graph is None:
        raise ValueError(f"{path} has no ONNX graph")

    def parse_value_info(value_info: bytes) -> dict[str, object]:
        typ = first_message(value_info, 2)
        elem_type = None
        shape: list[str] = []
        if typ:
            tensor = first_message(typ, 1)
            if tensor:
                elem_type = first_varint(tensor, 1)
                shape_msg = first_message(tensor, 2)
                if shape_msg:
                    shape = parse_shape(shape_msg)
        return {
            "name": first_string(value_info, 1),
            "dtype": DTYPES.get(elem_type, str(elem_type)),
            "shape": shape,
        }

    return {
        "file": str(path.relative_to(ROOT)),
        "size": path.stat().st_size,
        "sha256": sha256(path),
        "graph": first_string(graph, 2),
        "inputs": [parse_value_info(v) for v in messages(graph, 11)],
        "outputs": [parse_value_info(v) for v in messages(graph, 12)],
    }


def inspect_onnx(path: Path) -> dict[str, object]:
    try:
        import onnx  # type: ignore

        model = onnx.load(path, load_external_data=False)
        graph = model.graph

        def value_info(v) -> dict[str, object]:
            tensor = v.type.tensor_type
            dims = []
            for dim in tensor.shape.dim:
                if dim.HasField("dim_value"):
                    dims.append(str(dim.dim_value))
                elif dim.dim_param:
                    dims.append(dim.dim_param)
                else:
                    dims.append("?")
            return {
                "name": v.name,
                "dtype": onnx.TensorProto.DataType.Name(tensor.elem_type).lower(),
                "shape": dims,
            }

        return {
            "file": str(path.relative_to(ROOT)),
            "size": path.stat().st_size,
            "sha256": sha256(path),
            "graph": graph.name,
            "inputs": [value_info(v) for v in graph.input],
            "outputs": [value_info(v) for v in graph.output],
        }
    except ModuleNotFoundError:
        return inspect_onnx_fallback(path)


def convert_gesture_with_tf2onnx() -> int:
    conversions = [
        (
            CACHE / "gesture_task" / "hand_gesture_recognizer" / "gesture_embedder.tflite",
            CACHE / "gesture_embedder.onnx",
        ),
        (
            CACHE / "gesture_task" / "hand_gesture_recognizer" / "canned_gesture_classifier.tflite",
            CACHE / "canned_gesture_classifier.onnx",
        ),
    ]
    missing = [str(src) for src, _ in conversions if not src.exists()]
    if missing:
        print(f"missing extracted TFLite sources: {missing}", file=sys.stderr)
        return 2
    try:
        import tf2onnx  # noqa: F401
    except ModuleNotFoundError:
        print("tf2onnx is not installed. Run with the 'tensorflow' extra under Python 3.12.", file=sys.stderr)
        return 2

    status = 0
    for src, out in conversions:
        out.parent.mkdir(parents=True, exist_ok=True)
        cmd = [
            sys.executable,
            "-m",
            "tf2onnx.convert",
            "--tflite",
            str(src),
            "--output",
            str(out),
            "--opset",
            "17",
        ]
        print("+", " ".join(cmd))
        result = subprocess.run(cmd, cwd=ROOT, check=False)
        if result.returncode == 0:
            canonicalize_onnx(out, src)
        status = max(status, result.returncode)
    return status


def canonicalize_onnx(path: Path, source: Path) -> None:
    """Makes tf2onnx output byte-for-byte reproducible, so the pinned SHA-256 can be rebuilt.

    tf2onnx numbers the constants it generates (`const_fold_opt__N`, `const_axes__N`, ...) in a
    different order on every run and records the absolute .tflite path in the graph doc
    string. Generated constants are renumbered in order of first use, initializers are
    sorted, and the doc string names the source relative to the repo. The graph and weights
    are unchanged.
    """
    import re

    import onnx

    model = onnx.load(path)
    graph = model.graph
    subgraph_types = (onnx.AttributeProto.GRAPH, onnx.AttributeProto.GRAPHS)
    if any(attr.type in subgraph_types for node in graph.node for attr in node.attribute):
        raise SystemExit(f"{path}: subgraphs are not canonicalized; extend canonicalize_onnx")
    generated = re.compile(r"^(.+?)__\d+$")
    numbered = {init.name for init in graph.initializer if generated.match(init.name)}
    used = [name for node in graph.node for name in node.input if name in numbered]
    order = list(dict.fromkeys(used + sorted(numbered)))
    mapping = {name: f"{generated.match(name).group(1)}__{index}" for index, name in enumerate(order)}
    tensors = {out for node in graph.node for out in node.output} | {value.name for value in graph.input}
    if tensors & (set(mapping.values()) - numbered):
        raise SystemExit(f"{path}: renamed constants would collide with graph tensors")
    for node in graph.node:
        node.input[:] = [mapping.get(name, name) for name in node.input]
    for value in [*graph.input, *graph.output, *graph.value_info]:
        value.name = mapping.get(value.name, value.name)
    initializers = []
    for init in graph.initializer:
        copy = onnx.TensorProto()
        copy.CopyFrom(init)
        copy.name = mapping.get(copy.name, copy.name)
        initializers.append(copy)
    del graph.initializer[:]
    graph.initializer.extend(sorted(initializers, key=lambda init: init.name))
    graph.doc_string = f"converted from {source.relative_to(ROOT).as_posix()}"
    onnx.save(model, path)


def cmd_fetch(args: argparse.Namespace) -> None:
    names = {item.name for item in DOWNLOADS_TO_FETCH}
    unknown = sorted(set(args.names) - names)
    if unknown:
        raise SystemExit(f"unknown downloads {unknown}; choose from {sorted(names)}")
    summary = [fetch_one(item) for item in DOWNLOADS_TO_FETCH if not args.names or item.name in args.names]
    extract_known_archives()
    stage_canonical_cache_files()
    (DOWNLOADS / "download-summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    print(json.dumps(summary, indent=2))


def cmd_inspect(args: argparse.Namespace) -> None:
    paths = [Path(p) for p in args.paths] if args.paths else sorted(CACHE.glob("*.onnx"))
    print(json.dumps([inspect_onnx(p if p.is_absolute() else ROOT / p) for p in paths], indent=2))


def cmd_hash(args: argparse.Namespace) -> None:
    for raw in args.paths:
        path = Path(raw)
        if not path.is_absolute():
            path = ROOT / path
        print(f"{sha256(path)}  {path.relative_to(ROOT)}  {path.stat().st_size}")


def main() -> int:
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="cmd", required=True)
    fetch = sub.add_parser("fetch")
    fetch.add_argument("names", nargs="*", help="download only these sources (default: all)")
    fetch.set_defaults(func=cmd_fetch)
    inspect = sub.add_parser("inspect")
    inspect.add_argument("paths", nargs="*")
    inspect.set_defaults(func=cmd_inspect)
    hash_cmd = sub.add_parser("hash")
    hash_cmd.add_argument("paths", nargs="+")
    hash_cmd.set_defaults(func=cmd_hash)
    conv = sub.add_parser("convert-gesture")
    conv.set_defaults(func=lambda _args: sys.exit(convert_gesture_with_tf2onnx()))
    args = parser.parse_args()
    args.func(args)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
