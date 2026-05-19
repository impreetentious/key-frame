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


def block_array(path, key):
    """Reads an array that spans several lines, as the phase tables do."""
    text = path.read_text(encoding="utf-8")
    match = re.search(r"^%s\s*=\s*(\[.*?\n\])" % re.escape(key), text, re.MULTILINE | re.DOTALL)
    if not match:
        raise AssertionError("%s: missing array %s" % (path, key))
    return ast.literal_eval(match.group(1))


def rounded(value):
    return int(math.floor(value + 0.5))


def scalar_int(path, key):
    """Reads a declared integer scalar so it can be checked, not just rendered."""
    text = path.read_text(encoding="utf-8")
    match = re.search(r"^%s\s*=\s*(-?\d+)\s*$" % re.escape(key), text, re.MULTILINE)
    if not match:
        return None
    return int(match.group(1))


def scalar_text(path, key):
    """Reads a declared scalar that may be a quoted formula rather than a number."""
    text = path.read_text(encoding="utf-8")
    match = re.search(r'^%s\s*=\s*"?([^"\n]+?)"?\s*$' % re.escape(key), text, re.MULTILINE)
    if not match:
        raise AssertionError("%s: missing scalar %s" % (path, key))
    return match.group(1).strip()


def declared_shift(key, size):
    """Evaluates a declared stage shift for one block size.

    The shifts are the asset's own statement of how the transform is scaled.
    Restating their values here would make this checker agree with a copy of
    the numbers rather than with the specification, which is the failure this
    function exists to prevent, so the declared formula is evaluated instead.
    Only the three shapes the asset actually uses are accepted.
    """
    formula = scalar_text(V1 / "transforms.toml", key).replace(" ", "")
    log2_side = size.bit_length() - 1
    if re.fullmatch(r"-?\d+", formula):
        return int(formula)
    match = re.fullmatch(r"log2\(N\)\+(\d+)", formula)
    if match:
        return log2_side + int(match.group(1))
    match = re.fullmatch(r"(\d+)-log2\(N\)", formula)
    if match:
        return int(match.group(1)) - log2_side
    raise AssertionError("transforms.toml: unevaluatable shift formula for %s: %s" % (key, formula))


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
    # The closed asset set comes from the manifest that declares it. It used to
    # be restated here as a literal list, which made the manifest decoration:
    # the two could name different sets and this checker would go on verifying
    # its own copy while the published specification described another one.
    manifest = V1 / "manifest.toml"
    required = block_array(manifest, "required")
    for name in required:
        if not (V1 / name).is_file():
            errors.append("missing normative asset %s" % name)
    # And the reverse direction, which is the one that actually drifts: an asset
    # added to the directory and never added to the manifest is an asset the
    # frozen set does not contain, however normative it looks from inside.
    present = sorted(
        path.name
        for path in V1.iterdir()
        if path.is_file() and path.name != manifest.name
    )
    if sorted(required) != present:
        errors.append(
            "manifest.toml required (%s) is not the contents of spec/v1 (%s)"
            % (", ".join(sorted(required)), ", ".join(present))
        )

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
    # The declared count is normative prose everywhere else in the repository.
    # If nothing compares it to the enumerated ids it is decoration, and a
    # mutation to it survives every gate but the document hash.
    if scalar_int(V1 / "contexts.toml", "count") != len(ids):
        errors.append("contexts.toml count disagrees with the enumerated ids")
    if len(initials) != 144 or not all(1 <= value <= 4095 for value in initials):
        errors.append("contexts.toml must contain 144 legal p1 initials")
    if suffix_count != 144:
        errors.append("contexts.toml must contain 144 stable name suffixes")

    # The matrices below are derived from first principles rather than from the
    # asset's own scalars, so the declared scalars need their own comparison or
    # they are unverified normative text.
    if scalar_int(V1 / "transforms.toml", "dc_scale") != 64:
        errors.append("transforms.toml dc_scale differs from the reviewed derivation")
    if scalar_int(V1 / "transforms.toml", "coefficient_scale_bits") != 7:
        errors.append("transforms.toml coefficient_scale_bits differs from the reviewed derivation")
    if scalar_int(V1 / "transforms.toml", "post_inverse_min") != -32768:
        errors.append("transforms.toml post_inverse_min differs from the reviewed derivation")
    if scalar_int(V1 / "transforms.toml", "post_inverse_max") != 32767:
        errors.append("transforms.toml post_inverse_max differs from the reviewed derivation")
    # The first inverse stage exists to undo the matrix scale, so these two
    # declarations state one fact twice and must agree.
    if declared_shift("inverse_shift1", 4) != scalar_int(V1 / "transforms.toml", "coefficient_scale_bits"):
        errors.append("transforms.toml inverse_shift1 does not undo coefficient_scale_bits")

    # The packet and sequence headers are the most safety-critical declarations
    # in the tree — a decoder reads bytes at these offsets before it trusts
    # anything — and every scalar around them was unverified prose. They are all
    # derivable from the field layout the same asset lists, so they are derived
    # here rather than taken on faith.
    fields_path = V1 / "fields.toml"
    fields_text = fields_path.read_text(encoding="utf-8")
    widths = {"u8": 1, "u16": 2, "u32": 4}

    def layout(section):
        """Returns {name: (offset, width)} for one header's declared fields."""
        body = fields_text.split("[%s]" % section, 1)[1]
        listing = re.search(r"fields = \[(.*?)\]", body, re.DOTALL)
        if not listing:
            raise AssertionError("fields.toml: %s declares no field list" % section)
        parsed = {}
        for entry in re.findall(r'"([^"]+)"', listing.group(1)):
            name, rest = entry.split(":", 1)
            kind, offset = rest.split("@", 1)
            parsed[name] = (int(offset), widths.get(kind))
        return parsed

    def scoped(section, key):
        """Reads a scalar from one section, not from whichever section is first."""
        body = fields_text.split("[%s]" % section, 1)[1].split("\n[", 1)[0]
        match = re.search(r"^%s\s*=\s*(-?\d+)\s*$" % re.escape(key), body, re.MULTILINE)
        return int(match.group(1)) if match else None

    try:
        sequence = layout("sequence")
        crc_offset, crc_width = sequence["header_crc32c"]
        if scoped("sequence", "size_bytes") != crc_offset + crc_width:
            errors.append("fields.toml sequence size_bytes is not the end of its last field")
        if scoped("sequence", "crc_offset") != crc_offset:
            errors.append("fields.toml sequence crc_offset is not where header_crc32c sits")
        if scoped("sequence", "crc_start") != 0:
            errors.append("fields.toml sequence crc_start must cover the header from its first byte")
        if scoped("sequence", "crc_length") != crc_offset - scoped("sequence", "crc_start"):
            errors.append("fields.toml sequence crc_length does not reach the checksum it protects")

        packet = layout("packet")
        header_crc, _ = packet["header_crc32c"]
        payload_crc, payload_crc_width = packet["payload_crc32c"]
        payload_offset, _ = packet["payload"]
        sync_offset, sync_width = packet["sync"]
        if scoped("packet", "header_crc_offset") != header_crc:
            errors.append("fields.toml packet header_crc_offset is not where header_crc32c sits")
        if scoped("packet", "payload_crc_offset") != payload_crc:
            errors.append("fields.toml packet payload_crc_offset is not where payload_crc32c sits")
        if scoped("packet", "fixed_size_bytes") != payload_offset:
            errors.append("fields.toml packet fixed_size_bytes is not where the payload starts")
        if payload_crc + payload_crc_width != payload_offset:
            errors.append("fields.toml packet leaves a gap between its checksums and its payload")
        # The header checksum starts after the sync marker: resynchronization
        # scans for the marker, so the marker cannot be part of what the
        # checksum protects.
        if scoped("packet", "header_crc_start") != sync_offset + sync_width:
            errors.append("fields.toml packet header_crc_start does not begin after the sync marker")
        if scoped("packet", "header_crc_length") != header_crc - scoped("packet", "header_crc_start"):
            errors.append("fields.toml packet header_crc_length does not reach the checksum it protects")

        # The flag masks restate the bit positions listed beside them.
        key_bit = scoped("packet.flags", "key")
        golden_bit = scoped("packet.flags", "golden_refresh")
        show_bit = scoped("packet.flags", "show")
        named = (1 << key_bit) | (1 << golden_bit) | (1 << show_bit)
        if scoped("packet.flags", "reserved_mask") != (0xFF & ~named):
            errors.append("fields.toml reserved_mask is not exactly the unnamed flag bits")
        if scoped("packet.flags", "required_mask") != 1 << show_bit:
            errors.append("fields.toml required_mask is not the show bit")
        if scoped("packet.flags", "key_requires_mask") != 1 << golden_bit:
            errors.append("fields.toml key_requires_mask is not the golden-refresh bit")
    except (AssertionError, AttributeError, KeyError, TypeError, ValueError) as error:
        errors.append("fields.toml is malformed: %s" % error)

    # The last of the declared scalars that nothing else reaches. Each one is
    # implied by something the assets already say, so each is derived rather
    # than restated; between them and the checks above, every numeric scalar in
    # the tree is now load-bearing.
    constants = V1 / "constants.toml"
    search = V1 / "search.toml"
    manifest = V1 / "manifest.toml"
    deblock = V1 / "deblock.toml"
    if scalar_int(manifest, "bitstream_version") != scalar_int(constants, "bitstream_version"):
        errors.append("manifest.toml and constants.toml disagree on the bitstream version")
    if scalar_int(manifest, "asset_version") < 1:
        errors.append("manifest.toml asset_version must be a positive revision")
    if scalar_int(constants, "bit_depth") != 8:
        errors.append("constants.toml bit_depth is not the eight bits every other table assumes")
    # The chroma code and name are one fact written twice.
    if scalar_int(constants, "chroma_code") != 1 or scalar_text(constants, "chroma_name") != "C420jpeg":
        errors.append("constants.toml chroma_code and chroma_name do not name the same format")
    if scalar_int(constants, "reference_slots") != len(array(search, "reference_order")):
        errors.append("constants.toml reference_slots disagrees with the enumerated references")
    # Deblocking runs on coding-block and transform edges, so the smallest edge
    # it can meet is the smallest partition the search can choose.
    if scalar_int(deblock, "minimum_edge_px") != min(array(search, "partition_sizes")):
        errors.append("deblock.toml minimum_edge_px is not the smallest declared partition")
    # Every decision row starts at the bottom of the declared quantizer range.
    declared_minimum_qp = scalar_int(constants, "qp_min")
    for row in re.findall(r"^minimum_qp = (-?\d+)$", deblock.read_text(encoding="utf-8"), re.MULTILINE):
        if int(row) != declared_minimum_qp:
            errors.append("deblock.toml minimum_qp does not start at the declared quantizer floor")
    # The defaults have to be usable values of the fields they default.
    if not 0 < scalar_int(constants, "default_keyframe_interval") <= 65535:
        errors.append("constants.toml default_keyframe_interval does not fit its u16 field")
    if not 0 < scalar_int(constants, "default_golden_interval") <= 255:
        errors.append("constants.toml default_golden_interval does not fit its u8 field")
    if scalar_int(constants, "default_golden_interval") > scalar_int(constants, "default_keyframe_interval"):
        errors.append("constants.toml refreshes GOLDEN less often than it sends a keyframe")
    # CIF is twenty-five frames a second, and the page states the interval in
    # milliseconds; the two have to be the same rate.
    if scalar_int(constants, "inspector_cif_frame_milliseconds") * 25 != 1000:
        errors.append("constants.toml inspector_cif_frame_milliseconds is not the 25fps CIF interval")
    # The campaign the build runs before every commit has to be cheaper than the
    # one that runs overnight, or preflight is the nightly job under another
    # name and nobody would run it.
    if scalar_int(constants, "fuzz_iterations_preflight") >= scalar_int(
        constants, "fuzz_iterations_per_target"
    ):
        errors.append("constants.toml fuzz_iterations_preflight is not cheaper than the nightly budget")

    # The enumerations that name the same closed sets as other assets. Each was
    # declared and read by nothing, so a set could gain or lose a member in one
    # place and the rest of the tree would go on describing the old one.
    syntax_asset = V1 / "syntax.toml"
    intra = V1 / "intra.toml"
    if sorted(array(constants, "coding_block_sizes")) != sorted(array(search, "partition_sizes")):
        errors.append("constants.toml coding_block_sizes and search.toml partition_sizes name different sets")
    if sorted(array(constants, "transform_sizes")) != [4, 8, 16, 32]:
        errors.append("constants.toml transform_sizes is not the four declared transform sizes")
    for size in array(constants, "transform_sizes"):
        if not (V1 / "transforms.toml").read_text(encoding="utf-8").count("n%d = [" % size):
            errors.append("constants.toml transform_sizes names %d with no matching matrix" % size)
    if array(syntax_asset, "intra_modes") != array(intra, "modes"):
        errors.append("syntax.toml intra_modes and intra.toml modes name different modes in different orders")
    if array(syntax_asset, "intra_modes") != array(search, "intra_mode_order"):
        errors.append("syntax.toml intra_modes and search.toml intra_mode_order disagree")
    # The plane vocabulary and the residual coding order are one fact written
    # twice: the order string is the plane list with underscores between it.
    # Nothing reconciled them, so the list could name planes the residual order
    # did not code and the normative document would print both without comment.
    declared_planes = array(syntax_asset, "planes")
    if len(declared_planes) != len(set(declared_planes)):
        errors.append("syntax.toml planes names the same plane twice")
    if scalar_text(syntax_asset, "residual_plane_order") != "_".join(declared_planes):
        errors.append("syntax.toml residual_plane_order does not spell out its own plane list")
    # The motion-vector predictor's own declarations, reconciled with each
    # other. A componentwise median needs an odd number of candidates to have a
    # middle one, and the substitute for an unavailable candidate has to be a
    # motion vector rather than any other shape of number.
    mc_asset = V1 / "mc.toml"
    predictor_candidates = array(mc_asset, "candidates")
    if scalar_text(mc_asset, "combine") == "componentwise_median" and len(predictor_candidates) % 2 != 1:
        errors.append("mc.toml combines an even number of predictor candidates by median")
    if len(array(mc_asset, "unavailable")) != 2:
        errors.append("mc.toml unavailable is not a two-component motion vector")
    for name in predictor_candidates:
        halves = name.split("_else_")
        if len(halves) > 2 or (len(halves) == 2 and halves[0] == halves[1]):
            errors.append("mc.toml predictor candidate %s is not a choice between two distinct neighbours" % name)
    # Intra prediction runs on coding blocks and on transform blocks, so the
    # sizes it declares are the union of both.
    if sorted(array(intra, "sizes")) != sorted(set(array(constants, "coding_block_sizes")) | set(array(constants, "transform_sizes"))):
        errors.append("intra.toml sizes is not the union of the coding-block and transform sizes")

    # The deblock vocabulary has to be exactly what its own decision rows use.
    deblock_text = deblock.read_text(encoding="utf-8")
    used_kinds = sorted(set(re.findall(r'^edge_kind = "([^"]+)"$', deblock_text, re.MULTILINE)))
    used_strengths = sorted(set(int(value) for value in re.findall(r"^strength = (\d+)$", deblock_text, re.MULTILINE)))
    if sorted(array(deblock, "edge_kinds")) != used_kinds:
        errors.append("deblock.toml edge_kinds does not match the kinds its decision rows use")
    if not set(used_strengths) <= set(array(deblock, "strengths")):
        errors.append("deblock.toml uses a strength its own vocabulary does not declare")

    # The header rules have to name fields the header actually has.
    try:
        sequence_fields = set(layout("sequence"))
        for rule in ("required_nonzero", "reserved_zero"):
            for name in array(fields_path, rule):
                if name not in sequence_fields:
                    errors.append("fields.toml %s names %s, which the sequence header does not contain" % (rule, name))
    except (AssertionError, KeyError, ValueError) as error:
        errors.append("fields.toml header rules are malformed: %s" % error)

    # The declared sub-pixel round count is the length of the declared step
    # list. The encoder asserts this too, but the assertion runs only when the
    # Rust is built, and this harness never builds it.
    if scalar_int(V1 / "search.toml", "subpel_rounds") != len(array(V1 / "search.toml", "subpel_steps_q4")):
        errors.append("search.toml subpel_rounds disagrees with the enumerated sub-pixel steps")

    # Two assets state some of the same facts. That is reasonable — one is the
    # format's constant table and the other is the component that uses them —
    # but only if they are made to agree. Nothing reconciled them before, so
    # editing either one left the normative document quietly contradicting
    # itself, with both halves passing every gate.
    constants = V1 / "constants.toml"
    search = V1 / "search.toml"
    mc_asset = V1 / "mc.toml"
    duplicated = [
        (constants, "scene_history_capacity", search, "history_capacity"),
        (constants, "scene_history_minimum", search, "history_minimum"),
        (constants, "mv_fullpel_min", mc_asset, "fullpel_search_min"),
        (constants, "mv_fullpel_max", mc_asset, "fullpel_search_max"),
        (constants, "qp_min", V1 / "quant.toml", "qp_min"),
        (constants, "qp_max", V1 / "quant.toml", "qp_max"),
        (constants, "coefficient_abs_max", V1 / "quant.toml", "coefficient_abs_max"),
        (constants, "probability_min", V1 / "costs.toml", "probability_min"),
        (constants, "probability_max", V1 / "costs.toml", "probability_max"),
    ]
    for left_path, left_key, right_path, right_key in duplicated:
        left = scalar_int(left_path, left_key)
        right = scalar_int(right_path, right_key)
        if left is None or right is None or left != right:
            errors.append(
                "%s %s (%s) and %s %s (%s) state the same fact and disagree"
                % (left_path.name, left_key, left, right_path.name, right_key, right)
            )

    # The motion-vector range is stated in full pixels and consumed in quarter
    # pixels, so the fractional precision has to be the one the asset declares.
    if scalar_int(constants, "mv_fractional_bits") != scalar_int(mc_asset, "phase_denominator").bit_length() - 1:
        errors.append("constants.toml mv_fractional_bits does not match the declared phase denominator")

    # The intra asset states its interpolation scale, its rounding bias, and its
    # four angles. The angles are geometry, not taste: at 45 degrees the
    # projection advances exactly one reference sample per row, so the diagonal
    # angles are the denominator itself, and the two shallow angles have to fall
    # strictly inside that. Checked here because no Python gate replays intra
    # prediction, which would otherwise leave all six as unverified prose.
    intra = V1 / "intra.toml"
    angular_denominator = scalar_int(intra, "angular_denominator")
    if angular_denominator <= 0 or angular_denominator & (angular_denominator - 1):
        errors.append("intra.toml angular_denominator is not a positive power of two")
    if scalar_int(intra, "angular_rounding_offset") * 2 != angular_denominator:
        errors.append("intra.toml angular_rounding_offset is not half of the angular scale")
    if scalar_int(intra, "unavailable_fallback") != 128:
        errors.append("intra.toml unavailable_fallback is not the mid-grey 8-bit substitute")
    if scalar_int(intra, "d45") != angular_denominator:
        errors.append("intra.toml d45 must advance one reference sample per row")
    if scalar_int(intra, "d135") != -angular_denominator:
        errors.append("intra.toml d135 must be the negative of d45")
    for shallow in ("d117", "d153"):
        value = scalar_int(intra, shallow)
        if not -angular_denominator < value < 0:
            errors.append("intra.toml %s is not a shallow negative angle" % shallow)
    if len(array(intra, "modes")) != 8:
        errors.append("intra.toml must declare the eight version-one intra modes")

    # The motion asset states one scaling scheme several ways: a filter
    # denominator, a per-stage shift, and the rounding bias that goes with each
    # shift. The interpolation vectors cannot police them, because every
    # committed case uses a small motion vector and a one-off rounding bias
    # usually rounds to the same sample. So the relations between them are
    # checked directly: each is a fact the others already imply.
    mc = V1 / "mc.toml"
    filter_denominator = scalar_int(mc, "filter_denominator")
    single_shift = scalar_int(mc, "single_stage_shift")
    two_shift = scalar_int(mc, "two_stage_shift")
    if filter_denominator != 1 << single_shift:
        errors.append("mc.toml filter_denominator is not two to the single_stage_shift")
    if scalar_int(mc, "single_stage_rounding") != 1 << (single_shift - 1):
        errors.append("mc.toml single_stage_rounding is not half of the single-stage scale")
    if two_shift != 2 * single_shift:
        errors.append("mc.toml two_stage_shift is not two single stages")
    if scalar_int(mc, "two_stage_rounding") != 1 << (two_shift - 1):
        errors.append("mc.toml two_stage_rounding is not half of the two-stage scale")
    if scalar_int(mc, "phase_denominator") != len(array(mc, "phase_numerators")):
        errors.append("mc.toml phase_denominator disagrees with the enumerated luma phases")
    if scalar_int(mc, "chroma_phase_denominator") != len(array(mc, "chroma_phase_numerators")):
        errors.append("mc.toml chroma_phase_denominator disagrees with the enumerated chroma phases")
    if len(array(mc, "filter_taps")) != 6 or sum(array(mc, "filter_taps")) != filter_denominator:
        errors.append("mc.toml filter_taps must be six taps summing to the filter denominator")

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
                horizontal[fy * size + x] = max(
                    -(1 << 31), min((1 << 31) - 1, rounded_shift(total, declared_shift("inverse_shift1", size)))
                )
        shift = declared_shift("inverse_shift2", size)
        floor = scalar_int(V1 / "transforms.toml", "post_inverse_min")
        ceiling = scalar_int(V1 / "transforms.toml", "post_inverse_max")
        output = [0] * (size * size)
        for y in range(size):
            for x in range(size):
                total = sum(horizontal[fy * size + x] * matrix[fy * size + y] for fy in range(size))
                output[y * size + x] = max(floor, min(ceiling, rounded_shift(total, shift)))
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

    first_qp = [max(0, 63 - 2 * index) for index in range(32)]
    if array(V1 / "quant.toml", "first_qp") != first_qp:
        errors.append("quant.toml first_qp differs from 63-2*i")
    if scalar_int(V1 / "quant.toml", "fractional_bits") != 16:
        errors.append("quant.toml fractional_bits must be 16")
    if scalar_int(V1 / "quant.toml", "window_frames") != 8:
        errors.append("quant.toml window_frames must be 8")
    if scalar_int(V1 / "quant.toml", "bucket_multiple") != 2:
        errors.append("quant.toml bucket_multiple must be 2")
    if scalar_int(V1 / "quant.toml", "maximum_qp_step") != 2:
        errors.append("quant.toml maximum_qp_step must be 2")
    rc_header = (V1 / "quant.toml").read_text(encoding="utf-8")
    if 'initial_fill = "capacity/2"' not in rc_header:
        errors.append("quant.toml initial_fill must be capacity/2")

    def next_qp(fill, capacity, qp, step=2):
        low = capacity // 3
        high = (2 * capacity) // 3
        if fill < low:
            return max(0, qp - step)
        if fill > high:
            return min(63, qp + step)
        return qp

    rc_text = (V1 / "quant.toml").read_text(encoding="utf-8")
    rc_cases = re.findall(
        r'\[\[rc_vectors\]\]\nname = "([^"]+)"\nfill_bits = (\d+)\ncapacity_bits = (\d+)\nqp_in = (\d+)\nqp_out = (\d+)',
        rc_text,
    )
    if len(rc_cases) != 5:
        errors.append("quant.toml must contain 5 reviewed rate-control vectors")
    for name, fill_text, capacity_text, qp_in_text, qp_out_text in rc_cases:
        got = next_qp(int(fill_text), int(capacity_text), int(qp_in_text))
        if got != int(qp_out_text):
            errors.append("rate-control vector %s expected %s got %s" % (name, qp_out_text, got))

    def mc_sample(source, width, height, x, y):
        x = max(0, min(width - 1, x))
        y = max(0, min(height - 1, y))
        return source[y * width + x]

    def mc_blend(left, right, right_weight, denominator):
        return (left * (denominator - right_weight) + right * right_weight + denominator // 2) // denominator

    def mc_phase(integer, half, next_integer, phase, denominator):
        half_phase = denominator // 2
        if phase == 0:
            return integer
        if phase == half_phase:
            return half
        if phase < half_phase:
            return mc_blend(integer, half, phase, half_phase)
        return mc_blend(half, next_integer, phase - half_phase, half_phase)

    def check_phase_sequences(key, denominator):
        """The declared blend table and the blend the codec performs, compared.

        `mc.toml` enumerates, for every phase, which two reference positions are
        blended, with what weight, over what denominator, and with what rounding
        bias. Nothing read that table: both decoders and this checker derive the
        blend from the phase index instead, so the enumeration was normative
        text describing an algorithm nobody compared it against. The two are
        compared here on sample values rather than symbolically, because the
        table writes its fractions reduced — a chroma quarter-phase is declared
        as one half over two, not two over four — and reduced fractions are
        equal without being identical.
        """
        positions = {0: "integer", 1: "half", 2: "next"}
        for phase, sequence in enumerate(block_array(V1 / "mc.toml", key)):
            if len(sequence) != 5:
                errors.append("mc.toml %s phase %d is not a five-field blend" % (key, phase))
                continue
            left_position, right_position, right_weight, weight_denominator, bias = sequence
            if left_position not in positions or right_position not in positions:
                errors.append("mc.toml %s phase %d names an unknown reference position" % (key, phase))
                continue
            if bias * 2 != weight_denominator and not (weight_denominator == 1 and bias == 0):
                errors.append("mc.toml %s phase %d has a bias that is not half its denominator" % (key, phase))
            # Sample values chosen so every position is distinguishable and the
            # weights cannot coincide by accident.
            for integer, half, following in ((0, 255, 17), (255, 0, 200), (40, 90, 130), (7, 7, 7)):
                samples = {"integer": integer, "half": half, "next": following}
                left = samples[positions[left_position]]
                right = samples[positions[right_position]]
                declared = (
                    left * (weight_denominator - right_weight) + right * right_weight + bias
                ) // weight_denominator
                performed = mc_phase(integer, half, following, phase, denominator)
                if declared != performed:
                    errors.append(
                        "mc.toml %s phase %d declares %d where the blend produces %d"
                        % (key, phase, declared, performed)
                    )
                    break

    check_phase_sequences("luma_phase_sequences", scalar_int(V1 / "mc.toml", "phase_denominator"))
    check_phase_sequences(
        "chroma_phase_sequences", scalar_int(V1 / "mc.toml", "chroma_phase_denominator")
    )

    def mc_vector(source, width, height, block_x, block_y, block_size, mv, denominator):
        # Taken from the asset, not restated. A literal copy here would agree
        # with the Rust's literals and with nothing else, which is exactly the
        # drift this checker exists to catch.
        taps = array(V1 / "mc.toml", "filter_taps")
        scale = scalar_int(V1 / "mc.toml", "filter_denominator")
        rounding = scalar_int(V1 / "mc.toml", "two_stage_rounding")
        shift = scalar_int(V1 / "mc.toml", "two_stage_shift")

        def horizontal(x, y, phase):
            integer = mc_sample(source, width, height, x, y) * scale
            half = sum(tap * mc_sample(source, width, height, x + index - 2, y) for index, tap in enumerate(taps))
            following = mc_sample(source, width, height, x + 1, y) * scale
            return mc_phase(integer, half, following, phase, denominator)

        x_integer, x_phase = divmod(mv[0], denominator)
        y_integer, y_phase = divmod(mv[1], denominator)
        output = []
        for row in range(block_size):
            for column in range(block_size):
                x = block_x + column + x_integer
                y = block_y + row + y_integer
                if y_phase == 0:
                    scaled = horizontal(x, y, x_phase) * scale
                else:
                    half = sum(tap * horizontal(x, y + index - 2, x_phase) for index, tap in enumerate(taps))
                    integer = horizontal(x, y, x_phase) * scale
                    following = horizontal(x, y + 1, x_phase) * scale
                    scaled = mc_phase(integer, half, following, y_phase, denominator)
                output.append(max(0, min(255, (scaled + rounding) >> shift)))
        return output

    try:
        mc_path = V1 / "mc-vectors.toml"
        mc_text = mc_path.read_text(encoding="utf-8")
        source = array(mc_path, "source")
        scalar_values = {}
        for key in ("source_width", "source_height", "block_x", "block_y", "block_size"):
            match = re.search(r"^%s\s*=\s*(\d+)$" % key, mc_text, re.MULTILINE)
            if not match:
                raise ValueError("missing scalar %s" % key)
            scalar_values[key] = int(match.group(1))
        cases = re.findall(
            r'\[\[cases\]\]\nname = "([^"]+)"\nscale = "([^"]+)"\nmv_q4 = (\[.*\])\nexpected = (\[.*\])',
            mc_text,
        )
        if 'format = "key-frame-mc-vectors-v1"' not in mc_text or len(cases) != 24:
            errors.append("mc-vectors.toml must contain the 24 reviewed luma/chroma phase cases")
        for name, scale, mv_text, expected_text in cases:
            denominator = (
                scalar_int(V1 / "mc.toml", "phase_denominator")
                if scale == "luma"
                else scalar_int(V1 / "mc.toml", "chroma_phase_denominator")
            )
            expected = mc_vector(
                source,
                scalar_values["source_width"],
                scalar_values["source_height"],
                scalar_values["block_x"],
                scalar_values["block_y"],
                scalar_values["block_size"],
                ast.literal_eval(mv_text),
                denominator,
            )
            if ast.literal_eval(expected_text) != expected:
                errors.append("MC vector %s differs from the literal phase path" % name)
        stage_source = array(mc_path, "stage_order_source")
        stage_mv = array(mc_path, "stage_order_mv_q4")
        stage_expected = mc_vector(stage_source, 8, 8, 2, 2, 4, stage_mv, 4)
        transposed_source = [stage_source[x * 8 + y] for y in range(8) for x in range(8)]
        transposed_block = mc_vector(transposed_source, 8, 8, 2, 2, 4, stage_mv[::-1], 4)
        vertical_first = [transposed_block[x * 4 + y] for y in range(4) for x in range(4)]
        if array(mc_path, "stage_order_expected") != stage_expected:
            errors.append("MC stage-order vector differs from horizontal-before-vertical")
        if array(mc_path, "stage_order_vertical_first") != vertical_first or vertical_first == stage_expected:
            errors.append("MC stage-order trap does not distinguish the reversed path")
    except (AssertionError, KeyError, ValueError) as error:
        errors.append("mc-vectors.toml is malformed: %s" % error)

    cost0 = array(V1 / "costs.toml", "cost0_q16")
    cost1 = array(V1 / "costs.toml", "cost1_q16")
    expected0 = [rounded(-math.log((4096 - p1) / 4096.0, 2) * 65536) for p1 in range(1, 4096)]
    expected1 = [rounded(-math.log(p1 / 4096.0, 2) * 65536) for p1 in range(1, 4096)]
    if cost0 != expected0 or cost1 != expected1:
        errors.append("costs.toml entropy costs differ from the C.4 Q16 derivation")

    def clip_int(value, lo, hi):
        return max(lo, min(hi, value))

    def deblock_weak(samples, qp):
        p3, p2, p1, p0, q0, q1, q2, q3 = samples
        alpha = array(V1 / "deblock.toml", "alpha")[qp]
        beta = array(V1 / "deblock.toml", "beta")[qp]
        tc = array(V1 / "deblock.toml", "tc")[qp]
        if abs(p0 - q0) >= alpha or abs(p1 - p0) >= beta or abs(q1 - q0) >= beta:
            return samples[:]
        delta = clip_int(((q0 - p0) * 4 + (p1 - q1) + 4) >> 3, -tc, tc)
        return [p3, p2, p1, clip_int(p0 + delta, 0, 255), clip_int(q0 - delta, 0, 255), q1, q2, q3]

    def deblock_strong(samples):
        p3, p2, p1, p0, q0, q1, q2, q3 = samples
        p0_out = (p2 + 2 * p1 + 2 * p0 + 2 * q0 + q1 + 4) >> 3
        p1_out = (p2 + p1 + p0 + q0 + 2) >> 2
        p2_out = (2 * p3 + 3 * p2 + p1 + p0 + q0 + 4) >> 3
        q0_out = (p1 + 2 * p0 + 2 * q0 + 2 * q1 + q2 + 4) >> 3
        q1_out = (p0 + q0 + q1 + q2 + 2) >> 2
        q2_out = (p0 + q0 + q1 + 3 * q2 + 2 * q3 + 4) >> 3
        return [
            p3,
            clip_int(p2_out, 0, 255),
            clip_int(p1_out, 0, 255),
            clip_int(p0_out, 0, 255),
            clip_int(q0_out, 0, 255),
            clip_int(q1_out, 0, 255),
            clip_int(q2_out, 0, 255),
            q3,
        ]

    deblock_text = (V1 / "deblock.toml").read_text(encoding="utf-8")
    deblock_cases = re.findall(
        r'\[\[vectors\]\]\nname = "([^"]+)"\nkind = "(weak|strong)"\nqp = (\d+)\nsamples = (\[.*\])\nexpected = (\[.*\])',
        deblock_text,
    )
    if len(deblock_cases) != 7:
        errors.append("deblock.toml must contain 7 reviewed filter vectors")
    # A count alone only says vectors were not deleted. What matters is that
    # every activity rule the asset declares has a vector that would fail if the
    # rule were dropped: two of the three skip conditions had none, so removing
    # either from both decoders left this gate green and surfaced only as an
    # unexplained conformance hash drift.
    skip_rules = [
        ("skip_if_abs_p0_q0_ge_alpha", "activity_skip_alpha"),
        ("skip_if_abs_p1_p0_ge_beta", "activity_skip_beta_p"),
        ("skip_if_abs_q1_q0_ge_beta", "activity_skip_beta_q"),
    ]
    declared_names = [name for name, _, _, _, _ in deblock_cases]
    for rule, vector_name in skip_rules:
        if not re.search(r"^%s = true$" % re.escape(rule), deblock_text, re.MULTILINE):
            errors.append("deblock.toml no longer declares %s" % rule)
        if vector_name not in declared_names:
            errors.append("deblock.toml declares %s with no %s vector to defend it" % (rule, vector_name))
        else:
            index = declared_names.index(vector_name)
            _, _, _, samples_text, expected_text = deblock_cases[index]
            if ast.literal_eval(samples_text) != ast.literal_eval(expected_text):
                errors.append("deblock.toml %s must leave its samples untouched" % vector_name)
    for name, kind, qp_text, samples_text, expected_text in deblock_cases:
        samples = ast.literal_eval(samples_text)
        expected = ast.literal_eval(expected_text)
        qp = int(qp_text)
        got = deblock_weak(samples, qp) if kind == "weak" else deblock_strong(samples)
        if got != expected:
            errors.append("deblock vector %s expected %s got %s" % (name, expected, got))

    vectors = json.loads((V1 / "vectors.json").read_text(encoding="utf-8"))
    if vectors.get("format") != "key-frame-oracle-v1":
        errors.append("vectors.json has the wrong format id")
    probe_schema = json.loads((V1 / "probe.schema.json").read_text(encoding="utf-8"))
    if probe_schema.get("properties", {}).get("probe_version", {}).get("const") != 1:
        errors.append("probe.schema.json does not freeze probe_version 1")
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
