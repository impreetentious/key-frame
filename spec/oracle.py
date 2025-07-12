#!/usr/bin/env python3
"""Independent standard-library oracle for Key Frame v1 worked vectors."""

import argparse
import json
from pathlib import Path
import re
import sys


ROOT = Path(__file__).resolve().parent
ASSETS = ROOT / "v1"
VECTOR_PATH = ASSETS / "vectors.json"


def scalar(path, name):
    text = path.read_text(encoding="utf-8")
    match = re.search(r"^%s\s*=\s*(.+)$" % re.escape(name), text, re.MULTILINE)
    if not match:
        raise ValueError("missing %s in %s" % (name, path))
    value = match.group(1).strip()
    if value.startswith('"') and value.endswith('"'):
        return value[1:-1]
    return int(value)


def crc32c(data):
    register = 0xFFFFFFFF
    for byte in data:
        register ^= byte
        for _ in range(8):
            mask = -(register & 1) & 0xFFFFFFFF
            register = ((register >> 1) ^ (0x82F63B78 & mask)) & 0xFFFFFFFF
    return register ^ 0xFFFFFFFF


def little(value, width):
    return int(value).to_bytes(width, byteorder="little", signed=False)


class RangeEncoder:
    def __init__(self, initial_range, top):
        self.low = 0
        self.range = initial_range
        self.top = top
        self.cache = 0
        self.pending = 1
        self.output = bytearray()

    def shift_low(self):
        low32 = self.low & 0xFFFFFFFF
        carry = self.low >> 32
        if carry not in (0, 1):
            raise AssertionError("range carry escaped one bit")
        if low32 < 0xFF000000 or carry != 0:
            self.output.append((self.cache + carry) & 0xFF)
            for _ in range(self.pending - 1):
                self.output.append((0xFF + carry) & 0xFF)
            self.cache = low32 >> 24
            self.pending = 0
        self.pending += 1
        self.low = (low32 & 0x00FFFFFF) << 8

    def encode(self, symbol, p1):
        p0 = 4096 - p1
        bound = (self.range >> 12) * p0
        if symbol == 0:
            self.range = bound
        else:
            self.low += bound
            self.range -= bound
        while self.range < self.top:
            self.range = (self.range << 8) & 0xFFFFFFFF
            self.shift_low()

    def finish(self):
        for _ in range(5):
            self.shift_low()
        return bytes(self.output)


def adapt(p1, symbol):
    target = 4096 if symbol else 0
    return max(1, min(4095, p1 + ((target - p1) // 32)))


def range_vector(name, symbols, initial_p1, bypass_positions, constants):
    encoder = RangeEncoder(constants["range_initial"], constants["range_top"])
    p1 = initial_p1
    bypass = set(bypass_positions)
    for index, symbol in enumerate(symbols):
        if index in bypass:
            encoder.encode(symbol, 2048)
        else:
            encoder.encode(symbol, p1)
            p1 = adapt(p1, symbol)
    encoded = encoder.finish()
    return {
        "name": name,
        "symbols": symbols,
        "initial_p1": initial_p1,
        "bypass_positions": bypass_positions,
        "bytes_hex": encoded.hex(),
        "final_p1": p1,
    }


def build_vectors():
    constants_path = ASSETS / "constants.toml"
    fields_path = ASSETS / "fields.toml"
    constants = {
        "range_initial": scalar(constants_path, "range_initial"),
        "range_top": scalar(constants_path, "range_top"),
        "bitstream_version": scalar(constants_path, "bitstream_version"),
    }

    sequence = bytearray()
    sequence.extend(scalar(fields_path, "magic_ascii").encode("ascii"))
    sequence.extend(little(constants["bitstream_version"], 2))
    sequence.extend(little(64, 2))
    sequence.extend(little(64, 2))
    sequence.extend(bytes((1, 8)))
    sequence.extend(little(24, 2))
    sequence.extend(little(1, 2))
    sequence.extend(little(120, 2))
    sequence.extend(bytes((16, 0)))
    sequence_crc = crc32c(sequence)

    ranges = [
        range_vector("all_zero", [0] * 24, 2048, [], constants),
        range_vector("all_one", [1] * 24, 2048, [], constants),
        range_vector("alternating", [0, 1] * 16, 2048, [], constants),
        range_vector("asymmetric", [1, 1, 1, 0, 1, 1, 0, 1] * 4, 3072, [], constants),
        range_vector("bypass_no_adaptation", [0, 1] * 20, 1234, list(range(40)), constants),
        range_vector("carry_cascade", [1] * 9 + [0] * 3 + [1] * 21, 4095, [], constants),
    ]
    return {
        "format": "key-frame-oracle-v1",
        "crc32c": [
            {"name": "empty", "input_hex": "", "crc32c": "00000000"},
            {
                "name": "ascii_123456789",
                "input_hex": b"123456789".hex(),
                "crc32c": "%08x" % crc32c(b"123456789"),
            },
            {
                "name": "sequence_header_prefix_64x64_24fps",
                "input_hex": bytes(sequence).hex(),
                "crc32c": "%08x" % sequence_crc,
                "complete_header_hex": (bytes(sequence) + little(sequence_crc, 4)).hex(),
            },
        ],
        "range": ranges,
    }


def encoded_vectors():
    return json.dumps(build_vectors(), indent=2, sort_keys=True) + "\n"


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--emit", action="store_true", help="print the independent vectors")
    parser.add_argument("--check", action="store_true", help="compare against the committed vectors")
    args = parser.parse_args()
    output = encoded_vectors()
    if args.emit:
        sys.stdout.write(output)
        return 0
    if args.check or not args.emit:
        committed = VECTOR_PATH.read_text(encoding="utf-8")
        if committed != output:
            print("oracle: committed vectors differ; inspect the normative change", file=sys.stderr)
            return 1
        print("oracle: OK — independent CRC32C and range vectors match")
    return 0


if __name__ == "__main__":
    sys.exit(main())
