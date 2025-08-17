//! Proves the coverage inventory by decoding, not by reading it.
//!
//! `conformance/syntax-coverage.toml` claims which context ids and syntax
//! elements the suite exercises, and which ids no version-one stream can code
//! at all. Both halves are checked here against what the two decoders actually
//! observe while decoding every committed vector:
//!
//! - every vector the inventory names exists in the conformance manifest;
//! - the two decoders observe identical coverage on every vector, so a claim
//!   never rests on one traversal;
//! - a group's named vectors together code every id the group calls reachable;
//! - no vector — of any origin, including encoder output — ever codes a
//!   reserved id;
//! - the union over the whole suite is exactly the reachable set, so an id
//!   cannot be quietly moved to reserved to dodge writing a vector;
//! - every element in the closed set is coded, each by the vectors named for
//!   it, and at least one of those was not authored by the encoder.

use std::{collections::BTreeSet, fs, path::PathBuf, process::ExitCode};

use kf_bitstream::SyntaxElement;
use kf_dec::FastDecoder;
use kf_ref::ReferenceDecoder;

struct Vector {
    key: String,
    path: String,
}

struct Group {
    name: String,
    ids: Vec<u16>,
    reachable: Vec<u16>,
    reserved: Vec<u16>,
    vectors: Vec<String>,
}

struct Element {
    name: String,
    vectors: Vec<String>,
}

struct Measured {
    contexts: BTreeSet<u16>,
    elements: BTreeSet<String>,
}

fn main() -> ExitCode {
    match verify() {
        Ok(summary) => {
            println!("{summary}");
            ExitCode::SUCCESS
        }
        Err(failures) => {
            eprintln!("coverage: FAILED");
            for failure in failures {
                eprintln!(" - {failure}");
            }
            ExitCode::from(1)
        }
    }
}

fn verify() -> Result<String, Vec<String>> {
    let root = repository_root().map_err(|error| vec![error])?;
    let manifest = fs::read_to_string(root.join("conformance/manifest.toml"))
        .map_err(|error| vec![format!("conformance/manifest.toml: {error}")])?;
    let inventory = fs::read_to_string(root.join("conformance/syntax-coverage.toml"))
        .map_err(|error| vec![format!("conformance/syntax-coverage.toml: {error}")])?;

    let vectors = parse_vectors(&manifest);
    let groups = parse_groups(&inventory);
    let elements = parse_elements(&inventory);
    let mut failures = Vec::new();

    let mut measurements = Vec::new();
    for vector in &vectors {
        match measure(&root.join("conformance").join(&vector.path)) {
            Ok(measured) => measurements.push((vector, measured)),
            Err(error) => failures.push(format!("{}: {error}", vector.key)),
        }
    }
    if !failures.is_empty() {
        return Err(failures);
    }

    let named = |key: &str| measurements.iter().find(|(vector, _)| vector.key == key);
    let mut all_named = BTreeSet::new();
    for group in &groups {
        all_named.extend(group.vectors.iter().cloned());
    }
    for element in &elements {
        all_named.extend(element.vectors.iter().cloned());
    }
    for key in &all_named {
        if named(key).is_none() {
            failures.push(format!(
                "the inventory names {key}, which is not a vector in the conformance manifest"
            ));
        }
    }
    if !failures.is_empty() {
        return Err(failures);
    }

    let mut suite = BTreeSet::new();
    let mut suite_elements = BTreeSet::new();
    for (_, measured) in &measurements {
        suite.extend(measured.contexts.iter().copied());
        suite_elements.extend(measured.elements.iter().cloned());
    }

    let mut declared_reachable = BTreeSet::new();
    let mut declared_reserved = BTreeSet::new();
    for group in &groups {
        let mut halves: Vec<u16> = group
            .reachable
            .iter()
            .chain(group.reserved.iter())
            .copied()
            .collect();
        halves.sort_unstable();
        if halves != group.ids {
            failures.push(format!(
                "group {}: reachable plus reserved is not the group's id list",
                group.name
            ));
        }
        declared_reachable.extend(group.reachable.iter().copied());
        declared_reserved.extend(group.reserved.iter().copied());

        let mut covered = BTreeSet::new();
        for key in &group.vectors {
            if let Some((_, measured)) = named(key) {
                covered.extend(measured.contexts.iter().copied());
            }
        }
        let uncovered: Vec<u16> = group
            .reachable
            .iter()
            .copied()
            .filter(|id| !covered.contains(id))
            .collect();
        if !uncovered.is_empty() {
            failures.push(format!(
                "group {}: vectors {:?} do not code reachable ids {uncovered:?}",
                group.name, group.vectors
            ));
        }
        if !group.vectors.iter().any(|key| !key.starts_with("encoder:")) {
            failures.push(format!(
                "group {}: every named vector is encoder-authored",
                group.name
            ));
        }
    }

    for (vector, measured) in &measurements {
        let forbidden: Vec<u16> = measured
            .contexts
            .iter()
            .copied()
            .filter(|id| declared_reserved.contains(id))
            .collect();
        if !forbidden.is_empty() {
            failures.push(format!(
                "{} codes reserved ids {forbidden:?}, so they are not reserved",
                vector.key
            ));
        }
    }

    let unmet: Vec<u16> = declared_reachable.difference(&suite).copied().collect();
    if !unmet.is_empty() {
        failures.push(format!(
            "the suite never codes ids {unmet:?}, which the inventory calls reachable"
        ));
    }
    let unexpected: Vec<u16> = suite.difference(&declared_reachable).copied().collect();
    if !unexpected.is_empty() {
        failures.push(format!(
            "the suite codes ids {unexpected:?}, which the inventory does not declare reachable"
        ));
    }

    let closed: Vec<&str> = SyntaxElement::ALL
        .iter()
        .map(|element| element.name())
        .collect();
    let inventory_names: Vec<&str> = elements
        .iter()
        .map(|element| element.name.as_str())
        .collect();
    if inventory_names != closed {
        failures.push(format!(
            "inventory elements {inventory_names:?} are not the closed set {closed:?}"
        ));
    }
    for element in &elements {
        if !suite_elements.contains(&element.name) {
            failures.push(format!("no vector codes element {}", element.name));
        }
        for key in &element.vectors {
            if let Some((_, measured)) = named(key) {
                if !measured.elements.contains(&element.name) {
                    failures.push(format!("{key} does not code element {}", element.name));
                }
            }
        }
        if !element
            .vectors
            .iter()
            .any(|key| !key.starts_with("encoder:"))
        {
            failures.push(format!(
                "element {} is proved only by encoder-authored vectors",
                element.name
            ));
        }
    }

    if failures.is_empty() {
        Ok(format!(
            "coverage: OK — {} vectors, {}/{} context ids coded, {} held in reserve, {}/{} syntax elements",
            measurements.len(),
            suite.len(),
            declared_reachable.len() + declared_reserved.len(),
            declared_reserved.len(),
            suite_elements.len(),
            closed.len(),
        ))
    } else {
        Err(failures)
    }
}

/// Decodes one vector twice, independently, and returns what both saw.
fn measure(path: &std::path::Path) -> Result<Measured, String> {
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    let (fast_frames, fast) = FastDecoder::new()
        .decode_stream_coverage(&bytes)
        .map_err(|error| error.to_string())?;
    let (reference_frames, reference) = ReferenceDecoder::new()
        .decode_stream_coverage(&bytes)
        .map_err(|error| format!("reference decoder: {error}"))?;
    if fast_frames != reference_frames {
        return Err("the two decoders produced different frames".to_owned());
    }

    let fast_contexts: BTreeSet<u16> = (0..144).filter(|id| fast.contexts.contains(*id)).collect();
    let reference_contexts: BTreeSet<u16> = reference.context_ids().into_iter().collect();
    if fast_contexts != reference_contexts {
        return Err(format!(
            "the two decoders disagree on coded contexts: {:?} against {:?}",
            fast_contexts, reference_contexts
        ));
    }

    let fast_elements: BTreeSet<String> = SyntaxElement::ALL
        .into_iter()
        .filter(|element| fast.elements.contains(*element))
        .map(|element| element.name().to_owned())
        .collect();
    let reference_elements: BTreeSet<String> = reference
        .element_names()
        .into_iter()
        .map(str::to_owned)
        .collect();
    if fast_elements != reference_elements {
        return Err(format!(
            "the two decoders disagree on coded elements: {fast_elements:?} against {reference_elements:?}"
        ));
    }

    Ok(Measured {
        contexts: fast_contexts,
        elements: fast_elements,
    })
}

fn repository_root() -> Result<PathBuf, String> {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .map(PathBuf::from)
        .ok_or_else(|| "cannot locate repository root".to_owned())
}

fn parse_vectors(manifest: &str) -> Vec<Vector> {
    let mut vectors = Vec::new();
    let mut origin = None;
    let mut name = None;
    let mut stream = None;
    for line in manifest.lines() {
        if line.trim() == "[[vectors]]" {
            origin = None;
            name = None;
            stream = None;
            continue;
        }
        if let Some(value) = quoted(line, "origin") {
            origin = Some(value);
        }
        if let Some(value) = quoted(line, "name") {
            name = Some(value);
        }
        if let Some(value) = quoted(line, "stream") {
            stream = Some(value);
        }
        if let (Some(found_origin), Some(found_name), Some(found_stream)) =
            (origin.as_ref(), name.as_ref(), stream.as_ref())
        {
            vectors.push(Vector {
                key: format!("{found_origin}:{found_name}"),
                path: found_stream.clone(),
            });
            origin = None;
            name = None;
            stream = None;
        }
    }
    vectors
}

fn parse_groups(inventory: &str) -> Vec<Group> {
    let mut groups = Vec::new();
    for block in sections(inventory, "[[groups]]") {
        groups.push(Group {
            name: quoted_in(&block, "name").unwrap_or_default(),
            ids: numbers_in(&block, "ids"),
            reachable: numbers_in(&block, "reachable"),
            reserved: numbers_in(&block, "reserved"),
            vectors: strings_in(&block, "vectors"),
        });
    }
    groups
}

fn parse_elements(inventory: &str) -> Vec<Element> {
    sections(inventory, "[[elements]]")
        .into_iter()
        .map(|block| Element {
            name: quoted_in(&block, "name").unwrap_or_default(),
            vectors: strings_in(&block, "vectors"),
        })
        .collect()
}

fn sections(text: &str, header: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        if line.trim() == header {
            if let Some(block) = current.take() {
                blocks.push(block);
            }
            current = Some(String::new());
            continue;
        }
        if line.trim_start().starts_with("[[") && line.trim() != header {
            if let Some(block) = current.take() {
                blocks.push(block);
            }
            continue;
        }
        if let Some(block) = current.as_mut() {
            block.push_str(line);
            block.push('\n');
        }
    }
    if let Some(block) = current {
        blocks.push(block);
    }
    blocks
}

fn quoted(line: &str, key: &str) -> Option<String> {
    let trimmed = line.trim();
    let rest = trimmed.strip_prefix(key)?.trim_start().strip_prefix('=')?;
    let rest = rest.trim().strip_prefix('"')?;
    rest.strip_suffix('"').map(str::to_owned)
}

fn quoted_in(block: &str, key: &str) -> Option<String> {
    block.lines().find_map(|line| quoted(line, key))
}

fn array_in(block: &str, key: &str) -> Option<String> {
    block.lines().find_map(|line| {
        let trimmed = line.trim();
        let rest = trimmed.strip_prefix(key)?.trim_start().strip_prefix('=')?;
        let rest = rest.trim().strip_prefix('[')?;
        rest.strip_suffix(']').map(str::to_owned)
    })
}

fn numbers_in(block: &str, key: &str) -> Vec<u16> {
    array_in(block, key)
        .map(|body| {
            body.split(',')
                .filter_map(|value| value.trim().parse::<u16>().ok())
                .collect()
        })
        .unwrap_or_default()
}

fn strings_in(block: &str, key: &str) -> Vec<String> {
    array_in(block, key)
        .map(|body| {
            body.split(',')
                .filter_map(|value| {
                    let value = value.trim();
                    value
                        .strip_prefix('"')
                        .and_then(|rest| rest.strip_suffix('"'))
                        .map(str::to_owned)
                })
                .collect()
        })
        .unwrap_or_default()
}
