use kf_frame::Plane;
use kf_predict::{IntraMode, PredictError, predict_intra};

fn bordered_plane() -> Plane {
    let mut plane = Plane::filled(12, 12, 0).unwrap();
    for index in 0..12 {
        plane
            .set(index, 3, 10 + u8::try_from(index).unwrap())
            .unwrap();
        plane
            .set(3, index, 40 + u8::try_from(index).unwrap())
            .unwrap();
    }
    plane
}

#[test]
fn top_left_block_uses_neutral_fallback_for_every_mode() {
    let plane = Plane::filled(8, 8, 7).unwrap();
    for mode in [
        IntraMode::Dc,
        IntraMode::Planar,
        IntraMode::Horizontal,
        IntraMode::Vertical,
        IntraMode::D45,
        IntraMode::D135,
        IntraMode::D117,
        IntraMode::D153,
    ] {
        assert_eq!(predict_intra(&plane, 0, 0, 8, mode).unwrap(), vec![128; 64]);
    }
}

#[test]
fn axial_and_dc_modes_use_exact_available_edges() {
    let plane = bordered_plane();
    let vertical = predict_intra(&plane, 4, 4, 4, IntraMode::Vertical).unwrap();
    assert_eq!(&vertical[..4], &[14, 15, 16, 17]);
    assert_eq!(&vertical[12..], &[14, 15, 16, 17]);
    let horizontal = predict_intra(&plane, 4, 4, 4, IntraMode::Horizontal).unwrap();
    assert_eq!(&horizontal[..4], &[44; 4]);
    assert_eq!(&horizontal[12..], &[47; 4]);
    let dc = predict_intra(&plane, 4, 4, 4, IntraMode::Dc).unwrap();
    assert!(dc.iter().all(|&sample| sample == 31));
}

#[test]
fn d45_advances_one_top_sample_per_row() {
    let plane = bordered_plane();
    let prediction = predict_intra(&plane, 4, 4, 4, IntraMode::D45).unwrap();
    assert_eq!(&prediction[..4], &[15, 16, 17, 18]);
    assert_eq!(&prediction[4..8], &[16, 17, 18, 19]);
}

/// A plane where every sample is a function of both coordinates, so a
/// substituted reference cannot coincidentally equal the correct one.
fn textured_plane(width: u32, height: u32) -> Plane {
    let mut plane = Plane::filled(width, height, 0).unwrap();
    for y in 0..height {
        for x in 0..width {
            let value = (x * 3 + y * 29) % 251 + 1;
            plane.set(x, y, u8::try_from(value).unwrap()).unwrap();
        }
    }
    plane
}

const ALL_MODES: [IntraMode; 8] = [
    IntraMode::Dc,
    IntraMode::Planar,
    IntraMode::Horizontal,
    IntraMode::Vertical,
    IntraMode::D45,
    IntraMode::D135,
    IntraMode::D117,
    IntraMode::D153,
];

#[test]
fn trap_intra_unavailable_neighbors() {
    let plane = textured_plane(16, 16);

    // First row: no reconstructed row above exists, so the top references are
    // substituted from the nearest available sample, which is the first left
    // reference. Every mode must then predict from that single value only.
    let substitute = plane.get(3, 0).unwrap();
    let vertical = predict_intra(&plane, 4, 0, 4, IntraMode::Vertical).unwrap();
    assert_eq!(
        vertical,
        vec![substitute; 16],
        "first-row block did not substitute the top edge from the left reference"
    );

    // First column: mirror case. The left references are substituted from the
    // first top reference, so a horizontal prediction is constant.
    let substitute = plane.get(0, 3).unwrap();
    let horizontal = predict_intra(&plane, 0, 4, 4, IntraMode::Horizontal).unwrap();
    assert_eq!(
        horizontal,
        vec![substitute; 16],
        "first-column block did not substitute the left edge from the top reference"
    );

    // Frame corner: neither edge exists, so every mode falls back to the
    // neutral value rather than reading uninitialized storage.
    for mode in ALL_MODES {
        assert_eq!(
            predict_intra(&plane, 0, 0, 4, mode).unwrap(),
            vec![128; 16],
            "corner block did not use the neutral fallback for {mode:?}"
        );
    }

    // Right edge: the angular modes read up to 2n+1 top samples, which runs
    // past the last column. Those positions must clamp to the final available
    // sample, not wrap to the next row and not read past the plane.
    let last_column = plane.get(15, 3).unwrap();
    let edge = predict_intra(&plane, 12, 4, 4, IntraMode::D45).unwrap();
    assert_eq!(
        edge[3], last_column,
        "top-right substitution past the frame edge did not clamp"
    );
    assert_eq!(
        edge[15], last_column,
        "top-right substitution past the frame edge did not clamp on the last row"
    );

    // Bottom edge: the left column extends past the last row in the same way,
    // and planar prediction reads exactly the sample beyond the block. Its
    // bottom-right output is the rounded blend of that clamped bottom-left
    // reference with the clamped top-right one, so a wrong clamp on either
    // side changes this byte.
    let top_right = u32::from(plane.get(8, 11).unwrap());
    let bottom_left = u32::from(plane.get(3, 15).unwrap());
    let expected = u8::try_from((4 * top_right + 4 * bottom_left + 4) / 8).unwrap();
    let bottom = predict_intra(&plane, 4, 12, 4, IntraMode::Planar).unwrap();
    assert_eq!(
        bottom[15], expected,
        "planar prediction did not clamp its references at the bottom and right edges"
    );

    // Every mode stays inside the plane at all four corners and both edges.
    for &(x, y) in &[(0, 0), (12, 0), (0, 12), (12, 12), (12, 4), (4, 12)] {
        for mode in ALL_MODES {
            assert_eq!(
                predict_intra(&plane, x, y, 4, mode).unwrap().len(),
                16,
                "mode {mode:?} failed at ({x},{y})"
            );
        }
    }
}

#[test]
fn invalid_geometry_is_rejected_before_reference_reads() {
    let plane = Plane::filled(8, 8, 0).unwrap();
    assert_eq!(
        predict_intra(&plane, 4, 4, 8, IntraMode::Dc),
        Err(PredictError::BlockOutOfBounds {
            x: 4,
            y: 4,
            size: 8
        })
    );
    assert_eq!(
        predict_intra(&plane, 0, 0, 5, IntraMode::Dc),
        Err(PredictError::InvalidSize { size: 5 })
    );
}
