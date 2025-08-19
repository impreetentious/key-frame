//! Random access must be an optimization, never a different answer.
//!
//! For every frame of every stream checked here, seeking to that frame is
//! required to produce exactly the image a linear decode produces, in both
//! decoders, and to have restarted from the keyframe that governs it. A seek
//! that is merely close is a seek that is wrong.
//!
//! Two stream families are checked. The committed conformance vectors cover the
//! shapes the suite already pins. A freshly encoded clip with a short keyframe
//! interval covers what those cannot: several keyframes, so the entry point
//! actually moves, and P frames far enough behind one that a wrong entry point
//! would show.

use std::{fs, path::PathBuf, process::ExitCode};

use kf_bitstream::SequenceHeader;
use kf_dec::{FastDecoder, StreamIndex};
use kf_enc::Encoder;
use kf_frame::Frame;
use kf_ref::ReferenceDecoder;

const KEY_INTERVAL: u16 = 4;
const CLIP_FRAMES: u32 = 12;

fn main() -> ExitCode {
    match run() {
        Ok(summary) => {
            println!("{summary}");
            ExitCode::SUCCESS
        }
        Err(failures) => {
            eprintln!("seek: FAILED");
            for failure in failures {
                eprintln!(" - {failure}");
            }
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<String, Vec<String>> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .map(PathBuf::from)
        .ok_or_else(|| vec!["cannot locate repository root".to_owned()])?;

    let mut failures = Vec::new();
    let mut checked_frames = 0_usize;
    let mut checked_streams = 0_usize;

    let multi = multi_keyframe_stream().map_err(|error| vec![error])?;
    match check_stream("multi-keyframe clip", &multi) {
        Ok(frames) => {
            checked_frames += frames;
            checked_streams += 1;
        }
        Err(mut found) => failures.append(&mut found),
    }
    if let Err(mut found) = check_entry_points(&multi) {
        failures.append(&mut found);
    }

    for path in conformance_streams(&root).map_err(|error| vec![error])? {
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) => {
                failures.push(format!("{}: {error}", path.display()));
                continue;
            }
        };
        let label = path
            .strip_prefix(&root)
            .unwrap_or(&path)
            .display()
            .to_string();
        match check_stream(&label, &bytes) {
            Ok(frames) => {
                checked_frames += frames;
                checked_streams += 1;
            }
            Err(mut found) => failures.append(&mut found),
        }
    }

    if failures.is_empty() {
        Ok(format!(
            "seek: OK — {checked_frames} frame(s) across {checked_streams} stream(s) seek to the same image a linear decode produces"
        ))
    } else {
        Err(failures)
    }
}

/// Seeks to every frame of one stream and compares against a linear decode.
fn check_stream(label: &str, bytes: &[u8]) -> Result<usize, Vec<String>> {
    let mut failures = Vec::new();
    let linear = match FastDecoder::new().decode_stream(bytes) {
        Ok(frames) => frames,
        Err(error) => return Err(vec![format!("{label}: linear decode failed: {error}")]),
    };
    let independent = match ReferenceDecoder::new().decode_stream(bytes) {
        Ok(frames) => frames,
        Err(error) => {
            return Err(vec![format!(
                "{label}: independent linear decode failed: {error}"
            )]);
        }
    };
    if linear != independent {
        return Err(vec![format!("{label}: the two linear decodes disagree")]);
    }

    let index = match StreamIndex::scan(bytes) {
        Ok(index) => index,
        Err(error) => return Err(vec![format!("{label}: index scan failed: {error}")]),
    };
    if index.frame_count() != linear.len() {
        failures.push(format!(
            "{label}: index counts {} frames, linear decode produced {}",
            index.frame_count(),
            linear.len()
        ));
    }
    let keyframes = index.keyframes();
    if keyframes.first() != Some(&0) {
        failures.push(format!("{label}: the stream does not begin at a keyframe"));
    }

    for (position, expected) in linear.iter().enumerate() {
        let target = match u32::try_from(position) {
            Ok(target) => target,
            Err(_) => {
                failures.push(format!("{label}: frame {position} exceeds a u32 index"));
                continue;
            }
        };
        let fast = match FastDecoder::new().seek_frame(bytes, target) {
            Ok(outcome) => outcome,
            Err(error) => {
                failures.push(format!("{label}: seek to {target} failed: {error}"));
                continue;
            }
        };
        let reference = match ReferenceDecoder::new().seek_frame(bytes, target) {
            Ok(outcome) => outcome,
            Err(error) => {
                failures.push(format!(
                    "{label}: independent seek to {target} failed: {error}"
                ));
                continue;
            }
        };
        if &fast.frame != expected {
            failures.push(format!(
                "{label}: seek to {target} does not match the linear decode"
            ));
        }
        if fast.frame != reference.frame {
            failures.push(format!(
                "{label}: the two decoders seek to {target} differently"
            ));
        }
        if fast.keyframe_index != reference.keyframe_index
            || fast.frames_decoded != reference.frames_decoded
        {
            failures.push(format!(
                "{label}: seek to {target} restarted at {}/{} frames in one decoder and {}/{} in the other",
                fast.keyframe_index,
                fast.frames_decoded,
                reference.keyframe_index,
                reference.frames_decoded
            ));
        }

        let expected_entry = keyframes
            .iter()
            .rev()
            .find(|key| **key <= target)
            .copied()
            .unwrap_or(0);
        if fast.keyframe_index != expected_entry {
            failures.push(format!(
                "{label}: seek to {target} restarted at {} rather than the governing keyframe {expected_entry}",
                fast.keyframe_index
            ));
        }
        let expected_work = usize::try_from(target - expected_entry).unwrap_or(usize::MAX) + 1;
        if fast.frames_decoded != expected_work {
            failures.push(format!(
                "{label}: seek to {target} decoded {} frames rather than {expected_work}",
                fast.frames_decoded
            ));
        }

        // Resuming from a seek must also agree with the linear tail.
        match FastDecoder::new().decode_from(bytes, target) {
            Ok(tail) => {
                if tail.as_slice() != &linear[position..] {
                    failures.push(format!(
                        "{label}: decoding from {target} does not match the linear tail"
                    ));
                }
            }
            Err(error) => failures.push(format!("{label}: decode from {target} failed: {error}")),
        }
    }

    let past_end = u32::try_from(linear.len()).unwrap_or(u32::MAX);
    if FastDecoder::new().seek_frame(bytes, past_end).is_ok() {
        failures.push(format!("{label}: seeking past the last frame succeeded"));
    }
    if ReferenceDecoder::new().seek_frame(bytes, past_end).is_ok() {
        failures.push(format!(
            "{label}: independently seeking past the last frame succeeded"
        ));
    }

    if failures.is_empty() {
        Ok(linear.len())
    } else {
        Err(failures)
    }
}

/// The entry point has to move with the keyframes, not with the frame index.
/// A stream whose only keyframe is the first frame would pass every equality
/// check above while seeking did no work at all, so this asserts the shape.
fn check_entry_points(bytes: &[u8]) -> Result<(), Vec<String>> {
    let index = StreamIndex::scan(bytes).map_err(|error| vec![error.to_string()])?;
    let keyframes = index.keyframes();
    if keyframes.len() < 3 {
        return Err(vec![format!(
            "the multi-keyframe clip carries {} keyframes; it must carry at least three for seeking to be exercised",
            keyframes.len()
        )]);
    }
    let mut failures = Vec::new();
    let last_key = *keyframes.last().unwrap_or(&0);
    let entry = index
        .entry_point(last_key)
        .map_err(|error| vec![error.to_string()])?;
    if entry != last_key {
        failures.push(format!(
            "seeking to keyframe {last_key} resolved to entry point {entry}"
        ));
    }
    if let Ok(outcome) = FastDecoder::new().seek_frame(bytes, last_key)
        && outcome.frames_decoded != 1
    {
        failures.push(format!(
            "seeking to keyframe {last_key} decoded {} frames rather than one",
            outcome.frames_decoded
        ));
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures)
    }
}

fn multi_keyframe_stream() -> Result<Vec<u8>, String> {
    let sequence =
        SequenceHeader::new(64, 64, 24, 1, KEY_INTERVAL, 2).map_err(|error| error.to_string())?;
    let sources = (0..CLIP_FRAMES)
        .map(panning_frame)
        .collect::<Result<Vec<_>, _>>()?;
    let encoded = Encoder::new(sequence, 32)
        .map_err(|error| error.to_string())?
        .encode(&sources)
        .map_err(|error| error.to_string())?;
    Ok(encoded.bytes)
}

/// A diagonal ramp that pans one sample per frame, so consecutive frames are
/// similar enough for inter prediction and different enough that decoding the
/// wrong one is visible.
fn panning_frame(index: u32) -> Result<Frame, String> {
    let mut frame = Frame::filled_420(64, 64, 0).map_err(|error| error.to_string())?;
    for y in 0..64 {
        for x in 0..64 {
            let value = u8::try_from((x * 3 + y * 5 + index * 7) % 256)
                .map_err(|_| "sample out of range".to_owned())?;
            frame
                .y
                .set(x, y, value)
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(frame)
}

fn conformance_streams(root: &std::path::Path) -> Result<Vec<PathBuf>, String> {
    let mut streams = Vec::new();
    for origin in ["oracle", "hand", "encoder"] {
        let directory = root.join("conformance").join(origin);
        let entries = fs::read_dir(&directory)
            .map_err(|error| format!("{}: {error}", directory.display()))?;
        let mut found = Vec::new();
        for entry in entries {
            let path = entry.map_err(|error| error.to_string())?.path();
            if path.extension().is_some_and(|extension| extension == "kfv") {
                found.push(path);
            }
        }
        found.sort();
        streams.extend(found);
    }
    Ok(streams)
}
