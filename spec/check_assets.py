#!/usr/bin/env python3
"""Assert that committed normative assets match their reviewed derivations."""

import ast
import json
import math
from pathlib import Path
import re
import subprocess
import sys


ROOT = Path(__file__).resolve().parent
V1 = ROOT / "v1"


def array(path, key):
    text = path.read_text(encoding="utf-8")
    match = re.search(r"^%s\s*=\s*(\[.*\])$" % re.escape(key), text, re.MULTILINE)
    if not match:
        raise AssertionError("%s: missing literal array %s" % (path, key))
    return ast.literal_eval(match.group(1))


def rounded(value):
    return int(math.floor(value + 0.5))


def expected_matrix(size):
    result = []
    for frequency in range(size):
        scale = 64.0 if frequency == 0 else 64.0 * math.sqrt(2.0)
        for sample in range(size):
            angle = math.pi * (2 * sample + 1) * frequency / (2 * size)
            result.append(rounded(scale * math.cos(angle)))
    return result


def expected_scan(size):
    result = []
    for diagonal in range(2 * size - 1):
        low = max(0, diagonal - size + 1)
        high = min(size - 1, diagonal)
        ys = range(high, low - 1, -1) if diagonal % 2 == 0 else range(low, high + 1)
        for y in ys:
            result.extend((diagonal - y, y))
    return result


def main():
    errors = []
    required = [
        "constants.toml", "fields.toml", "contexts.toml", "syntax.toml",
        "intra.toml", "mc.toml", "deblock.toml", "search.toml", "scans.toml",
        "transforms.toml", "quant.toml", "costs.toml", "transform-vectors.toml",
        "vectors.json",
    ]
    for name in required:
        if not (V1 / name).is_file():
            errors.append("missing normative asset %s" % name)

    contexts = (V1 / "contexts.toml").read_text(encoding="utf-8")
    ids = []
    initials = []
    suffix_count = 0
    for match in re.finditer(r"^ids\s*=\s*(\[.*\])$", contexts, re.MULTILINE):
        ids.extend(ast.literal_eval(match.group(1)))
    for match in re.finditer(r"^initial_p1\s*=\s*(\[.*\])$", contexts, re.MULTILINE):
        initials.extend(ast.literal_eval(match.group(1)))
    for match in re.finditer(r"^suffixes\s*=\s*(\[.*\])$", contexts, re.MULTILINE):
        suffix_count += len(ast.literal_eval(match.group(1)))
    if ids != list(range(144)):
        errors.append("contexts.toml ids must be the contiguous stable range 0..143")
    if len(initials) != 144 or not all(1 <= value <= 4095 for value in initials):
        errors.append("contexts.toml must contain 144 legal p1 initials")
    if suffix_count != 144:
        errors.append("contexts.toml must contain 144 stable name suffixes")

    for size in (4, 8, 16, 32):
        if array(V1 / "transforms.toml", "n%d" % size) != expected_matrix(size):
            errors.append("transforms.toml n%d differs from the reviewed derivation" % size)
        scan = array(V1 / "scans.toml", "n%d" % size)
        if scan != expected_scan(size) or len(scan) != size * size * 2:
            errors.append("scans.toml n%d is not the complete diagonal coordinate table" % size)

    def rounded_shift(value, shift):
        bias = 1 << (shift - 1)
        return (value + bias) >> shift if value >= 0 else -((abs(value) + bias) >> shift)

    def inverse_vector(coefficients, size):
        matrix = expected_matrix(size)
        horizontal = [0] * (size * size)
        for fy in range(size):
            for x in range(size):
                total = sum(coefficients[fy * size + fx] * matrix[fx * size + x] for fx in range(size))
                horizontal[fy * size + x] = max(-(1 << 31), min((1 << 31) - 1, rounded_shift(total, 7)))
        shift = {4: 11, 8: 10, 16: 9, 32: 8}[size]
        output = [0] * (size * size)
        for y in range(size):
            for x in range(size):
                total = sum(horizontal[fy * size + x] * matrix[fy * size + y] for fy in range(size))
                output[y * size + x] = max(-32768, min(32767, rounded_shift(total, shift)))
        return output

    vector_text = (V1 / "transform-vectors.toml").read_text(encoding="utf-8")
    vector_cases = re.findall(
        r'\[\[cases\]\]\nname = "([^"]+)"\nsize = (\d+)\ninput = (\[.*\])\nexpected = (\[.*\])',
        vector_text,
    )
    if len(vector_cases) != 24:
        errors.append("transform-vectors.toml must contain 24 reviewed cases")
    for name, size_text, inputs_text, expected_text in vector_cases:
        size = int(size_text)
        inputs = ast.literal_eval(inputs_text)
        expected = ast.literal_eval(expected_text)
        if len(inputs) != size * size or expected != inverse_vector(inputs, size):
            errors.append("transform vector %s differs from the literal matrix path" % name)

    qscale = [rounded(2 ** (qp / 6.0 + 4)) for qp in range(64)]
    lambda_q8 = [rounded(0.57 * 2 ** ((qp - 12) / 3.0) * 256) for qp in range(64)]
    if array(V1 / "quant.toml", "qscale") != qscale:
        errors.append("quant.toml qscale differs from C.4")
    if array(V1 / "quant.toml", "lambda_q8") != lambda_q8:
        errors.append("quant.toml lambda_q8 differs from C.4")

    cost0 = array(V1 / "costs.toml", "cost0_q16")
    cost1 = array(V1 / "costs.toml", "cost1_q16")
    expected0 = [rounded(-math.log((4096 - p1) / 4096.0, 2) * 65536) for p1 in range(1, 4096)]
    expected1 = [rounded(-math.log(p1 / 4096.0, 2) * 65536) for p1 in range(1, 4096)]
    if cost0 != expected0 or cost1 != expected1:
        errors.append("costs.toml entropy costs differ from the C.4 Q16 derivation")

    vectors = json.loads((V1 / "vectors.json").read_text(encoding="utf-8"))
    if vectors.get("format") != "key-frame-oracle-v1":
        errors.append("vectors.json has the wrong format id")
    oracle = subprocess.run(
        [sys.executable, str(ROOT / "oracle.py"), "--check"],
        cwd=str(ROOT.parent),
        text=True,
        capture_output=True,
        check=False,
    )
    if oracle.returncode != 0:
        errors.append(oracle.stderr.strip() or "oracle vector check failed")

    if errors:
        print("spec-assets: FAILED", file=sys.stderr)
        for error in errors:
            print(" - %s" % error, file=sys.stderr)
        return 1
    print("spec-assets: OK — 144 contexts, four transforms/scans, 4095 probability rows")
    return 0


if __name__ == "__main__":
    sys.exit(main())
