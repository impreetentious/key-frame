#!/usr/bin/env python3
"""Prove the specification gates are not one opinion agreeing with itself.

A generator and its consumer that share a wrong formula agree perfectly. The
whole point of writing the derivation checks and the oracle independently of
the Rust implementation is that a mistake has to be made twice to survive, and
nothing in a passing suite tells you whether that independence is still real.

This mutates exactly one entry of one committed asset at a time, in a throwaway
copy of the specification tree, and asserts the gates do not all shrug. Two
claims are checked per case: the generated-document inventory always notices,
because it hashes every asset it renders from; and for every asset that has an
independent derivation, a semantic gate notices too, so the tripwire is not the
only thing standing there.
"""

import json
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path


ROOT = Path(__file__).resolve().parent
REPO = ROOT.parent


class Case:
    """One single-entry mutation and what is expected to reject it."""

    def __init__(self, asset, describe, mutate, semantic):
        self.asset = asset
        self.describe = describe
        self.mutate = mutate
        # True when an independent derivation covers this asset, so a gate
        # other than the document hash must also reject the mutation.
        self.semantic = semantic


def bump_first_in_array(key):
    """Changes the first element of a named literal array by one."""

    def apply(text):
        match = re.search(r"^%s\s*=\s*\[\s*(-?\d+)" % re.escape(key), text, re.MULTILINE)
        if not match:
            raise SystemExit("mutation target %s not found" % key)
        value = int(match.group(1)) + 1
        start, end = match.span(1)
        return text[:start] + str(value) + text[end:]

    return apply


def bump_scalar(key):
    """Changes a named scalar by one."""

    def apply(text):
        match = re.search(r"^%s\s*=\s*(-?\d+)\s*$" % re.escape(key), text, re.MULTILINE)
        if not match:
            raise SystemExit("mutation target %s not found" % key)
        value = int(match.group(1)) + 1
        start, end = match.span(1)
        return text[:start] + str(value) + text[end:]

    return apply


def bump_shift_formula(key):
    """Changes the constant inside a declared `K-log2(N)` stage shift."""

    def apply(text):
        match = re.search(r'^%s\s*=\s*"(\d+)-log2\(N\)"\s*$' % re.escape(key), text, re.MULTILINE)
        if not match:
            raise SystemExit("mutation target %s not found" % key)
        value = int(match.group(1)) + 1
        start, end = match.span(1)
        return text[:start] + str(value) + text[end:]

    return apply


def bump_phase_weight(key):
    """Changes one weight inside a multi-line phase blend table."""

    def apply(text):
        match = re.search(r'^%s = \[\n  \[[^\]]*\],\n  \[(\d+), (\d+), (\d+),' % re.escape(key), text, re.MULTILINE)
        if not match:
            raise SystemExit("mutation target %s not found" % key)
        value = int(match.group(3)) + 1
        start, end = match.span(3)
        return text[:start] + str(value) + text[end:]

    return apply


def bump_first_case_expectation(text):
    """Changes one expected sample of the first motion-compensation case."""
    match = re.search(r"^expected = \[\s*(\d+)", text, re.MULTILINE)
    if not match:
        raise SystemExit("mutation target expected not found")
    value = (int(match.group(1)) + 1) % 256
    start, end = match.span(1)
    return text[:start] + str(value) + text[end:]


def rename_first_plane(text):
    """Renames the first coded plane, leaving the residual order behind."""
    match = re.search(r'^planes = \["([a-z]+)"', text, re.MULTILINE)
    if not match:
        raise SystemExit("mutation target planes not found")
    start, end = match.span(1)
    return text[:start] + (match.group(1) + "1") + text[end:]


def drop_last_predictor_candidate(text):
    """Removes the last motion-vector predictor candidate."""
    match = re.search(r'^candidates = \[(.*)\]$', text, re.MULTILINE)
    if not match:
        raise SystemExit("mutation target candidates not found")
    entries = [entry.strip() for entry in match.group(1).split(",")]
    if len(entries) < 2:
        raise SystemExit("candidates is too short to shorten")
    start, end = match.span(1)
    return text[:start] + ", ".join(entries[:-1]) + text[end:]


def flip_oracle_payload(text):
    """Changes one byte of one committed oracle vector."""
    document = json.loads(text)
    payload = document["packet"]["payload_hex"]
    first = payload[:2]
    replacement = "%02x" % ((int(first, 16) ^ 0x01) & 0xFF)
    document["packet"]["payload_hex"] = replacement + payload[2:]
    return json.dumps(document, indent=2) + "\n"


# Two halves of the same protection, and it is worth being precise about which
# half lives here. This harness mutates a copied specification tree and runs the
# Python gates over it, so it proves that a changed *asset* is rejected. It
# cannot prove the reverse — that the Rust still enforces what the asset says —
# because it never builds the Rust. That direction is covered by the
# `normative_limits` tests in `kf-bitstream`, `kf-range`, and `kf-fuzz`, which
# drive the real constructors with the declared values and require them to
# accept exactly what the document promises.
CASES = [
    Case("constants.toml", "the largest declared picture width", bump_scalar("max_width"), False),
    Case("constants.toml", "the declared probability clamp", bump_scalar("probability_max"), False),
    Case("transforms.toml", "one transform scale constant", bump_scalar("dc_scale"), True),
    Case("transforms.toml", "the first inverse stage shift", bump_scalar("inverse_shift1"), True),
    Case("transforms.toml", "the second inverse stage shift", bump_shift_formula("inverse_shift2"), True),
    Case("transforms.toml", "the final reconstruction clamp", bump_scalar("post_inverse_max"), True),
    Case("quant.toml", "one quantizer scale entry", bump_first_in_array("qscale"), True),
    Case("costs.toml", "one modeled-entropy cost entry", bump_first_in_array("cost0_q16"), True),
    Case("contexts.toml", "the closed context count", bump_scalar("count"), True),
    Case("fields.toml", "the sequence header length", bump_scalar("size_bytes"), True),
    Case("fields.toml", "the packet header checksum span", bump_scalar("header_crc_length"), True),
    Case("fields.toml", "the payload checksum offset", bump_scalar("payload_crc_offset"), True),
    Case("fields.toml", "the reserved flag mask", bump_scalar("reserved_mask"), True),
    Case("constants.toml", "the scene-cut history bound", bump_scalar("scene_history_capacity"), True),
    Case("constants.toml", "the full-pixel motion range", bump_scalar("mv_fullpel_max"), True),
    Case("search.toml", "the sub-pixel round count", bump_scalar("subpel_rounds"), True),
    Case("intra.toml", "the angular interpolation scale", bump_scalar("angular_denominator"), True),
    Case("intra.toml", "the angular rounding bias", bump_scalar("angular_rounding_offset"), True),
    Case("intra.toml", "the forty-five degree angle", bump_scalar("d45"), True),
    Case("intra.toml", "the unavailable-neighbor substitute", bump_scalar("unavailable_fallback"), True),
    Case("mc.toml", "one interpolation filter tap", bump_first_in_array("filter_taps"), True),
    Case("mc.toml", "one declared phase blend weight", bump_phase_weight("luma_phase_sequences"), True),
    Case("mc.toml", "the two-stage rounding bias", bump_scalar("two_stage_rounding"), True),
    Case("mc.toml", "the quarter-pixel phase denominator", bump_scalar("phase_denominator"), True),
    Case("syntax.toml", "one coded plane name", rename_first_plane, True),
    Case("mc.toml", "one motion-vector predictor candidate", drop_last_predictor_candidate, True),
    Case("mc-vectors.toml", "one expected interpolated sample", bump_first_case_expectation, True),
    Case("vectors.json", "one oracle packet payload byte", flip_oracle_payload, True),
]


def run(script, tree):
    """Runs one specification gate against a throwaway tree."""
    result = subprocess.run(
        [sys.executable, str(tree / "spec" / script), "--check"]
        if script != "check_assets.py"
        else [sys.executable, str(tree / "spec" / script)],
        cwd=str(tree),
        capture_output=True,
        text=True,
        check=False,
    )
    return result.returncode == 0


def main():
    errors = []
    for case in CASES:
        with tempfile.TemporaryDirectory() as directory:
            tree = Path(directory)
            shutil.copytree(ROOT, tree / "spec")
            (tree / "docs").mkdir()
            shutil.copy2(REPO / "docs" / "bitstream.md", tree / "docs" / "bitstream.md")

            target = tree / "spec" / "v1" / case.asset
            original = target.read_text(encoding="utf-8")
            mutated = case.mutate(original)
            if mutated == original:
                errors.append("%s: the mutation changed nothing" % case.asset)
                continue
            target.write_text(mutated, encoding="utf-8")

            document_accepted = run("generate_docs.py", tree)
            derivation_accepted = run("check_assets.py", tree)
            oracle_accepted = run("oracle.py", tree)

            if document_accepted:
                errors.append(
                    "%s: the generated document accepted %s" % (case.asset, case.describe)
                )
            if case.semantic and derivation_accepted and oracle_accepted:
                errors.append(
                    "%s: no independent derivation rejected %s" % (case.asset, case.describe)
                )
            if document_accepted and derivation_accepted and oracle_accepted:
                errors.append("%s: every gate accepted %s" % (case.asset, case.describe))

    if errors:
        print("trap_spec_oracle_independence: FAILED")
        for error in errors:
            print(" - %s" % error)
        return 1

    print(
        "trap_spec_oracle_independence: OK — %d single-entry mutations rejected" % len(CASES)
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
