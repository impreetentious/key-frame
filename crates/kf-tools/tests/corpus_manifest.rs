//! The two readings of `corpus/manifest.toml`, required to agree.
//!
//! The manifest is the repository's statement of which clips every published
//! measurement was taken on. Two programs read it: `scripts/corpus-manifest.sh`
//! for the fetch script and the shell gates, and `kf_tools::pinned_clips` for
//! the rate–distortion campaign. That is the same arrangement as the two
//! decoders, and it is worth the same only under the same condition — that
//! something compares them. Nothing did.
//!
//! They parted on three shapes, and one of them was silent: a table after the
//! last clip donated its keys to that clip on the shell side, so a manifest
//! that grew a `[fetch]` section with a `url` in it would have had the fetch
//! script download a different file into the pinned clip's name while every
//! declared field still looked present. The other two — indented keys and a
//! trailing comment — made one reader fail and the other succeed.
//!
//! So this drives both over the committed manifest and over each of those
//! shapes, and requires the same records from each. `url` and `md5` are read
//! only by the shell, deliberately, so the fields compared field-for-field are
//! the five they share; the shapes below pin the other two by construction.

use std::{fs, path::PathBuf, process::Command};

use kf_tools::{PinnedClip, pinned_clips};

mod common;
use common::scratch;

/// One record as the shell reader prints it.
#[derive(Debug, Eq, PartialEq)]
struct Record {
    name: String,
    file: String,
    url: String,
    md5: String,
    width: u32,
    height: u32,
    frames: u32,
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the workspace root is reachable from this crate")
}

/// Runs the shell reader over one manifest and parses what it prints.
fn shell_records(manifest: &std::path::Path) -> Result<Vec<Record>, String> {
    let output = Command::new("bash")
        .arg(repo_root().join("scripts/corpus-manifest.sh"))
        .arg(manifest)
        .output()
        .expect("the shell reader is executable");
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    let text = String::from_utf8(output.stdout).expect("the shell reader prints text");
    let mut records = Vec::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let fields: Vec<&str> = line.split('\t').collect();
        assert_eq!(
            fields.len(),
            7,
            "the shell reader printed {} field(s), not seven: {line:?}",
            fields.len()
        );
        let number = |index: usize| -> u32 {
            fields[index]
                .parse()
                .unwrap_or_else(|_| panic!("field {index} is not a number: {:?}", fields[index]))
        };
        records.push(Record {
            name: fields[0].to_owned(),
            file: fields[1].to_owned(),
            url: fields[2].to_owned(),
            md5: fields[3].to_owned(),
            width: number(4),
            height: number(5),
            frames: number(6),
        });
    }
    Ok(records)
}

/// Both readers over the same text, compared on the fields they share.
fn agree(label: &str, manifest_text: &str) -> Vec<Record> {
    let directory = scratch(&format!("corpus-{label}"));
    let path = directory.join("manifest.toml");
    fs::write(&path, manifest_text).expect("the manifest is writable");

    let shell =
        shell_records(&path).unwrap_or_else(|error| panic!("{label}: shell reader: {error}"));
    let native: Vec<PinnedClip> = pinned_clips(manifest_text)
        .unwrap_or_else(|error| panic!("{label}: native reader: {error}"));

    fs::remove_dir_all(&directory).ok();

    assert_eq!(
        shell.len(),
        native.len(),
        "{label}: the shell reader found {} clip(s), the native one {}",
        shell.len(),
        native.len()
    );
    for (index, (record, clip)) in shell.iter().zip(&native).enumerate() {
        assert_eq!(record.name, clip.name, "{label}: clip {index} name");
        assert_eq!(record.file, clip.file, "{label}: clip {index} file");
        assert_eq!(record.width, clip.width, "{label}: clip {index} width");
        assert_eq!(record.height, clip.height, "{label}: clip {index} height");
        assert_eq!(record.frames, clip.frames, "{label}: clip {index} frames");
    }
    shell
}

#[test]
fn both_readers_agree_on_the_committed_manifest() {
    let path = repo_root().join("corpus/manifest.toml");
    let text = fs::read_to_string(&path).expect("the manifest is in the repository");
    let records = agree("committed", &text);
    assert!(
        !records.is_empty(),
        "the committed manifest pins no clips, so this compared nothing"
    );
    for record in &records {
        assert!(
            record.url.starts_with("https://"),
            "{} is pinned to {:?}, which is not an address",
            record.name,
            record.url
        );
        assert_eq!(
            record.md5.len(),
            32,
            "{} is pinned to a checksum that is not an MD5 digest",
            record.name
        );
    }
}

#[test]
fn a_table_after_the_last_clip_belongs_to_neither_reader() {
    // The silent one. The shell reader had no rule for a table other than
    // `[[clips]]`, so these keys landed on the clip above and the fetch script
    // would have downloaded `wrong.y4m` into `one.y4m`.
    let records = agree(
        "trailing-table",
        concat!(
            "[[clips]]\n",
            "name = \"one\"\n",
            "file = \"one.y4m\"\n",
            "url = \"https://example.test/one.y4m\"\n",
            "md5 = \"00000000000000000000000000000000\"\n",
            "width = 176\n",
            "height = 144\n",
            "frames = 300\n",
            "\n",
            "[fetch]\n",
            "url = \"https://example.invalid/wrong.y4m\"\n",
            "frames = 999\n",
        ),
    );
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].url, "https://example.test/one.y4m");
    assert_eq!(records[0].frames, 300);
}

#[test]
fn indented_keys_read_the_same_in_both() {
    // TOML allows a table's keys to be indented. The native reader trimmed
    // each line and the shell reader anchored every field at column zero, so
    // an indented manifest parsed on one side and failed the build on the
    // other.
    let records = agree(
        "indented",
        concat!(
            "[[clips]]\n",
            "  name = \"one\"\n",
            "  file = \"one.y4m\"\n",
            "  url = \"https://example.test/one.y4m\"\n",
            "  md5 = \"00000000000000000000000000000000\"\n",
            "  width = 176\n",
            "  height = 144\n",
            "  frames = 300\n",
        ),
    );
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].name, "one");
    assert_eq!(records[0].frames, 300);
}

#[test]
fn a_trailing_comment_reads_the_same_in_both() {
    let records = agree(
        "commented",
        concat!(
            "[[clips]]\n",
            "name = \"one\" # the static clip\n",
            "file = \"one.y4m\"\n",
            "url = \"https://example.test/one.y4m#part\"\n",
            "md5 = \"00000000000000000000000000000000\"\n",
            "width = 176\n",
            "height = 144\n",
            "frames = 300 # the whole sequence\n",
        ),
    );
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].name, "one");
    assert_eq!(records[0].frames, 300);
    // A `#` inside quotes is part of the value, not the start of a comment.
    assert_eq!(records[0].url, "https://example.test/one.y4m#part");
}

#[test]
fn both_readers_refuse_a_manifest_with_no_clips() {
    // The one outcome worse than disagreeing is agreeing on nothing: a reader
    // that returned an empty list would let every consumer loop zero times and
    // report success.
    let directory = scratch("corpus-empty");
    let path = directory.join("manifest.toml");
    let text = "format = \"key-frame-corpus-v1\"\n";
    fs::write(&path, text).expect("the manifest is writable");

    let shell =
        shell_records(&path).expect_err("the shell reader accepted a manifest with no clips");
    let native =
        pinned_clips(text).expect_err("the native reader accepted a manifest with no clips");
    fs::remove_dir_all(&directory).ok();

    assert!(shell.contains("no clips"), "{shell}");
    assert!(native.contains("no clips"), "{native}");
}
