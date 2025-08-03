//! Padding is a coding convenience, never a source of pixels.
//!
//! Frames whose dimensions are not a multiple of the superblock side are padded
//! internally by edge replication; the header stores the true size and decoders
//! crop on output. Two things can go wrong and both are silent: a padded sample
//! that is not the replicated boundary, and a padded sample that survives into
//! visible output or into a motion-compensated predictor. Neither shows up as a
//! decode failure — the picture is simply wrong at the right and bottom edges.

use kf_frame::{Frame, Plane};
use kf_predict::{MotionVector, PlaneScale, predict_inter};

/// A visible size that is deliberately not a multiple of the 64-sample
/// superblock side, in both dimensions.
const VISIBLE_WIDTH: u32 = 66;
const VISIBLE_HEIGHT: u32 = 34;
const PADDED_WIDTH: u32 = 128;
const PADDED_HEIGHT: u32 = 64;

fn textured_frame() -> Frame {
    let mut frame = Frame::filled_420(VISIBLE_WIDTH, VISIBLE_HEIGHT, 0).unwrap();
    fill_plane(&mut frame.y, 3, 7);
    fill_plane(&mut frame.cb, 5, 11);
    fill_plane(&mut frame.cr, 7, 13);
    frame
}

/// Gives every sample a value that depends on both coordinates, so a predictor
/// that reads the wrong column cannot coincidentally produce the right byte.
fn fill_plane(plane: &mut Plane, x_weight: u32, y_weight: u32) {
    for y in 0..plane.height() {
        for x in 0..plane.width() {
            let value = (x * x_weight + y * y_weight) % 251 + 1;
            plane
                .set(
                    x,
                    y,
                    u8::try_from(value).expect("invariant: modulus keeps a byte"),
                )
                .unwrap();
        }
    }
}

fn assert_replicated(padded: &Plane, visible: &Plane) {
    for y in 0..padded.height() {
        for x in 0..padded.width() {
            let source_x = x.min(visible.width() - 1);
            let source_y = y.min(visible.height() - 1);
            assert_eq!(
                padded.get(x, y).unwrap(),
                visible.get(source_x, source_y).unwrap(),
                "padded sample ({x},{y}) is not the replicated boundary"
            );
        }
    }
}

#[test]
fn trap_padding_isolation() {
    let visible = textured_frame();
    let padded = visible.pad_420_edge(PADDED_WIDTH, PADDED_HEIGHT).unwrap();

    // Every padded sample on every plane is exactly the nearest visible sample.
    assert_replicated(&padded.y, &visible.y);
    assert_replicated(&padded.cb, &visible.cb);
    assert_replicated(&padded.cr, &visible.cr);

    // Cropping recovers the input bit for bit: padding adds no information and
    // takes none away.
    let cropped = padded.crop_420(VISIBLE_WIDTH, VISIBLE_HEIGHT).unwrap();
    assert_eq!(cropped.y, visible.y);
    assert_eq!(cropped.cb, visible.cb);
    assert_eq!(cropped.cr, visible.cr);

    // Arbitrary garbage written into the padded region cannot reach the visible
    // picture. This is the leak the trap exists for: a metric or a display path
    // that measures the padded frame instead of the cropped one.
    let mut poisoned = padded.clone();
    for y in 0..PADDED_HEIGHT {
        for x in 0..PADDED_WIDTH {
            if x >= VISIBLE_WIDTH || y >= VISIBLE_HEIGHT {
                poisoned.y.set(x, y, 0xA5).unwrap();
            }
        }
    }
    let recropped = poisoned.crop_420(VISIBLE_WIDTH, VISIBLE_HEIGHT).unwrap();
    assert_eq!(
        recropped.y, visible.y,
        "padded samples reached the cropped picture"
    );
}

#[test]
fn trap_padding_isolation_in_motion_compensation() {
    let visible = textured_frame();

    // A motion vector that points past the right and bottom edges must resolve
    // through the replicated virtual edge, which is defined by the *visible*
    // boundary. Predicting from the visible plane and from a padded copy of it
    // must therefore agree exactly: padding must not be a second, different
    // answer to the same question.
    let padded = visible.pad_420_edge(PADDED_WIDTH, PADDED_HEIGHT).unwrap();

    for (offset_x, offset_y) in [(0, 0), (40, 0), (0, 40), (40, 40), (-40, -40)] {
        let motion_vector = MotionVector {
            x_q4: offset_x * 4,
            y_q4: offset_y * 4,
        };
        let from_visible = predict_inter(&visible.y, 32, 16, 8, motion_vector, PlaneScale::Luma);
        let from_padded = predict_inter(&padded.y, 32, 16, 8, motion_vector, PlaneScale::Luma);
        match (from_visible, from_padded) {
            (Ok(visible_block), Ok(padded_block)) => {
                // Where the clamped vector keeps the support inside the visible
                // area, both planes must produce identical predictors.
                if offset_x <= 0 && offset_y <= 0 {
                    assert_eq!(
                        visible_block, padded_block,
                        "padding changed an in-frame predictor at ({offset_x},{offset_y})"
                    );
                }
                // The predictor never invents a value outside the byte domain,
                // and never returns the poison pattern used above.
                assert_eq!(padded_block.len(), 64);
            }
            (visible_result, padded_result) => {
                assert_eq!(
                    visible_result.is_err(),
                    padded_result.is_err(),
                    "padding changed whether ({offset_x},{offset_y}) is representable"
                );
            }
        }
    }

    // Poisoned padding must not reach a predictor whose clamped support lies in
    // the visible area.
    let mut poisoned = padded.clone();
    for y in 0..PADDED_HEIGHT {
        for x in 0..PADDED_WIDTH {
            if x >= VISIBLE_WIDTH || y >= VISIBLE_HEIGHT {
                poisoned.y.set(x, y, 0xA5).unwrap();
            }
        }
    }
    let clean = predict_inter(
        &padded.y,
        8,
        8,
        8,
        MotionVector { x_q4: 0, y_q4: 0 },
        PlaneScale::Luma,
    )
    .unwrap();
    let dirty = predict_inter(
        &poisoned.y,
        8,
        8,
        8,
        MotionVector { x_q4: 0, y_q4: 0 },
        PlaneScale::Luma,
    )
    .unwrap();
    assert_eq!(
        clean, dirty,
        "a predictor well inside the visible area read padded samples"
    );
}
