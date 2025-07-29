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


def flip_oracle_payload(text):
    """Changes one byte of one committed oracle vector."""
    document = json.loads(text)
    payload = document["packet"]["payload_hex"]
    first = payload[:2]
    replacement = "%02x" % ((int(first, 16) ^ 0x01) & 0xFF)
    document["packet"]["payload_hex"] = replacement + payload[2:]
    return json.dumps(document, indent=2) + "\n"


CASES = [
    Case("transforms.toml", "one transform scale constant", bump_scalar("dc_scale"), True),
    Case("quant.toml", "one quantizer scale entry", bump_first_in_array("qscale"), True),
    Case("costs.toml", "one modeled-entropy cost entry", bump_first_in_array("cost0_q16"), True),
    Case("contexts.toml", "the closed context count", bump_scalar("count"), True),
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
