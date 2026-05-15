//! Writes a deterministic synthetic Y4M clip.
//!
//! Two things need sources that are not the pinned corpus. Ablations need
//! content chosen to isolate one tool — a clip that pans rigidly says something
//! about motion compensation that a talking head does not — and a reader
//! trying the command-line tools should not have to fetch eighty megabytes of
//! video first.
//!
//! Every pattern is a pure function of the seed and the frame index, so a clip
//! is identified by its arguments and two runs anywhere produce the same bytes.
//! That is what lets a receipt name a synthetic source without shipping it.

use std::{env, fs, process::ExitCode};

use kf_core::Xoshiro256PlusPlus;
use kf_frame::Frame;
use kf_tools::{Y4mStream, encode_y4m, sha256_hex};

const USAGE: &str = "\
usage: make_clip --output <clip.y4m> [options]

  --width N        picture width, even, default 176
  --height N       picture height, even, default 144
  --frames N       frame count, default 16
  --fps N/D        frame rate, default 30/1
  --seed N         generator seed, default 1
  --pattern NAME   one of:
                     flat      a constant grey field
                     gradient  a static diagonal ramp
                     noise     independent samples every frame
                     motion    a textured field panning at a fixed velocity
                     cut       motion, with a hard scene change halfway
                     fade      a gradient dimming linearly to black";

fn main() -> ExitCode {
    match run(env::args().skip(1).collect()) {
        Ok(message) => {
            println!("{message}");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("make-clip: {message}");
            eprintln!("{USAGE}");
            ExitCode::from(1)
        }
    }
}

fn run(arguments: Vec<String>) -> Result<String, String> {
    let output = value(&arguments, "--output").ok_or("no --output given")?;
    let width = parse(&arguments, "--width", 176)?;
    let height = parse(&arguments, "--height", 144)?;
    let frames = parse(&arguments, "--frames", 16)?;
    let seed = u64::from(parse(&arguments, "--seed", 1)?);
    let pattern = value(&arguments, "--pattern").unwrap_or_else(|| "gradient".to_owned());
    let (fps_num, fps_den) = frame_rate(value(&arguments, "--fps").as_deref().unwrap_or("30/1"))?;

    if width < 2 || height < 2 || !width.is_multiple_of(2) || !height.is_multiple_of(2) {
        return Err(format!("{width}x{height} is not an even picture size"));
    }
    if frames == 0 {
        return Err("a clip needs at least one frame".to_owned());
    }

    let mut clip = Vec::with_capacity(frames as usize);
    for index in 0..frames {
        clip.push(draw(&pattern, width, height, index, frames, seed)?);
    }

    let narrow = |value: u32, what: &str| {
        u16::try_from(value).map_err(|_| format!("{what} {value} does not fit a Y4M header field"))
    };
    let bytes = encode_y4m(&Y4mStream {
        width: narrow(width, "width")?,
        height: narrow(height, "height")?,
        fps_num: narrow(fps_num, "frame rate numerator")?,
        fps_den: narrow(fps_den, "frame rate denominator")?,
        frames: clip,
    })
    .map_err(|error| error.to_string())?;
    fs::write(&output, &bytes).map_err(|error| format!("{output}: {error}"))?;
    Ok(format!(
        "make-clip: wrote {output} — {pattern} {width}x{height} {frames} frames, sha256 {}",
        sha256_hex(&bytes)
    ))
}

/// Renders one frame of a named pattern.
fn draw(
    pattern: &str,
    width: u32,
    height: u32,
    index: u32,
    frames: u32,
    seed: u64,
) -> Result<Frame, String> {
    let mut frame = Frame::filled_420(width, height, 128).map_err(|error| error.to_string())?;
    match pattern {
        "flat" => return Ok(frame),
        "gradient" => {
            for y in 0..height {
                for x in 0..width {
                    set(&mut frame, x, y, ramp(x, y));
                }
            }
        }
        "noise" => {
            // Seeded per frame rather than carried across frames, so any single
            // frame of the clip can be regenerated on its own.
            let mut generator = seeded(seed ^ (u64::from(index) << 32));
            for y in 0..height {
                for x in 0..width {
                    let sample = (generator.next_u64() >> 32) as u32;
                    set(&mut frame, x, y, (sample & 0xFF) as u8);
                }
            }
        }
        "motion" => {
            let field = texture(width, height, seed);
            pan(&mut frame, &field, width, height, index, 0);
        }
        "cut" => {
            // A hard content change halfway, which is what a scene-cut decision
            // has to notice. Both halves pan, so the change is in the content
            // and not merely in whether anything is moving.
            let half = frames / 2;
            let (source, phase) = if index < half {
                (texture(width, height, seed), index)
            } else {
                (
                    texture(width, height, seed ^ 0x9E37_79B9_7F4A_7C15),
                    index - half,
                )
            };
            pan(
                &mut frame,
                &source,
                width,
                height,
                phase,
                u32::from(index >= half),
            );
        }
        "fade" => {
            // Every sample changes by a small amount every frame: nothing is
            // static, but nothing moves either. Motion search has nothing to
            // find here and the residual path carries the whole clip.
            let scale = frames.saturating_sub(1).max(1);
            for y in 0..height {
                for x in 0..width {
                    let base = u32::from(ramp(x, y));
                    let dimmed = base * (scale - index.min(scale)) / scale;
                    set(&mut frame, x, y, dimmed as u8);
                }
            }
        }
        other => return Err(format!("{other} is not a pattern")),
    }
    Ok(frame)
}

/// A static diagonal ramp with a coarse checker over it, so a frame has both
/// low-frequency and block-scale content.
fn ramp(x: u32, y: u32) -> u8 {
    let diagonal = (x * 3 + y * 2) % 256;
    let checker = if ((x / 16) + (y / 16)).is_multiple_of(2) {
        16
    } else {
        0
    };
    ((diagonal + checker) % 256) as u8
}

/// A fixed noise field, generated once and then translated.
///
/// Translating one field is what makes this useful: the content is identical
/// frame to frame, so a residual that is not near zero means prediction missed,
/// not that the picture changed.
fn texture(width: u32, height: u32, seed: u64) -> Vec<u8> {
    let mut generator = seeded(seed);
    let stride = width + 64;
    let rows = height + 64;
    let span = stride * rows;
    let mut field = Vec::with_capacity(span as usize);
    for _ in 0..span {
        let sample = ((generator.next_u64() >> 40) & 0xFF) as u32;
        field.push(((sample + 128) / 2) as u8);
    }

    // Two box-blur passes over the noise.
    //
    // White noise is the wrong test content for a codec demo in both
    // directions: it is close to incompressible, so the numbers look bad for a
    // reason that has nothing to do with the codec, and it has no structure
    // below the sample, so sub-pixel motion has nothing to interpolate towards
    // and the quadtree has no edge to follow. Blurring gives the field
    // low-frequency content — which is what real pictures are mostly made of —
    // while leaving it deterministic.
    for _ in 0..2 {
        let source = field.clone();
        for y in 1..rows - 1 {
            for x in 1..stride - 1 {
                let mut total = 0_u32;
                for dy in 0..3_u32 {
                    for dx in 0..3_u32 {
                        let index = (y + dy - 1) * stride + (x + dx - 1);
                        total += u32::from(source[index as usize]);
                    }
                }
                field[(y * stride + x) as usize] = (total / 9) as u8;
            }
        }
    }
    field
}

/// Copies a window of `field` into `frame`, displaced by a fixed velocity.
fn pan(frame: &mut Frame, field: &[u8], width: u32, height: u32, phase: u32, variant: u32) {
    let stride = width + 64;
    // Three across and two down per frame: not a whole number of blocks, so the
    // motion is not accidentally free.
    let (dx, dy) = if variant == 0 { (3, 2) } else { (2, 3) };
    for y in 0..height {
        for x in 0..width {
            let source_x = (x + (phase * dx)) % (width + 63);
            let source_y = (y + (phase * dy)) % (height + 63);
            let sample = field[(source_y * stride + source_x) as usize];
            set(frame, x, y, sample);
        }
    }
}

fn set(frame: &mut Frame, x: u32, y: u32, value: u8) {
    frame
        .y
        .set(x, y, value)
        .expect("the coordinate is inside the plane");
}

/// Expands a small seed into the generator's full state.
///
/// The generator wants four non-zero words and a caller wants to type one
/// number, so the seed is stirred through SplitMix64 first. Seeding all four
/// words from the same value would leave visible structure in the first
/// outputs.
fn seeded(seed: u64) -> Xoshiro256PlusPlus {
    let mut state = [0_u64; 4];
    let mut current = seed;
    for word in &mut state {
        current = current.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = current;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        *word = z ^ (z >> 31);
    }
    Xoshiro256PlusPlus::from_state(state)
}

fn frame_rate(text: &str) -> Result<(u32, u32), String> {
    let (numerator, denominator) = text
        .split_once('/')
        .ok_or_else(|| format!("{text} is not a frame rate like 30/1"))?;
    let numerator = numerator
        .parse::<u32>()
        .map_err(|_| format!("{text} has a non-numeric numerator"))?;
    let denominator = denominator
        .parse::<u32>()
        .map_err(|_| format!("{text} has a non-numeric denominator"))?;
    if numerator == 0 || denominator == 0 {
        return Err(format!("{text} is not a positive frame rate"));
    }
    Ok((numerator, denominator))
}

fn value(arguments: &[String], flag: &str) -> Option<String> {
    arguments
        .iter()
        .position(|argument| argument == flag)
        .and_then(|at| arguments.get(at + 1))
        .cloned()
}

fn parse(arguments: &[String], flag: &str, fallback: u64) -> Result<u32, String> {
    match value(arguments, flag) {
        None => u32::try_from(fallback).map_err(|_| format!("{flag} default is out of range")),
        Some(text) => text
            .parse::<u32>()
            .map_err(|_| format!("{flag} must be a non-negative integer, not {text}")),
    }
}
