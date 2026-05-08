//! Quality metrics, defined precisely enough to be argued with.
//!
//! Floating point is allowed here and nowhere near the codec. These are
//! measurements *about* decoded video, not steps that produce it, so a
//! rounding difference between two machines changes a reported number rather
//! than a decoded pixel.
//!
//! Every definition below is pinned, because "PSNR" and "SSIM" each name a
//! family of slightly different computations and a comparison across two of
//! them is not a comparison at all. Luma only, cropped to the true display
//! dimensions, population moments, the exact window and constants named in the
//! benchmark conventions. Anything computed differently must call itself
//! something else.

use kf_frame::Frame;

/// A quality figure for a clip, and the per-frame figures behind it.
#[derive(Clone, Debug, PartialEq)]
pub struct Quality {
    /// Per-frame values, in display order.
    pub per_frame: Vec<f64>,
    /// The clip-global value. For PSNR this is computed from the summed
    /// squared error over the whole clip, not by averaging the per-frame
    /// numbers: averaging decibels weights a short easy frame the same as a
    /// long hard one and quietly flatters the result.
    pub global: f64,
    /// True when the clips are identical, in which case PSNR is unbounded and
    /// reporting a large finite number would be a lie with a decimal point.
    pub lossless: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MetricError {
    pub message: String,
}

impl core::fmt::Display for MetricError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for MetricError {}

fn error(message: impl Into<String>) -> MetricError {
    MetricError {
        message: message.into(),
    }
}

fn check_pair(reference: &[Frame], distorted: &[Frame]) -> Result<(), MetricError> {
    if reference.len() != distorted.len() {
        return Err(error(format!(
            "the clips have {} and {} frames",
            reference.len(),
            distorted.len()
        )));
    }
    if reference.is_empty() {
        return Err(error("the clips have no frames"));
    }
    for (index, (left, right)) in reference.iter().zip(distorted).enumerate() {
        if left.y.width() != right.y.width() || left.y.height() != right.y.height() {
            return Err(error(format!(
                "frame {index} is {}x{} against {}x{}",
                left.y.width(),
                left.y.height(),
                right.y.width(),
                right.y.height()
            )));
        }
    }
    Ok(())
}

/// The displayed luma samples of a frame, tightly packed.
///
/// A plane carries its own stride, and a padded plane's backing buffer holds
/// samples to the right of the picture that were never displayed. Reading the
/// buffer directly would fold that padding into every mean and every squared
/// error — a metric measured over pixels nobody sees. Everything below reads
/// through here instead.
fn luma(frame: &Frame) -> Vec<u8> {
    let width = frame.y.width() as usize;
    let height = frame.y.height() as usize;
    let stride = frame.y.stride() as usize;
    let data = frame.y.data();
    let mut packed = Vec::with_capacity(width * height);
    for row in 0..height {
        let start = row * stride;
        packed.extend_from_slice(&data[start..start + width]);
    }
    packed
}

/// PSNR over the luma plane, with `L = 255`.
pub fn psnr_y(reference: &[Frame], distorted: &[Frame]) -> Result<Quality, MetricError> {
    check_pair(reference, distorted)?;
    let mut clip_squared_error = 0.0_f64;
    let mut clip_samples = 0.0_f64;
    let mut per_frame = Vec::with_capacity(reference.len());

    for (left, right) in reference.iter().zip(distorted) {
        let (a, b) = (luma(left), luma(right));
        let mut squared_error = 0.0_f64;
        let samples = a.len();
        for (a, b) in a.iter().zip(&b) {
            let difference = f64::from(i32::from(*a) - i32::from(*b));
            squared_error += difference * difference;
        }
        clip_squared_error += squared_error;
        clip_samples += samples as f64;
        per_frame.push(decibels(squared_error, samples as f64));
    }

    Ok(Quality {
        lossless: clip_squared_error == 0.0,
        global: decibels(clip_squared_error, clip_samples),
        per_frame,
    })
}

/// Infinite when there is no error at all, which the caller reports as
/// `lossless` rather than as a number.
fn decibels(squared_error: f64, samples: f64) -> f64 {
    if squared_error == 0.0 {
        return f64::INFINITY;
    }
    let mean = squared_error / samples;
    10.0 * (255.0 * 255.0 / mean).log10()
}

/// The separable Gaussian window, eleven taps at `sigma = 1.5`.
///
/// Held as literal normalized weights rather than recomputed, so the window is
/// a fact of the metric rather than a consequence of whichever `exp` the
/// platform ships. They sum to one, which a test asserts: a window that does
/// not would shift every mean it computes and change SSIM everywhere without
/// changing anything visibly.
const GAUSSIAN_11: [f64; 11] = [
    0.00102838008447911,
    0.007598758135239185,
    0.03600077212843083,
    0.10936068950970002,
    0.2130055377112537,
    0.26601172486179436,
    0.2130055377112537,
    0.10936068950970002,
    0.03600077212843083,
    0.007598758135239185,
    0.00102838008447911,
];

const SSIM_K1: f64 = 0.01;
const SSIM_K2: f64 = 0.03;
const SSIM_L: f64 = 255.0;

/// SSIM over the luma plane in the Wang 11×11 Gaussian form.
///
/// Population moments, reflected edges, and a value at every displayed sample —
/// not only at window centres that fit inside the frame. Cropping the border
/// instead would quietly exclude exactly the region a codec finds hardest.
pub fn ssim_y(reference: &[Frame], distorted: &[Frame]) -> Result<Quality, MetricError> {
    check_pair(reference, distorted)?;
    let c1 = (SSIM_K1 * SSIM_L).powi(2);
    let c2 = (SSIM_K2 * SSIM_L).powi(2);

    let mut per_frame = Vec::with_capacity(reference.len());
    let mut total = 0.0_f64;
    let mut counted = 0.0_f64;

    for (left, right) in reference.iter().zip(distorted) {
        let width = left.y.width() as usize;
        let height = left.y.height() as usize;
        let a: Vec<f64> = luma(left).iter().map(|s| f64::from(*s)).collect();
        let b: Vec<f64> = luma(right).iter().map(|s| f64::from(*s)).collect();
        let aa: Vec<f64> = a.iter().map(|v| v * v).collect();
        let bb: Vec<f64> = b.iter().map(|v| v * v).collect();
        let ab: Vec<f64> = a.iter().zip(&b).map(|(x, y)| x * y).collect();

        let mu_a = blur(&a, width, height);
        let mu_b = blur(&b, width, height);
        let mu_aa = blur(&aa, width, height);
        let mu_bb = blur(&bb, width, height);
        let mu_ab = blur(&ab, width, height);

        let mut sum = 0.0_f64;
        for index in 0..a.len() {
            let ma = mu_a[index];
            let mb = mu_b[index];
            let va = mu_aa[index] - ma * ma;
            let vb = mu_bb[index] - mb * mb;
            let cov = mu_ab[index] - ma * mb;
            let numerator = (2.0 * ma * mb + c1) * (2.0 * cov + c2);
            let denominator = (ma * ma + mb * mb + c1) * (va + vb + c2);
            sum += numerator / denominator;
        }
        let samples = a.len() as f64;
        per_frame.push(sum / samples);
        total += sum;
        counted += samples;
    }

    Ok(Quality {
        lossless: per_frame.iter().all(|value| (value - 1.0).abs() < 1e-12),
        global: total / counted,
        per_frame,
    })
}

/// Separable Gaussian blur with reflected edges.
fn blur(plane: &[f64], width: usize, height: usize) -> Vec<f64> {
    let mut horizontal = vec![0.0_f64; plane.len()];
    for row in 0..height {
        for column in 0..width {
            let mut accumulated = 0.0;
            for (tap, weight) in GAUSSIAN_11.iter().enumerate() {
                let offset = tap as isize - 5;
                let sample = reflect(column as isize + offset, width);
                accumulated += weight * plane[row * width + sample];
            }
            horizontal[row * width + column] = accumulated;
        }
    }
    let mut vertical = vec![0.0_f64; plane.len()];
    for row in 0..height {
        for column in 0..width {
            let mut accumulated = 0.0;
            for (tap, weight) in GAUSSIAN_11.iter().enumerate() {
                let offset = tap as isize - 5;
                let sample = reflect(row as isize + offset, height);
                accumulated += weight * horizontal[sample * width + column];
            }
            vertical[row * width + column] = accumulated;
        }
    }
    vertical
}

/// Mirrors a coordinate back inside the plane, repeatedly if it has to.
fn reflect(position: isize, extent: usize) -> usize {
    if extent == 0 {
        return 0;
    }
    let limit = extent as isize;
    let mut value = position;
    loop {
        if value < 0 {
            value = -value;
        } else if value >= limit {
            value = 2 * limit - value - 2;
        } else {
            return value as usize;
        }
        // A window wider than the plane can bounce more than once, which is
        // why this loops rather than folding a single time.
        if limit == 1 {
            return 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Quality, blur, psnr_y, reflect, ssim_y};
    use kf_frame::{Frame, Plane};

    fn flat(value: u8) -> Frame {
        Frame::filled_420(64, 64, value).expect("the frame is in range")
    }

    #[test]
    fn identical_clips_are_lossless_rather_than_very_good() {
        let reference = vec![flat(128)];
        let result = psnr_y(&reference, &reference).unwrap();
        assert!(result.lossless);
        assert!(result.global.is_infinite());
        assert!(result.per_frame[0].is_infinite());

        let structural = ssim_y(&reference, &reference).unwrap();
        assert!(structural.lossless);
        assert!((structural.global - 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_known_error_gives_the_textbook_decibels() {
        // Every sample off by one: mean squared error is exactly 1, so PSNR is
        // 10 log10(255^2) = 48.1308... dB.
        let reference = vec![flat(100)];
        let distorted = vec![flat(101)];
        let result = psnr_y(&reference, &distorted).unwrap();
        assert!(!result.lossless);
        assert!(
            (result.global - 48.130_803_608_679_1).abs() < 1e-9,
            "{}",
            result.global
        );
    }

    #[test]
    fn the_global_figure_comes_from_summed_error_not_averaged_decibels() {
        // One perfect frame and one poor one. Averaging decibels would be
        // pulled to infinity by the perfect frame; summing squared error is
        // finite and honest.
        let reference = vec![flat(100), flat(100)];
        let distorted = vec![flat(100), flat(140)];
        let result = psnr_y(&reference, &distorted).unwrap();
        assert!(result.per_frame[0].is_infinite());
        assert!(result.global.is_finite());
        assert!(result.global > 0.0);
    }

    #[test]
    fn structural_similarity_falls_as_the_picture_degrades() {
        let mut reference = flat(0);
        for y in 0..64 {
            for x in 0..64 {
                reference
                    .y
                    .set(x, y, u8::try_from((x * 4 + y) % 256).unwrap())
                    .unwrap();
            }
        }
        let mut mild = reference.clone();
        let mut severe = reference.clone();
        for y in 0..64 {
            for x in 0..64 {
                let base = i32::from(reference.y.get(x, y).unwrap());
                mild.y
                    .set(x, y, u8::try_from((base + 2).clamp(0, 255)).unwrap())
                    .unwrap();
                severe
                    .y
                    .set(x, y, u8::try_from((base / 3).clamp(0, 255)).unwrap())
                    .unwrap();
            }
        }
        let clean = ssim_y(&[reference.clone()], &[reference]).unwrap().global;
        let a = ssim_y(&[mild.clone()], &[mild.clone()]).unwrap().global;
        assert!((clean - 1.0).abs() < 1e-9);
        assert!((a - 1.0).abs() < 1e-9);

        let mild_score = ssim_y(&[severe.clone()], &[mild]).unwrap().global;
        let severe_score = ssim_y(&[severe.clone()], &[flat(0)]).unwrap().global;
        assert!(mild_score < 1.0);
        assert!(severe_score < mild_score, "{severe_score} !< {mild_score}");
    }

    #[test]
    fn the_window_is_normalized_so_a_flat_plane_blurs_to_itself() {
        let total: f64 = super::GAUSSIAN_11.iter().sum();
        assert!((total - 1.0).abs() < 1e-12, "the window sums to {total}");
        let plane = vec![37.0_f64; 16 * 16];
        for value in blur(&plane, 16, 16) {
            assert!((value - 37.0).abs() < 1e-9, "{value}");
        }
    }

    #[test]
    fn reflection_folds_repeatedly_when_the_window_is_wider_than_the_plane() {
        assert_eq!(reflect(-1, 8), 1);
        assert_eq!(reflect(8, 8), 6);
        assert_eq!(reflect(-5, 3), 1);
        assert_eq!(reflect(7, 3), 1);
        assert_eq!(reflect(-5, 1), 0);
    }

    #[test]
    fn stride_padding_is_outside_the_picture_and_outside_the_measurement() {
        // A decoded frame can arrive padded to the superblock grid. The samples
        // to the right of the picture were never displayed, so a metric that
        // read the backing buffer would be measuring them — and would report a
        // different number for two frames that look identical.
        let clean = flat(64);
        let mut padded = flat(64);
        let mut plane = Plane::with_stride(64, 64, 96, 64).unwrap();
        for y in 0..64 {
            for x in 64..96 {
                // Garbage in the padding, deliberately far from the picture
                // value so that leaking it would be unmissable.
                plane.data_mut()[y * 96 + x] = 240;
            }
        }
        padded.y = plane;

        let result = psnr_y(std::slice::from_ref(&clean), std::slice::from_ref(&padded)).unwrap();
        assert!(result.lossless, "padding leaked into the squared error");
        let structural = ssim_y(&[clean], &[padded]).unwrap();
        assert!(
            (structural.global - 1.0).abs() < 1e-12,
            "padding leaked into the moments: {}",
            structural.global
        );
    }

    #[test]
    fn mismatched_clips_are_refused_rather_than_measured() {
        assert!(psnr_y(&[flat(1)], &[]).is_err());
        assert!(psnr_y(&[], &[]).is_err());
        let wide = Frame::filled_420(128, 64, 0).unwrap();
        assert!(psnr_y(&[flat(1)], std::slice::from_ref(&wide)).is_err());
        assert!(ssim_y(&[flat(1)], &[wide]).is_err());
    }

    #[test]
    fn quality_is_comparable_for_equality() {
        let one = Quality {
            per_frame: vec![1.0],
            global: 1.0,
            lossless: false,
        };
        assert_eq!(one.clone(), one);
    }
}
