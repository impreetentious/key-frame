#!/usr/bin/env python3
"""Independent standard-library oracle for the Key Frame quality metrics.

The codec has an oracle in `spec/` because a bitstream that only its own
encoder agrees with is not a format. The measurements have one for the same
reason: PSNR, SSIM and BD-rate are each a family of slightly different
computations, and a published number is only meaningful if it names which
member of the family it is. This file is that name, written out.

It is deliberately a *second* implementation rather than a transcription of the
Rust one:

  * SSIM here convolves with the full 11x11 outer-product window directly. The
    Rust applies the same window as two separable passes. Separability is an
    algebraic identity, so the two must agree to rounding — and if the Rust
    ever gets its row and column passes crossed, or drops a tap at an edge, the
    identity breaks and this notices.
  * The Gaussian weights here are derived from `exp` and normalized. The Rust
    holds them as literals so that its numbers do not depend on the platform's
    libm. The check below asserts that the literals *are* the derivation, which
    is the only thing that makes freezing them safe.
  * BD-rate here integrates each cubic piece with two-point Gauss-Legendre,
    which is exact for cubics. The Rust evaluates the Hermite antiderivative in
    closed form. Same integral, no shared arithmetic.

Run with `--check` to compare the committed vectors against a fresh derivation.
Run with no arguments to print them; `--write` commits them.
"""

import argparse
import json
import math
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parent
VECTOR_PATH = ROOT / "metric-vectors.json"

SSIM_SIGMA = 1.5
SSIM_TAPS = 11
SSIM_K1 = 0.01
SSIM_K2 = 0.03
SSIM_L = 255.0

# Both implementations run IEEE-754 doubles over the same inputs but in
# different orders, so they agree to well under a rounding error's worth of any
# metric. A decibel is not meaningful below its sixth decimal; this is far
# tighter than that and still loose enough that operand order cannot fail it.
TOLERANCE = 1e-9


def gaussian_window():
    """The eleven-tap window at sigma = 1.5, normalized to sum to one."""
    centre = (SSIM_TAPS - 1) // 2
    raw = [
        math.exp(-((tap - centre) ** 2) / (2.0 * SSIM_SIGMA * SSIM_SIGMA))
        for tap in range(SSIM_TAPS)
    ]
    total = sum(raw)
    return [value / total for value in raw]


def reflect(position, extent):
    """Mirrors a coordinate back inside `[0, extent)`, folding as often as needed.

    A window wider than the plane bounces more than once, which is exactly the
    case a single fold gets wrong. Small planes in the vector set exist to make
    a single-fold implementation fail here.
    """
    if extent == 1:
        return 0
    while position < 0 or position >= extent:
        if position < 0:
            position = -position
        else:
            position = 2 * extent - position - 2
    return position


def convolve(plane, width, height, window):
    """Direct 2D convolution with the outer product of `window` with itself."""
    centre = (len(window) - 1) // 2
    output = [0.0] * (width * height)
    for row in range(height):
        for column in range(width):
            total = 0.0
            for tap_row, weight_row in enumerate(window):
                source_row = reflect(row + tap_row - centre, height)
                for tap_column, weight_column in enumerate(window):
                    source_column = reflect(column + tap_column - centre, width)
                    total += (
                        weight_row
                        * weight_column
                        * plane[source_row * width + source_column]
                    )
            output[row * width + column] = total
    return output


def ssim_frame(reference, distorted, width, height):
    """Mean SSIM over every displayed sample of one frame."""
    window = gaussian_window()
    c1 = (SSIM_K1 * SSIM_L) ** 2
    c2 = (SSIM_K2 * SSIM_L) ** 2

    a = [float(sample) for sample in reference]
    b = [float(sample) for sample in distorted]
    mu_a = convolve(a, width, height, window)
    mu_b = convolve(b, width, height, window)
    mu_aa = convolve([value * value for value in a], width, height, window)
    mu_bb = convolve([value * value for value in b], width, height, window)
    mu_ab = convolve([x * y for x, y in zip(a, b)], width, height, window)

    total = 0.0
    for index in range(width * height):
        ma, mb = mu_a[index], mu_b[index]
        variance_a = mu_aa[index] - ma * ma
        variance_b = mu_bb[index] - mb * mb
        covariance = mu_ab[index] - ma * mb
        numerator = (2.0 * ma * mb + c1) * (2.0 * covariance + c2)
        denominator = (ma * ma + mb * mb + c1) * (variance_a + variance_b + c2)
        total += numerator / denominator
    return total / (width * height)


def psnr_clip(frames):
    """Per-frame and clip-global PSNR-Y from summed squared error.

    `frames` is a list of `(reference, distorted, width, height)`. The global
    figure sums squared error over the whole clip rather than averaging
    decibels, so one perfect frame cannot pull the clip figure to infinity.
    """
    per_frame = []
    clip_error = 0.0
    clip_samples = 0
    for reference, distorted, width, height in frames:
        error = 0.0
        for a, b in zip(reference, distorted):
            difference = float(a - b)
            error += difference * difference
        clip_error += error
        clip_samples += width * height
        per_frame.append(decibels(error, width * height))
    return per_frame, decibels(clip_error, clip_samples)


def decibels(squared_error, samples):
    if squared_error == 0.0:
        return None
    return 10.0 * math.log10(255.0 * 255.0 / (squared_error / samples))


def pchip_slopes(xs, ys):
    """Fritsch-Carlson slopes: the rule that makes the interpolant monotone."""
    count = len(xs)
    widths = [xs[i + 1] - xs[i] for i in range(count - 1)]
    secants = [(ys[i + 1] - ys[i]) / widths[i] for i in range(count - 1)]
    derivatives = [0.0] * count
    for index in range(1, count - 1):
        before, after = secants[index - 1], secants[index]
        if before * after <= 0.0:
            derivatives[index] = 0.0
        else:
            w1 = 2.0 * widths[index] + widths[index - 1]
            w2 = widths[index] + 2.0 * widths[index - 1]
            derivatives[index] = (w1 + w2) / (w1 / before + w2 / after)
    derivatives[0] = endpoint(secants[0], secants[1], widths[0], widths[1])
    derivatives[-1] = endpoint(secants[-1], secants[-2], widths[-1], widths[-2])
    return derivatives


def endpoint(near, far, near_width, far_width):
    estimate = ((2.0 * near_width + far_width) * near - near_width * far) / (
        near_width + far_width
    )
    if estimate * near <= 0.0:
        return 0.0
    if near * far <= 0.0 and abs(estimate) > abs(3.0 * near):
        return 3.0 * near
    return estimate


def hermite(t, y0, y1, d0, d1, width):
    """The cubic Hermite value at `t` in `[0, 1]` across an interval of `width`."""
    t2 = t * t
    t3 = t2 * t
    h00 = 2.0 * t3 - 3.0 * t2 + 1.0
    h10 = t3 - 2.0 * t2 + t
    h01 = -2.0 * t3 + 3.0 * t2
    h11 = t3 - t2
    return y0 * h00 + width * d0 * h10 + y1 * h01 + width * d1 * h11


# Two-point Gauss-Legendre on [-1, 1] integrates any cubic exactly, which is
# what lets this sample the interpolant and still be an exact answer rather than
# a fine-enough approximation of one.
GAUSS_NODES = (-1.0 / math.sqrt(3.0), 1.0 / math.sqrt(3.0))


def integrate(xs, ys, low, high):
    derivatives = pchip_slopes(xs, ys)
    total = 0.0
    for index in range(len(xs) - 1):
        x0, x1 = xs[index], xs[index + 1]
        start, end = max(x0, low), min(x1, high)
        if end <= start:
            continue
        width = x1 - x0
        half = (end - start) / 2.0
        middle = (end + start) / 2.0
        for node in GAUSS_NODES:
            x = middle + half * node
            total += half * hermite(
                (x - x0) / width,
                ys[index],
                ys[index + 1],
                derivatives[index],
                derivatives[index + 1],
                width,
            )
    return total


def bd_rate(baseline, candidate):
    """Percent bitrate difference of `candidate` against `baseline`.

    Each argument is a list of `(rate, quality)`. Points are sorted by quality
    and the log-rate curves are compared only where they overlap.
    """
    left = prepare(baseline)
    right = prepare(candidate)
    low = max(left[0][0], right[0][0])
    high = min(left[-1][0], right[-1][0])
    if high <= low:
        raise ValueError("the curves share no quality interval")
    left_x = [point[0] for point in left]
    left_y = [point[1] for point in left]
    right_x = [point[0] for point in right]
    right_y = [point[1] for point in right]
    difference = (
        integrate(right_x, right_y, low, high) - integrate(left_x, left_y, low, high)
    ) / (high - low)
    return 100.0 * (math.exp(difference) - 1.0)


def prepare(points):
    prepared = sorted((quality, math.log(rate)) for rate, quality in points)
    deduplicated = []
    for quality, log_rate in prepared:
        if deduplicated and deduplicated[-1][0] == quality:
            continue
        deduplicated.append((quality, log_rate))
    if len(deduplicated) < 4:
        raise ValueError("a curve needs four distinct points")
    return deduplicated


def lcg(seed, count, low=0, high=255):
    """A tiny reproducible sample generator.

    Numerical Recipes' constants, taken from the high bits because the low bits
    of a linear congruential generator are famously not random. The planes only
    have to be arbitrary and reproducible, not statistically good.
    """
    state = seed & 0xFFFFFFFF
    span = high - low + 1
    values = []
    for _ in range(count):
        state = (1664525 * state + 1013904223) & 0xFFFFFFFF
        values.append(low + ((state >> 16) % span))
    return values


def blur_plane(samples, width, height, radius):
    """A crude box blur, used only to author a plausibly distorted plane."""
    output = []
    for row in range(height):
        for column in range(width):
            total = 0
            count = 0
            for dy in range(-radius, radius + 1):
                for dx in range(-radius, radius + 1):
                    y = reflect(row + dy, height)
                    x = reflect(column + dx, width)
                    total += samples[y * width + x]
                    count += 1
            output.append(total // count)
    return output


def ramp_plane(width, height, slope):
    return [(x * slope + y) % 256 for y in range(height) for x in range(width)]


def build_planes():
    """The plane pairs the vectors measure, each chosen for one failure mode."""
    cases = []

    # Wider than the window in both directions: the ordinary interior case.
    reference = lcg(0x5EED, 16 * 16)
    cases.append(
        (
            "noise_16x16_blurred",
            16,
            16,
            reference,
            blur_plane(reference, 16, 16, 1),
            "A 16x16 noise field against a box-blurred copy of itself. Wider "
            "than the eleven-tap window in both directions, so most windows sit "
            "entirely inside the plane and this is the case an implementation "
            "gets right by accident.",
        )
    )

    # Narrower than the window: every single window overhangs an edge, so the
    # reflection rule decides the whole answer.
    reference = ramp_plane(9, 9, 7)
    distorted = [min(255, value + 12) for value in reference]
    cases.append(
        (
            "ramp_9x9_offset",
            9,
            9,
            reference,
            distorted,
            "A 9x9 ramp against the same ramp brightened by twelve. Narrower "
            "than the window, so every window overhangs an edge and the "
            "reflection rule is doing all of the work.",
        )
    )

    # Three samples across: reflection has to fold repeatedly. An
    # implementation that mirrors once and clamps afterwards diverges here.
    reference = [10, 200, 30, 240, 60, 90, 15, 180, 45]
    distorted = [12, 190, 35, 250, 55, 95, 20, 170, 50]
    cases.append(
        (
            "tiny_3x3_repeated_reflection",
            3,
            3,
            reference,
            distorted,
            "A 3x3 pair. The eleven-tap window is nearly four times the plane, "
            "so a coordinate has to fold back and forth several times before it "
            "lands inside. Mirroring once and clamping gives a different "
            "number here and nowhere else.",
        )
    )

    # One row: the vertical pass folds to a single line while the horizontal
    # pass behaves normally, so a crossed pair of passes shows up immediately.
    reference = [0, 32, 64, 96, 128, 160, 192, 224, 255, 128, 64, 32]
    distorted = [4, 30, 70, 90, 130, 155, 200, 220, 250, 130, 60, 36]
    cases.append(
        (
            "single_row_12x1",
            12,
            1,
            reference,
            distorted,
            "A single row. The vertical direction collapses to one line while "
            "the horizontal one does not, so an implementation that has its two "
            "separable passes crossed cannot agree with a direct convolution "
            "here.",
        )
    )

    # A flat pair: SSIM is exactly one and PSNR is unbounded, which is the
    # boundary where a "very large number" stand-in would be wrong.
    flat_reference = [128] * (12 * 12)
    cases.append(
        (
            "flat_identical_12x12",
            12,
            12,
            flat_reference,
            list(flat_reference),
            "An identical flat pair. SSIM is exactly one and PSNR is unbounded; "
            "a report that prints a large finite decibel figure here is lying "
            "with a decimal point.",
        )
    )

    return cases


def build_curves():
    """BD-rate cases, each aimed at a way the interpolant can be wrong."""
    return [
        (
            "uniform_halved_rate",
            [(100.0, 30.0), (200.0, 33.0), (400.0, 36.0), (800.0, 39.0), (1600.0, 42.0)],
            [(50.0, 30.0), (100.0, 33.0), (200.0, 36.0), (400.0, 39.0), (800.0, 42.0)],
            "Every rate halved at the same quality. The answer is exactly -50% "
            "for any correct integrator, so this pins the sign convention and "
            "the exp/log round trip without depending on the interpolant at "
            "all.",
        ),
        (
            "uneven_quality_spacing",
            [(90.0, 28.5), (260.0, 33.0), (520.0, 34.2), (900.0, 38.9), (2100.0, 41.0)],
            [(80.0, 29.1), (210.0, 32.4), (505.0, 35.6), (880.0, 38.1), (1750.0, 41.8)],
            "Irregular quality spacing on both curves and no shared quality "
            "point. The interval widths enter the slope rule, so a "
            "uniform-spacing shortcut gives a different answer here.",
        ),
        (
            "plateau_then_knee",
            [(120.0, 31.0), (240.0, 31.0), (500.0, 31.2), (1000.0, 39.5), (1900.0, 40.0)],
            [(110.0, 30.8), (230.0, 33.5), (460.0, 35.0), (980.0, 36.2), (2000.0, 40.4)],
            "A near-plateau followed by a sharp knee. An unconstrained cubic "
            "spline overshoots across the knee and dips below the plateau "
            "before it; the monotone rule flattens through both, and the two "
            "give visibly different areas.",
        ),
        (
            "partial_overlap",
            [(150.0, 30.0), (300.0, 33.0), (600.0, 36.0), (1200.0, 39.0), (2400.0, 42.0)],
            [(140.0, 34.0), (280.0, 37.0), (560.0, 40.0), (1100.0, 43.0), (2200.0, 46.0)],
            "The curves overlap over only part of their quality range. "
            "Integrating outside the overlap would be an extrapolation, so the "
            "bounds have to be the intersection and not either curve's own "
            "span.",
        ),
    ]


def build_vectors():
    window = gaussian_window()

    planes = []
    for name, width, height, reference, distorted, note in build_planes():
        per_frame, _ = psnr_clip([(reference, distorted, width, height)])
        planes.append(
            {
                "name": name,
                "note": note,
                "width": width,
                "height": height,
                "reference": reference,
                "distorted": distorted,
                "psnr_y": per_frame[0],
                "ssim_y": ssim_frame(reference, distorted, width, height),
            }
        )

    # One multi-frame clip, so the global-versus-per-frame rule is pinned and
    # not only asserted in prose.
    clip_frames = []
    for index in range(3):
        reference = lcg(0xC0FFEE + index * 17, 10 * 8)
        distorted = [
            min(255, max(0, value + (7 if (position % 3) == 0 else -5)))
            for position, value in enumerate(reference)
        ]
        if index == 1:
            distorted = list(reference)
        clip_frames.append((reference, distorted, 10, 8))
    per_frame, global_psnr = psnr_clip(clip_frames)

    curves = []
    for name, baseline, candidate, note in build_curves():
        curves.append(
            {
                "name": name,
                "note": note,
                "baseline": [{"rate": r, "quality": q} for r, q in baseline],
                "candidate": [{"rate": r, "quality": q} for r, q in candidate],
                "bd_rate_percent": bd_rate(baseline, candidate),
            }
        )

    # The interpolant itself, pinned separately from any BD-rate it produces.
    # A slope table and an area can disagree with the Rust while the percentage
    # they roll up into still happens to land close enough to hide it.
    pchip = []
    for name, xs, ys, low, high, note in [
        (
            "monotone_rise",
            [0.0, 1.0, 2.0, 4.0],
            [0.0, 2.0, 4.0, 8.0],
            0.0,
            4.0,
            "A straight line sampled unevenly. Every slope is two and the area "
            "is exactly sixteen, so any deviation is arithmetic and not "
            "interpretation.",
        ),
        (
            "plateau",
            [0.0, 1.0, 2.0, 3.0],
            [1.0, 1.0, 1.0, 5.0],
            0.0,
            3.0,
            "A flat run into a rise. The monotone rule forces the first three "
            "slopes to zero; an unconstrained spline makes them negative and "
            "dips below the plateau.",
        ),
        (
            "knee",
            [0.0, 1.5, 2.0, 6.0],
            [0.0, 4.0, 4.2, 4.4],
            0.5,
            5.0,
            "A sharp rise into a near-plateau, integrated over a sub-interval "
            "that starts and ends inside pieces rather than at knots.",
        ),
    ]:
        pchip.append(
            {
                "name": name,
                "note": note,
                "x": xs,
                "y": ys,
                "slopes": pchip_slopes(xs, ys),
                "low": low,
                "high": high,
                "integral": integrate(xs, ys, low, high),
            }
        )

    return {
        "format": "key-frame-metric-vectors-v1",
        "note": (
            "Authored by bench/metric_oracle.py, which implements SSIM by "
            "direct 2D convolution and the BD-rate integral by Gauss-Legendre "
            "quadrature. The Rust in crates/kf-tools uses separable passes and "
            "a closed-form antiderivative. Agreement between the two is the "
            "point; neither is derived from the other."
        ),
        "tolerance": TOLERANCE,
        "ssim": {
            "sigma": SSIM_SIGMA,
            "taps": SSIM_TAPS,
            "k1": SSIM_K1,
            "k2": SSIM_K2,
            "dynamic_range": SSIM_L,
            "window": window,
        },
        "planes": planes,
        "clip": {
            "note": (
                "Three frames, the middle one identical to its reference. The "
                "clip figure comes from summed squared error, so it stays "
                "finite even though one frame's own figure is not."
            ),
            "width": 10,
            "height": 8,
            "frames": [
                {"reference": reference, "distorted": distorted}
                for reference, distorted, _, _ in clip_frames
            ],
            "psnr_per_frame": per_frame,
            "psnr_global": global_psnr,
        },
        "pchip": pchip,
        "curves": curves,
    }


def close_enough(left, right):
    if left is None or right is None:
        return left is None and right is None
    if isinstance(left, list) or isinstance(right, list):
        if len(left) != len(right):
            return False
        return all(close_enough(a, b) for a, b in zip(left, right))
    if isinstance(left, float) or isinstance(right, float):
        return abs(float(left) - float(right)) <= TOLERANCE * max(
            1.0, abs(float(left)), abs(float(right))
        )
    return left == right


def compare(committed, fresh, path=""):
    """Reports every difference rather than the first, so one run fixes all of them."""
    differences = []
    if isinstance(fresh, dict):
        if not isinstance(committed, dict):
            return ["%s: committed value is not an object" % (path or "<root>")]
        for key in sorted(set(fresh) | set(committed)):
            if key not in committed:
                differences.append("%s/%s: missing from the committed file" % (path, key))
            elif key not in fresh:
                differences.append("%s/%s: not produced by the oracle" % (path, key))
            else:
                differences.extend(
                    compare(committed[key], fresh[key], "%s/%s" % (path, key))
                )
        return differences
    if isinstance(fresh, list) and fresh and isinstance(fresh[0], dict):
        if not isinstance(committed, list) or len(committed) != len(fresh):
            return ["%s: list length differs" % path]
        for index, (a, b) in enumerate(zip(committed, fresh)):
            differences.extend(compare(a, b, "%s[%d]" % (path, index)))
        return differences
    if not close_enough(committed, fresh):
        differences.append("%s: %r against %r" % (path, committed, fresh))
    return differences


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--check",
        action="store_true",
        help="compare the committed vectors against a fresh derivation",
    )
    parser.add_argument(
        "--write", action="store_true", help="write the derivation to the vector file"
    )
    arguments = parser.parse_args()

    fresh = build_vectors()

    if arguments.write:
        VECTOR_PATH.write_text(json.dumps(fresh, indent=2) + "\n", encoding="utf-8")
        print("metric oracle: wrote %s" % VECTOR_PATH.name)
        return 0

    if arguments.check:
        if not VECTOR_PATH.exists():
            print("metric oracle: %s does not exist" % VECTOR_PATH, file=sys.stderr)
            return 1
        committed = json.loads(VECTOR_PATH.read_text(encoding="utf-8"))
        differences = compare(committed, fresh)
        if differences:
            print("metric oracle: the committed vectors do not match:", file=sys.stderr)
            for difference in differences:
                print("  %s" % difference, file=sys.stderr)
            return 1
        planes = len(committed["planes"])
        curves = len(committed["curves"])
        pchip = len(committed["pchip"])
        print(
            "metric oracle: OK — window derivation, %d SSIM/PSNR plane pairs, "
            "one multi-frame clip, %d interpolant cases, %d rate curves"
            % (planes, pchip, curves)
        )
        return 0

    print(json.dumps(fresh, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())
