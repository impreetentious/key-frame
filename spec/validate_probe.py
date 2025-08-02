#!/usr/bin/env python3
"""Validate kfprobe JSON against the dependency-free v1 schema subset."""

import json
from pathlib import Path
import sys


class ValidationError(ValueError):
    pass


def kind_matches(value, expected):
    if expected == "null":
        return value is None
    if expected == "boolean":
        return isinstance(value, bool)
    if expected == "integer":
        return isinstance(value, int) and not isinstance(value, bool)
    if expected == "number":
        return isinstance(value, (int, float)) and not isinstance(value, bool)
    if expected == "string":
        return isinstance(value, str)
    if expected == "array":
        return isinstance(value, list)
    if expected == "object":
        return isinstance(value, dict)
    raise ValidationError("schema uses unsupported type %r" % expected)


def validate(value, schema, path="$"):
    if "oneOf" in schema:
        matches = 0
        for candidate in schema["oneOf"]:
            try:
                validate(value, candidate, path)
                matches += 1
            except ValidationError:
                pass
        if matches != 1:
            raise ValidationError("%s: expected exactly one oneOf branch, got %d" % (path, matches))

    if "const" in schema and value != schema["const"]:
        raise ValidationError("%s: expected constant %r" % (path, schema["const"]))
    if "enum" in schema and value not in schema["enum"]:
        raise ValidationError("%s: value %r is outside enum" % (path, value))

    expected = schema.get("type")
    if expected is not None and not kind_matches(value, expected):
        raise ValidationError("%s: expected %s" % (path, expected))

    if isinstance(value, (int, float)) and not isinstance(value, bool):
        if "minimum" in schema and value < schema["minimum"]:
            raise ValidationError("%s: value is below minimum" % path)
        if "maximum" in schema and value > schema["maximum"]:
            raise ValidationError("%s: value is above maximum" % path)

    if isinstance(value, list):
        if len(value) < schema.get("minItems", 0):
            raise ValidationError("%s: array is too short" % path)
        if "maxItems" in schema and len(value) > schema["maxItems"]:
            raise ValidationError("%s: array is too long" % path)
        if "items" in schema:
            for index, item in enumerate(value):
                validate(item, schema["items"], "%s[%d]" % (path, index))

    if isinstance(value, dict):
        properties = schema.get("properties", {})
        for name in schema.get("required", []):
            if name not in value:
                raise ValidationError("%s: missing required property %s" % (path, name))
        if schema.get("additionalProperties") is False:
            unexpected = sorted(set(value) - set(properties))
            if unexpected:
                raise ValidationError("%s: unexpected properties %s" % (path, unexpected))
        for name, child in value.items():
            if name in properties:
                validate(child, properties[name], "%s.%s" % (path, name))


def main():
    if len(sys.argv) != 3:
        print("usage: validate_probe.py SCHEMA REPORT", file=sys.stderr)
        return 2
    schema = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
    report = json.loads(Path(sys.argv[2]).read_text(encoding="utf-8"))
    try:
        validate(report, schema)
    except ValidationError as error:
        print("probe-schema: FAILED — %s" % error, file=sys.stderr)
        return 1
    print("probe-schema: OK — report matches frozen probe version 1")
    return 0


if __name__ == "__main__":
    sys.exit(main())
