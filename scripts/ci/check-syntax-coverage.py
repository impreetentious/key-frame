#!/usr/bin/env python3
"""Prove the syntax-coverage inventory matches the frozen 144-context asset."""

from __future__ import annotations

import ast
from pathlib import Path
import re
import sys


ROOT = Path(__file__).resolve().parents[2]
CONTEXTS = ROOT / "spec" / "v1" / "contexts.toml"
COVERAGE = ROOT / "conformance" / "syntax-coverage.toml"
SYNTAX = ROOT / "spec" / "v1" / "syntax.toml"


def toml_arrays(path: Path, key: str) -> list:
    values = []
    for match in re.finditer(r"^%s\s*=\s*(\[.*\])$" % re.escape(key), path.read_text(encoding="utf-8"), re.MULTILINE):
        values.append(ast.literal_eval(match.group(1)))
    return values


def toml_names(path: Path, header: str) -> list[str]:
    names = []
    in_section = False
    for line in path.read_text(encoding="utf-8").splitlines():
        if line.strip() == header:
            in_section = True
            continue
        if line.startswith("[") and line.strip() != header:
            in_section = False
        if in_section:
            match = re.match(r'^name\s*=\s*"([^"]+)"\s*$', line)
            if match:
                names.append(match.group(1))
    return names


def scalar_int(path: Path, key: str) -> int | None:
    match = re.search(r"^%s\s*=\s*(-?\d+)\s*$" % re.escape(key), path.read_text(encoding="utf-8"), re.MULTILINE)
    return int(match.group(1)) if match else None


def main() -> int:
    errors = []
    context_ids = []
    for group in toml_arrays(CONTEXTS, "ids"):
        context_ids.extend(group)
    coverage_ids = []
    for group in toml_arrays(COVERAGE, "ids"):
        coverage_ids.extend(group)

    if context_ids != list(range(144)):
        errors.append("contexts.toml ids are not the contiguous range 0..143")
    if coverage_ids != context_ids:
        errors.append("syntax-coverage.toml ids must equal the frozen context id list")
    if scalar_int(COVERAGE, "context_count") != 144:
        errors.append("syntax-coverage.toml context_count must be 144")
    if scalar_int(COVERAGE, "bitstream_version") != 1:
        errors.append("syntax-coverage.toml bitstream_version must be 1")

    context_groups = toml_names(CONTEXTS, "[[groups]]")
    coverage_groups = toml_names(COVERAGE, "[[groups]]")
    if context_groups != coverage_groups:
        errors.append("coverage groups %s != frozen groups %s" % (coverage_groups, context_groups))

    # Every group states which of its ids a version-one stream can code and
    # which it cannot. The two halves must partition the group exactly: an id
    # that is in neither would be silently exempt from the measuring gate, and
    # one in both would be claimed and excused at the same time.
    reachable = toml_arrays(COVERAGE, "reachable")
    reserved = toml_arrays(COVERAGE, "reserved")
    group_ids = toml_arrays(COVERAGE, "ids")
    if not (len(reachable) == len(reserved) == len(group_ids)):
        errors.append("every coverage group needs one ids, one reachable, and one reserved array")
    else:
        held = 0
        for name, ids, live, dead in zip(coverage_groups, group_ids, reachable, reserved):
            if sorted(live + dead) != ids:
                errors.append("group %s: reachable plus reserved is not its id list" % name)
            if set(live) & set(dead):
                errors.append("group %s: an id is both reachable and reserved" % name)
            held += len(dead)
        reasons = re.findall(r'^reserved_reason\s*=\s*"([^"]*)"\s*$', COVERAGE.read_text(encoding="utf-8"), re.MULTILINE)
        if len(reasons) != len(coverage_groups):
            errors.append("every coverage group needs a reserved_reason, even an empty one")
        else:
            for name, dead, reason in zip(coverage_groups, reserved, reasons):
                if dead and not reason:
                    errors.append("group %s reserves ids without saying why" % name)
                if not dead and reason:
                    errors.append("group %s reserves nothing but gives a reason" % name)

    required_elements = [
        "partition_tree", "skip", "is_inter", "intra_mode", "ref_select", "mvd",
        "has_coeff", "last_x", "last_y", "sig", "gt1", "gt2", "magnitude_remainder", "nonzero_sign",
    ]
    coverage_elements = toml_names(COVERAGE, "[[elements]]")
    if coverage_elements != required_elements:
        errors.append("coverage elements %s != required %s" % (coverage_elements, required_elements))

    coeff_elements = None
    in_coeff = False
    for line in SYNTAX.read_text(encoding="utf-8").splitlines():
        if line.strip() == "[coefficient_block]":
            in_coeff = True
            continue
        if line.startswith("[") and in_coeff:
            break
        if in_coeff and line.startswith("elements = "):
            coeff_elements = ast.literal_eval(line.split("=", 1)[1].strip())
            break
    if coeff_elements != ["has_coeff", "last_x", "last_y", "sig", "gt1", "gt2", "magnitude_remainder", "nonzero_sign"]:
        errors.append("syntax.toml coefficient elements drifted from the coverage inventory")

    vector_lines = [
        line for line in COVERAGE.read_text(encoding="utf-8").splitlines()
        if line.startswith("vectors = ")
    ]
    for line in vector_lines:
        vectors = ast.literal_eval(line.split("=", 1)[1].strip())
        if not vectors:
            errors.append("every coverage row needs at least one vector")
        for vector in vectors:
            if not re.match(r"^(oracle|hand|encoder):[A-Za-z0-9_]+$", vector):
                errors.append("vector %s is not origin:name" % vector)

    if errors:
        print("syntax-coverage: FAILED")
        for error in errors:
            print(" - %s" % error)
        return 1
    print(
        "syntax-coverage: OK — 144 ids across %d groups, %d held in reserve, %d elements"
        % (len(coverage_groups), sum(len(dead) for dead in reserved), len(coverage_elements))
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
