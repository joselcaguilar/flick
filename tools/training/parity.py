#!/usr/bin/env python3
"""Synthetic TFLite-vs-ONNX parity for the MediaPipe gesture submodels."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

import numpy as np
import onnxruntime as ort
import tensorflow as tf


ROOT = Path(__file__).resolve().parents[2]
CACHE = ROOT / "models" / "cache"


def run_tflite(model_path: Path, inputs: dict[str, np.ndarray]) -> dict[str, np.ndarray]:
    interpreter = tf.lite.Interpreter(model_path=str(model_path))
    for detail in interpreter.get_input_details():
        value = inputs[detail["name"]].astype(detail["dtype"])
        if tuple(detail["shape"]) != tuple(value.shape):
            interpreter.resize_tensor_input(detail["index"], value.shape, strict=False)
    interpreter.allocate_tensors()
    for detail in interpreter.get_input_details():
        interpreter.set_tensor(detail["index"], inputs[detail["name"]].astype(detail["dtype"]))
    interpreter.invoke()
    return {
        detail["name"]: interpreter.get_tensor(detail["index"]).copy()
        for detail in interpreter.get_output_details()
    }


def run_onnx(model_path: Path, inputs: dict[str, np.ndarray]) -> dict[str, np.ndarray]:
    session = ort.InferenceSession(str(model_path), providers=["CPUExecutionProvider"])
    feed = {input_meta.name: inputs[input_meta.name].astype(np.float32) for input_meta in session.get_inputs()}
    values = session.run(None, feed)
    return {output_meta.name: value for output_meta, value in zip(session.get_outputs(), values, strict=True)}


def diff_stats(lhs: np.ndarray, rhs: np.ndarray) -> dict[str, float | int]:
    delta = np.abs(lhs.astype(np.float64) - rhs.astype(np.float64))
    return {
        "max_abs_diff": float(delta.max(initial=0.0)),
        "mean_abs_diff": float(delta.mean() if delta.size else 0.0),
        "top1_agreement": int(np.argmax(lhs, axis=-1).tolist() == np.argmax(rhs, axis=-1).tolist()),
    }


def synthetic_landmark_cases(count: int) -> list[dict[str, np.ndarray]]:
    rng = np.random.default_rng(0xF1_1C)
    cases = []
    base = np.linspace(-0.45, 0.45, 21 * 3, dtype=np.float32).reshape(1, 21, 3)
    for i in range(count):
        hand = base + rng.normal(0.0, 0.15, size=(1, 21, 3)).astype(np.float32)
        world = (base * 0.08 + rng.normal(0.0, 0.02, size=(1, 21, 3))).astype(np.float32)
        handedness = np.array([[float(i % 2)]], dtype=np.float32)
        cases.append({"hand": hand, "handedness": handedness, "world_hand": world})
    return cases


def synthetic_embeddings(count: int) -> np.ndarray:
    rng = np.random.default_rng(0x3057_1E)
    raw = rng.normal(0.0, 1.0, size=(count, 128)).astype(np.float32)
    norms = np.linalg.norm(raw, axis=1, keepdims=True)
    return raw / np.maximum(norms, 1e-6)


def compare_embedder(cases: list[dict[str, np.ndarray]]) -> tuple[dict[str, float | int], np.ndarray, np.ndarray]:
    tflite_model = CACHE / "gesture_task" / "hand_gesture_recognizer" / "gesture_embedder.tflite"
    onnx_model = CACHE / "gesture_embedder.onnx"
    tflite_outputs = []
    onnx_outputs = []
    for inputs in cases:
        tflite_outputs.append(run_tflite(tflite_model, inputs)["Identity"])
        onnx_outputs.append(run_onnx(onnx_model, inputs)["Identity"])
    tflite_stack = np.concatenate(tflite_outputs, axis=0)
    onnx_stack = np.concatenate(onnx_outputs, axis=0)
    stats = diff_stats(tflite_stack, onnx_stack)
    stats["top1_agreement"] = int(np.array_equal(np.argmax(tflite_stack, axis=1), np.argmax(onnx_stack, axis=1)))
    return stats, tflite_stack, onnx_stack


def compare_classifier(embeddings: np.ndarray) -> dict[str, float | int]:
    tflite_model = CACHE / "gesture_task" / "hand_gesture_recognizer" / "canned_gesture_classifier.tflite"
    onnx_model = CACHE / "canned_gesture_classifier.onnx"
    inputs = {"hand_embedding": embeddings.astype(np.float32)}
    tflite = run_tflite(tflite_model, inputs)["Identity"]
    onnx = run_onnx(onnx_model, inputs)["Identity"]
    stats = diff_stats(tflite, onnx)
    stats["top1_agreement"] = int(np.array_equal(np.argmax(tflite, axis=1), np.argmax(onnx, axis=1)))
    return stats


def compare_pipeline(cases: list[dict[str, np.ndarray]]) -> dict[str, float | int]:
    tflite_embedder = CACHE / "gesture_task" / "hand_gesture_recognizer" / "gesture_embedder.tflite"
    tflite_classifier = CACHE / "gesture_task" / "hand_gesture_recognizer" / "canned_gesture_classifier.tflite"
    onnx_embedder = CACHE / "gesture_embedder.onnx"
    onnx_classifier = CACHE / "canned_gesture_classifier.onnx"

    tflite_logits = []
    onnx_logits = []
    for inputs in cases:
        tflite_embedding = run_tflite(tflite_embedder, inputs)["Identity"]
        onnx_embedding = run_onnx(onnx_embedder, inputs)["Identity"]
        tflite_logits.append(run_tflite(tflite_classifier, {"hand_embedding": tflite_embedding})["Identity"])
        onnx_logits.append(run_onnx(onnx_classifier, {"hand_embedding": onnx_embedding})["Identity"])
    tflite_stack = np.concatenate(tflite_logits, axis=0)
    onnx_stack = np.concatenate(onnx_logits, axis=0)
    stats = diff_stats(tflite_stack, onnx_stack)
    stats["top1_agreement"] = int(np.array_equal(np.argmax(tflite_stack, axis=1), np.argmax(onnx_stack, axis=1)))
    return stats


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", default=str(ROOT / "models" / "cache" / "parity-report.json"))
    parser.add_argument("--cases", type=int, default=64)
    args = parser.parse_args()

    cases = synthetic_landmark_cases(args.cases)
    embedder_stats, tflite_embeddings, onnx_embeddings = compare_embedder(cases)
    classifier_inputs = np.concatenate([synthetic_embeddings(args.cases), tflite_embeddings, onnx_embeddings], axis=0)
    report = {
        "status": "ok",
        "python": sys.version,
        "cases": args.cases,
        "embedder": embedder_stats,
        "classifier": compare_classifier(classifier_inputs),
        "pipeline": compare_pipeline(cases),
        "notes": "Deterministic synthetic tensors only; video/MediaPipe Tasks parity still waits for consented or CC0 fixtures.",
    }

    out = Path(args.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
