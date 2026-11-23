//! Runs every recorded transcript in `testdata/transcripts` through the core.
//!
//! Set `UPDATE_VECTORS=1` to rewrite the expected renderings in `testdata/vectors`.

use bridge_core::{
    log::{LogStore, MemoryLog, condense},
    render,
    state::SessionState,
    transcript::Transcript,
};
use bridge_schema::{acp, rules::RuleSet};
use indexmap::IndexMap;
use std::{
    fs,
    path::{Path, PathBuf},
};

fn testdata() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata")
}

/// Every transcript as `(set/name, transcript)`, sorted.
fn transcripts() -> Vec<(String, Transcript)> {
    let root = testdata().join("transcripts");
    let mut found = Vec::new();
    for set in fs::read_dir(&root)
        .expect("testdata/transcripts exists")
        .flatten()
    {
        if !set.path().is_dir() {
            continue;
        }
        for file in fs::read_dir(set.path()).expect("readable set").flatten() {
            let path = file.path();
            if path.extension().is_some_and(|ext| ext == "ndjson") {
                let name = format!(
                    "{}/{}",
                    set.file_name().to_string_lossy(),
                    path.file_stem().unwrap().to_string_lossy()
                );
                let text = fs::read_to_string(&path).expect("readable transcript");
                let transcript =
                    Transcript::parse(&text).unwrap_or_else(|error| panic!("{name}: {error}"));
                found.push((name, transcript));
            }
        }
    }
    found.sort_by(|a, b| a.0.cmp(&b.0));
    assert!(!found.is_empty(), "no transcripts found");
    found
}

/// The state of each session after applying the given updates.
fn reduce<'a>(
    rules: &RuleSet,
    updates: impl Iterator<Item = (&'a str, &'a serde_json::Value)>,
) -> IndexMap<String, SessionState> {
    let mut sessions: IndexMap<String, SessionState> = IndexMap::new();
    for (session_id, update) in updates {
        sessions
            .entry(session_id.to_owned())
            .or_default()
            .apply_update(rules, update)
            .expect("update applies");
    }
    sessions
}

#[test]
fn rendering_matches_the_vectors() {
    let rules = RuleSet::pinned();
    let update = std::env::var_os("UPDATE_VECTORS").is_some();
    let mut stale = Vec::new();
    for (name, transcript) in transcripts() {
        let conversation = transcript
            .conversation(&rules)
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        let mut rendered = format!("# {name}\n");
        for prompt in &transcript.header.prompts {
            rendered.push_str(&format!("# prompt: {prompt}\n"));
        }
        rendered.push('\n');
        rendered.push_str(&render::conversation(&conversation));

        let path = testdata().join("vectors").join(format!("{name}.txt"));
        if update {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, &rendered).unwrap();
        } else if fs::read_to_string(&path).ok().as_deref() != Some(rendered.as_str()) {
            stale.push(name);
        }
    }
    assert!(
        stale.is_empty(),
        "renderings differ from testdata/vectors for {stale:?}; run with UPDATE_VECTORS=1 and review the diff"
    );
}

#[test]
fn every_update_kind_is_known_to_the_rules() {
    let rules = RuleSet::pinned();
    for (name, transcript) in transcripts() {
        for (session_id, state) in reduce(&rules, transcript.updates()) {
            assert!(
                state.unknown.is_empty(),
                "{name} session {session_id}: unknown updates {:?}",
                state.unknown
            );
        }
    }
}

#[test]
fn a_condensed_replay_gives_the_same_state() {
    let rules = RuleSet::pinned();
    let (mut total, mut kept) = (0, 0);
    for (name, transcript) in transcripts() {
        let mut log = MemoryLog::default();
        for (session_id, update) in transcript.updates() {
            log.append(session_id, update.clone());
        }
        let all = log.after(0);
        let condensed = condense(&rules, &all);
        total += all.len();
        kept += condensed.len();

        let full = reduce(
            &rules,
            all.iter()
                .map(|entry| (entry.session_id.as_str(), &entry.update)),
        );
        let replayed = reduce(
            &rules,
            condensed
                .iter()
                .map(|entry| (entry.session_id.as_str(), &entry.update)),
        );
        assert_eq!(full, replayed, "{name}: condensed replay differs");
        assert_eq!(
            condensed.last().map(|entry| entry.seq),
            all.last().map(|entry| entry.seq),
            "{name}: head moved"
        );
        assert!(
            condensed.windows(2).all(|pair| pair[0].seq < pair[1].seq),
            "{name}: sequence numbers out of order"
        );
    }
    assert!(
        kept < total,
        "condensing removed nothing ({kept} of {total})"
    );
    println!("condensed {total} updates to {kept}");
}

#[test]
fn catching_up_from_any_point_gives_the_same_state() {
    let rules = RuleSet::pinned();
    for (name, transcript) in transcripts() {
        let mut log = MemoryLog::default();
        for (session_id, update) in transcript.updates() {
            log.append(session_id, update.clone());
        }
        let all = log.after(0);
        let full = reduce(
            &rules,
            all.iter()
                .map(|entry| (entry.session_id.as_str(), &entry.update)),
        );
        // A client that applied the first half, then asks for what came after.
        let cut = log.head() / 2;
        let later = log.after(cut);
        assert_eq!(
            later.first().map(|entry| entry.seq),
            (cut < log.head()).then_some(cut + 1),
            "{name}"
        );
        let resumed = reduce(
            &rules,
            all.iter()
                .filter(|entry| entry.seq <= cut)
                .chain(later.iter())
                .map(|entry| (entry.session_id.as_str(), &entry.update)),
        );
        assert_eq!(full, resumed, "{name}: catching up from {cut} differs");
    }
}

#[test]
fn every_update_parses_with_the_official_acp_types() {
    for (name, transcript) in transcripts() {
        for (_, update) in transcript.updates() {
            let kind = update["sessionUpdate"].as_str().unwrap_or_default();
            let typed: acp::SessionUpdate = serde_json::from_value(update.clone())
                .unwrap_or_else(|error| panic!("{name}: `{kind}` does not parse: {error}"));
            let back = serde_json::to_value(&typed).expect("serialises");
            assert_eq!(
                back["sessionUpdate"], update["sessionUpdate"],
                "{name}: `{kind}` changed kind when parsed"
            );
        }
    }
}
