//! The pinned corpus, read from the file that pins it.
//!
//! `corpus/manifest.toml` is the repository's statement of which clips the
//! published measurements use. The shell side reads it through
//! `scripts/corpus-manifest.sh`; this is the same reading for the tools, so the
//! rate–distortion campaign covers the corpus rather than a list of clip names
//! that happened to be true when the campaign was written.
//!
//! The parser is deliberately small and strict. It understands the shape this
//! one manifest has — a sequence of `[[clips]]` tables of `key = value` lines —
//! and refuses anything else rather than guessing, because a clip silently
//! dropped from a measurement is a published number describing a corpus that
//! was never measured.
//!
//! Two readers of one normative file only mean something if something compares
//! them, which is the same argument the two decoders rest on.
//! `tests/corpus_manifest.rs` drives this one and the shell one over the
//! committed manifest and over the shapes where they could plausibly part —
//! a second table after the last clip, indented keys, a trailing comment — and
//! requires the same answer from each. They did part on all three.

/// One clip the corpus manifest pins.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PinnedClip {
    /// Stable name, used in receipts and gate output.
    pub name: String,
    /// File name inside the clip directory.
    pub file: String,
    /// Declared luma dimensions.
    pub width: u32,
    pub height: u32,
    /// Declared frame count of the whole sequence.
    pub frames: u32,
}

/// Everything the manifest pins, in declared order.
///
/// # Errors
///
/// Returns a message naming the clip and the field when a clip is missing one,
/// and when the manifest declares no clips at all — a caller that looped over
/// an empty list would report success having measured nothing.
pub fn pinned_clips(manifest: &str) -> Result<Vec<PinnedClip>, String> {
    let mut clips = Vec::new();
    let mut current: Option<Partial> = None;

    for line in manifest.lines() {
        let line = line.trim();
        if line == "[[clips]]" {
            if let Some(partial) = current.take() {
                clips.push(partial.finish()?);
            }
            current = Some(Partial::default());
            continue;
        }
        if line.starts_with('[') {
            // Any other table ends the clip being read. The manifest's
            // preamble is such a region and carries no clip fields.
            if let Some(partial) = current.take() {
                clips.push(partial.finish()?);
            }
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let Some(partial) = current.as_mut() else {
            continue;
        };
        partial.set(key.trim(), &unquote(value))?;
    }
    if let Some(partial) = current.take() {
        clips.push(partial.finish()?);
    }

    if clips.is_empty() {
        return Err("corpus manifest declares no clips".to_owned());
    }
    Ok(clips)
}

/// The text of one `key = value` right-hand side.
///
/// A value is either a quoted string or a bare integer, and a `#` outside
/// quotes begins a comment. Trimming quote characters off both ends — which is
/// what this did — read `name = "one" # the static clip` as `one" # the static
/// clip`, because the trailing character was not a quote and nothing stripped
/// the comment. The shell reader applies the same rule, and the two are
/// compared on exactly these shapes.
fn unquote(value: &str) -> String {
    let value = value.trim();
    if let Some(rest) = value.strip_prefix('"') {
        return match rest.split_once('"') {
            Some((inside, _)) => inside.to_owned(),
            None => rest.to_owned(),
        };
    }
    match value.split_once('#') {
        Some((before, _)) => before.trim_end().to_owned(),
        None => value.to_owned(),
    }
}

#[derive(Default)]
struct Partial {
    name: Option<String>,
    file: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    frames: Option<u32>,
}

impl Partial {
    fn set(&mut self, key: &str, value: &str) -> Result<(), String> {
        let number = |value: &str| -> Result<u32, String> {
            value
                .parse::<u32>()
                .map_err(|_| format!("corpus manifest {key} is not a whole number: {value}"))
        };
        match key {
            "name" => self.name = Some(value.to_owned()),
            "file" => self.file = Some(value.to_owned()),
            "width" => self.width = Some(number(value)?),
            "height" => self.height = Some(number(value)?),
            "frames" => self.frames = Some(number(value)?),
            // `url` and `md5` belong to fetching, which the shell does. Naming
            // them here would be a second place to keep them right.
            _ => {}
        }
        Ok(())
    }

    fn finish(self) -> Result<PinnedClip, String> {
        let name = self
            .name
            .ok_or_else(|| "corpus manifest declares a clip with no name".to_owned())?;
        let missing = |field: &str| format!("corpus manifest clip {name} declares no {field}");
        Ok(PinnedClip {
            file: self.file.clone().ok_or_else(|| missing("file"))?,
            width: self.width.ok_or_else(|| missing("width"))?,
            height: self.height.ok_or_else(|| missing("height"))?,
            frames: self.frames.ok_or_else(|| missing("frames"))?,
            name,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{PinnedClip, pinned_clips, unquote};

    const MANIFEST: &str = include_str!("../../../corpus/manifest.toml");

    #[test]
    fn the_committed_manifest_parses_completely() {
        let clips = pinned_clips(MANIFEST).unwrap();
        assert!(!clips.is_empty());
        for clip in &clips {
            assert!(!clip.name.is_empty());
            assert_eq!(clip.file, format!("{}.y4m", clip.name));
            // Even dimensions inside the format's declared limits, because a
            // clip outside them could not be encoded and would fail far from
            // here with a message about a picture size instead of a corpus.
            assert!(clip.width >= 64 && clip.height >= 64);
            assert_eq!(clip.width % 2, 0);
            assert_eq!(clip.height % 2, 0);
            assert!(clip.frames > 0);
        }
    }

    #[test]
    fn a_clip_missing_a_field_is_an_error_rather_than_a_hole() {
        let manifest = "[[clips]]\nname = \"only\"\nfile = \"only.y4m\"\nwidth = 64\n";
        let error = pinned_clips(manifest).unwrap_err();
        assert!(error.contains("only"), "{error}");
        assert!(error.contains("height"), "{error}");
    }

    #[test]
    fn a_manifest_with_no_clips_is_an_error_rather_than_an_empty_list() {
        let error = pinned_clips("format = \"key-frame-corpus-v1\"\n").unwrap_err();
        assert!(error.contains("no clips"), "{error}");
    }

    #[test]
    fn a_preamble_before_the_first_clip_is_not_read_as_one() {
        let manifest = concat!(
            "format = \"key-frame-corpus-v1\"\n",
            "source = \"somewhere\"\n",
            "\n",
            "[[clips]]\n",
            "name = \"one\"\n",
            "file = \"one.y4m\"\n",
            "width = 176\n",
            "height = 144\n",
            "frames = 300\n",
        );
        assert_eq!(
            pinned_clips(manifest).unwrap(),
            vec![PinnedClip {
                name: "one".to_owned(),
                file: "one.y4m".to_owned(),
                width: 176,
                height: 144,
                frames: 300,
            }]
        );
    }

    #[test]
    fn a_trailing_comment_is_not_part_of_the_value() {
        // Both halves of the rule. The quoted form ends at its closing quote,
        // so a `#` after it is a comment; the bare form ends at the first `#`.
        // Read the other way round, `frames` became a frame count with prose
        // in it and `name` kept a stray quote.
        let manifest = concat!(
            "[[clips]]\n",
            "name = \"one\" # the static clip\n",
            "file = \"one.y4m\"\n",
            "width = 176\n",
            "height = 144\n",
            "frames = 300 # the whole sequence\n",
        );
        assert_eq!(
            pinned_clips(manifest).unwrap(),
            vec![PinnedClip {
                name: "one".to_owned(),
                file: "one.y4m".to_owned(),
                width: 176,
                height: 144,
                frames: 300,
            }]
        );
    }

    #[test]
    fn a_hash_inside_a_quoted_value_is_kept() {
        // A `#` only begins a comment outside quotes. Stripping from the first
        // one anywhere would truncate any address carrying a fragment.
        assert_eq!(
            unquote(" \"https://example.test/a.y4m#part\" "),
            "https://example.test/a.y4m#part"
        );
        assert_eq!(unquote(" 300 # prose "), "300");
        assert_eq!(unquote(" \"unterminated"), "unterminated");
    }

    #[test]
    fn a_table_after_the_last_clip_does_not_extend_it() {
        // The shape the shell reader used to get wrong: keys under a later
        // table were attributed to the clip above it.
        let manifest = concat!(
            "[[clips]]\n",
            "name = \"one\"\n",
            "file = \"one.y4m\"\n",
            "width = 176\n",
            "height = 144\n",
            "frames = 300\n",
            "\n",
            "[fetch]\n",
            "frames = 999\n",
        );
        assert_eq!(pinned_clips(manifest).unwrap()[0].frames, 300);
    }

    #[test]
    fn clips_keep_their_declared_order() {
        let manifest = concat!(
            "[[clips]]\nname = \"first\"\nfile = \"first.y4m\"\n",
            "width = 64\nheight = 64\nframes = 2\n",
            "[[clips]]\nname = \"second\"\nfile = \"second.y4m\"\n",
            "width = 64\nheight = 64\nframes = 2\n",
        );
        let names: Vec<_> = pinned_clips(manifest)
            .unwrap()
            .into_iter()
            .map(|clip| clip.name)
            .collect();
        assert_eq!(names, ["first", "second"]);
    }
}
