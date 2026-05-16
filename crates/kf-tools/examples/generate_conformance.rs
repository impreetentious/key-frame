use std::{env, fs, path::PathBuf, process::ExitCode};

use kf_bitstream::{
    BITSTREAM_VERSION, BlockSize, FrameFlags, FramePacket, FrameType, IntraMode, PartitionTree,
    Prediction, SequenceHeader, SyntaxWriter,
};
use kf_dec::FastDecoder;
use kf_enc::{Encoder, IntraEncoder};
use kf_frame::Frame;
use kf_range::ContextBank;
use kf_ref::ReferenceDecoder;
use kf_tools::{hand_vectors, sha256_hex, transform_schedule};

struct EncoderVector {
    name: &'static str,
    width: u16,
    height: u16,
    qp: u8,
    source: Frame,
}

struct StreamSpec<'a> {
    origin: &'a str,
    name: &'a str,
    width: u16,
    height: u16,
    qp: u8,
    frame_count: u32,
    bytes: &'a [u8],
}

struct ArtifactSink {
    output_dir: PathBuf,
    manifest: String,
    artifacts: Vec<(PathBuf, Vec<u8>)>,
}

impl ArtifactSink {
    fn new(output_dir: PathBuf) -> Self {
        Self {
            output_dir,
            manifest: String::from(
                "format = \"key-frame-conformance-v1\"\nbitstream_version = 1\n\n",
            ),
            artifacts: Vec::new(),
        }
    }

    fn push_decoded(&mut self, spec: StreamSpec<'_>) -> Result<(), String> {
        let fast = FastDecoder::new()
            .decode_stream(spec.bytes)
            .map_err(|error| error.to_string())?;
        let reference = ReferenceDecoder::new()
            .decode_stream(spec.bytes)
            .map_err(|error| error.to_string())?;
        if fast != reference {
            return Err(format!("{}:{} decoders disagree", spec.origin, spec.name));
        }
        let decoded = fast.iter().flat_map(raw_yuv).collect::<Vec<_>>();
        self.push_hashed(spec, &decoded)
    }

    fn push_hashed(&mut self, spec: StreamSpec<'_>, decoded: &[u8]) -> Result<(), String> {
        let file_name = format!("{}/{}.kfv", spec.origin, spec.name);
        self.manifest.push_str(&format!(
            "[[vectors]]\norigin = \"{}\"\nname = \"{}\"\nstream = \"{file_name}\"\nwidth = {}\nheight = {}\nframe_count = {}\nqp = {}\nstream_sha256 = \"{}\"\ndecoded_yuv_sha256 = \"{}\"\n\n",
            spec.origin,
            spec.name,
            spec.width,
            spec.height,
            spec.frame_count,
            spec.qp,
            sha256_hex(spec.bytes),
            sha256_hex(decoded),
        ));
        self.artifacts
            .push((self.output_dir.join(file_name), spec.bytes.to_vec()));
        Ok(())
    }
}

fn main() -> ExitCode {
    let check = env::args().nth(1).as_deref() == Some("--check");
    match generate(check) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("conformance: {message}");
            ExitCode::from(1)
        }
    }
}

fn generate(check: bool) -> Result<(), String> {
    if BITSTREAM_VERSION != 1 {
        return Err("conformance suite is pinned to bitstream version 1".to_owned());
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .ok_or_else(|| "cannot locate repository root".to_owned())?
        .to_path_buf();
    let mut sink = ArtifactSink::new(root.join("conformance"));

    let oracle = oracle_stream(&root)?;
    sink.push_decoded(StreamSpec {
        origin: "oracle",
        name: "intra64_dc_all_zero",
        width: 64,
        height: 64,
        qp: 32,
        frame_count: 1,
        bytes: &oracle,
    })?;

    let hand = hand_split32_dc_all_zero()?;
    sink.push_decoded(StreamSpec {
        origin: "hand",
        name: "split32_dc_all_zero",
        width: 64,
        height: 64,
        qp: 32,
        frame_count: 1,
        bytes: &hand,
    })?;

    for vector in hand_vectors()? {
        sink.push_decoded(StreamSpec {
            origin: "hand",
            name: vector.name,
            width: vector.width,
            height: vector.height,
            qp: vector.qp,
            frame_count: vector.frame_count,
            bytes: &vector.bytes,
        })?;
    }

    for vector in encoder_vectors()? {
        let sequence = SequenceHeader::new(vector.width, vector.height, 24, 1, 120, 16)
            .map_err(|error| error.to_string())?;
        let encoded = IntraEncoder::new(sequence, vector.qp)
            .map_err(|error| error.to_string())?
            .encode(std::slice::from_ref(&vector.source))
            .map_err(|error| error.to_string())?;
        let decoded = FastDecoder::new()
            .decode_stream(&encoded.bytes)
            .map_err(|error| error.to_string())?
            .remove(0);
        if vector.name.contains("gradient") && raw_yuv(&decoded) == raw_yuv(&vector.source) {
            return Err("gradient encoder stream matched the unfiltered source".to_owned());
        }
        let decoded_yuv = raw_yuv(&decoded);
        sink.push_hashed(
            StreamSpec {
                origin: "encoder",
                name: vector.name,
                width: vector.width,
                height: vector.height,
                qp: vector.qp,
                frame_count: 1,
                bytes: &encoded.bytes,
            },
            &decoded_yuv,
        )?;
    }

    let inter_sources = inter_sources()?;
    let inter_sequence =
        SequenceHeader::new(64, 64, 24, 1, 120, 2).map_err(|error| error.to_string())?;
    let inter_encoded = Encoder::new(inter_sequence, 32)
        .map_err(|error| error.to_string())?
        .encode(&inter_sources)
        .map_err(|error| error.to_string())?;
    let inter_decoded = FastDecoder::new()
        .decode_stream(&inter_encoded.bytes)
        .map_err(|error| error.to_string())?;
    let independent = ReferenceDecoder::new()
        .decode_stream(&inter_encoded.bytes)
        .map_err(|error| error.to_string())?;
    if inter_decoded != independent || inter_decoded != inter_encoded.reconstructed_frames {
        return Err("encoder inter stream disagrees across closed loop and decoders".to_owned());
    }
    let decoded_bytes = inter_decoded.iter().flat_map(raw_yuv).collect::<Vec<_>>();
    sink.push_hashed(
        StreamSpec {
            origin: "encoder",
            name: "inter_motion64_qp32",
            width: 64,
            height: 64,
            qp: 32,
            frame_count: 3,
            bytes: &inter_encoded.bytes,
        },
        &decoded_bytes,
    )?;

    let ArtifactSink {
        output_dir,
        manifest,
        mut artifacts,
    } = sink;
    artifacts.push((output_dir.join("manifest.toml"), manifest.into_bytes()));
    if check {
        for (path, expected) in artifacts {
            let actual = fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
            if actual != expected {
                // Deliberately not "regenerate the suite". These streams are
                // the frozen record of what version one means, so drift is a
                // behaviour change until proven otherwise: a codec edit that
                // silently altered reconstruction produces exactly this
                // failure, and regenerating would bake the regression into the
                // record that is supposed to catch it. Regenerating is correct
                // only for a deliberate, separately justified format change.
                return Err(format!(
                    "{} drifted from the committed conformance record.\n  \
                     This is a behaviour change until you have shown otherwise. Find what \
                     altered the coded bytes or the reconstruction first;\n  \
                     regenerate only for a format change you intended and can justify.",
                    path.display()
                ));
            }
        }
        println!("conformance: OK — oracle, hand, and encoder streams match committed hashes");
    } else {
        for origin in ["oracle", "hand", "encoder"] {
            fs::create_dir_all(output_dir.join(origin)).map_err(|error| error.to_string())?;
        }
        for (path, contents) in artifacts {
            fs::write(&path, contents).map_err(|error| format!("{}: {error}", path.display()))?;
        }
        println!("conformance: wrote {}", output_dir.display());
    }
    Ok(())
}

fn oracle_stream(root: &std::path::Path) -> Result<Vec<u8>, String> {
    let json =
        fs::read_to_string(root.join("spec/v1/vectors.json")).map_err(|error| error.to_string())?;
    let marker = "\"complete_stream_hex\": \"";
    let start = json
        .find(marker)
        .ok_or_else(|| "oracle complete_stream_hex missing".to_owned())?
        + marker.len();
    let end = json[start..]
        .find('"')
        .ok_or_else(|| "oracle complete_stream_hex unterminated".to_owned())?;
    hex_decode(&json[start..start + end])
}

fn hand_split32_dc_all_zero() -> Result<Vec<u8>, String> {
    let sequence =
        SequenceHeader::new(64, 64, 24, 1, 120, 16).map_err(|error| error.to_string())?;
    let mut writer = SyntaxWriter::new(ContextBank::initial());
    writer
        .write_partition(&PartitionTree::Split {
            size: BlockSize::N64,
            children: Box::new([
                PartitionTree::Leaf(BlockSize::N32),
                PartitionTree::Leaf(BlockSize::N32),
                PartitionTree::Leaf(BlockSize::N32),
                PartitionTree::Leaf(BlockSize::N32),
            ]),
        })
        .map_err(|error| error.to_string())?;
    for _ in 0..4 {
        writer
            .write_prediction(FrameType::Key, Prediction::Intra(IntraMode::Dc))
            .map_err(|error| error.to_string())?;
        write_zero_residual(&mut writer, BlockSize::N32)?;
    }
    let (payload, _) = writer.finish();
    let packet = FramePacket::new(
        0,
        FrameFlags {
            key: true,
            golden_refresh: true,
            show: true,
        },
        32,
        payload.bytes,
    )
    .map_err(|error| error.to_string())?;
    let mut stream = sequence.encode().to_vec();
    stream.extend_from_slice(&packet.encode());
    Ok(stream)
}

fn write_zero_residual(writer: &mut SyntaxWriter, size: BlockSize) -> Result<(), String> {
    for (plane, transform, count) in transform_schedule(size) {
        for _ in 0..count {
            writer
                .write_coefficients(plane, transform, &vec![0; transform.side().pow(2)])
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

fn encoder_vectors() -> Result<Vec<EncoderVector>, String> {
    let neutral = Frame::filled_420(64, 64, 128).map_err(|error| error.to_string())?;
    let mut gradient = Frame::filled_420(66, 64, 0).map_err(|error| error.to_string())?;
    for y in 0..64 {
        for x in 0..66 {
            gradient
                .y
                .set(x, y, u8::try_from((x * 3 + y * 2) % 256).unwrap())
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(vec![
        EncoderVector {
            name: "neutral64_qp28",
            width: 64,
            height: 64,
            qp: 28,
            source: neutral,
        },
        EncoderVector {
            name: "gradient66x64_qp32",
            width: 66,
            height: 64,
            qp: 32,
            source: gradient,
        },
    ])
}

fn inter_sources() -> Result<Vec<Frame>, String> {
    let mut first = Frame::filled_420(64, 64, 96).map_err(|error| error.to_string())?;
    for y in 0..64 {
        for x in 0..64 {
            first
                .y
                .set(x, y, u8::try_from((x * 5 + y * 3) % 256).unwrap())
                .map_err(|error| error.to_string())?;
        }
    }
    let mut second = first.clone();
    let mut third = first.clone();
    for y in 0..64 {
        for x in 0..64 {
            second
                .y
                .set(x, y, first.y.get(x.saturating_sub(1), y).unwrap())
                .map_err(|error| error.to_string())?;
            third
                .y
                .set(x, y, first.y.get(x.saturating_sub(2), y).unwrap())
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(vec![first, second, third])
}

fn hex_decode(hex: &str) -> Result<Vec<u8>, String> {
    if !hex.len().is_multiple_of(2) {
        return Err("oracle stream hex has an odd length".to_owned());
    }
    (0..hex.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&hex[index..index + 2], 16)
                .map_err(|_| "oracle stream hex is not hexadecimal".to_owned())
        })
        .collect()
}

fn raw_yuv(frame: &Frame) -> Vec<u8> {
    let mut bytes =
        Vec::with_capacity(frame.y.data().len() + frame.cb.data().len() + frame.cr.data().len());
    bytes.extend_from_slice(frame.y.data());
    bytes.extend_from_slice(frame.cb.data());
    bytes.extend_from_slice(frame.cr.data());
    bytes
}
