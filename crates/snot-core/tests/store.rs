use serde_json::json;
use snot_core::*;

fn doc(text: &str) -> serde_json::Value {
    json!({"type":"doc","content":[
        {"type":"paragraph","content":[{"type":"text","text":text}]}
    ]})
}

#[test]
fn creates_and_reads_back_a_note() {
    let s = Store::open_in_memory().unwrap();
    let note = s
        .create_note(None, Some(doc("Shopping list\nmilk")))
        .unwrap();
    assert_eq!(note.summary.title, "Shopping list");
    let again = s.get_note(&note.summary.id).unwrap();
    assert_eq!(again.doc, note.doc);
}

#[test]
fn title_follows_the_first_line_until_it_is_overridden() {
    let s = Store::open_in_memory().unwrap();
    let n = s.create_note(None, Some(doc("draft"))).unwrap();
    let id = &n.summary.id;

    let n = s
        .update_note(
            id,
            NotePatch {
                doc: Some(doc("second draft")),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(n.summary.title, "second draft");

    let n = s
        .update_note(
            id,
            NotePatch {
                title: Some("Pinned title".into()),
                doc: Some(doc("body changed again")),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(n.summary.title, "Pinned title");
}

#[test]
fn search_matches_titles_and_bodies_by_prefix() {
    let s = Store::open_in_memory().unwrap();
    s.create_note(None, Some(doc("Roast chicken\nheat the oven to 200C")))
        .unwrap();
    s.create_note(None, Some(doc("Tax return\ndeadline in January")))
        .unwrap();

    assert_eq!(s.search("chick", &Scope::All).unwrap().len(), 1);
    assert_eq!(s.search("oven", &Scope::All).unwrap().len(), 1);
    assert_eq!(s.search("deadl", &Scope::All).unwrap().len(), 1);
    assert!(s.search("zzz", &Scope::All).unwrap().is_empty());
    // Punctuation must not be able to reach FTS5's query syntax.
    assert!(s.search("\"(*)", &Scope::All).is_ok());
}

#[test]
fn a_reindexed_note_stops_matching_its_old_text() {
    let s = Store::open_in_memory().unwrap();
    let n = s.create_note(None, Some(doc("aardvark"))).unwrap();
    s.update_note(
        &n.summary.id,
        NotePatch {
            doc: Some(doc("buffalo")),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(s.search("aardvark", &Scope::All).unwrap().is_empty());
    assert_eq!(s.search("buffalo", &Scope::All).unwrap().len(), 1);
}

#[test]
fn trash_hides_a_note_without_destroying_it() {
    let s = Store::open_in_memory().unwrap();
    let n = s.create_note(None, Some(doc("temporary"))).unwrap();
    s.trash_note(&n.summary.id).unwrap();

    assert!(s
        .list_notes(&Scope::All, SortBy::Updated)
        .unwrap()
        .is_empty());
    assert_eq!(
        s.list_notes(&Scope::Trash, SortBy::Updated).unwrap().len(),
        1
    );
    assert!(s.search("temporary", &Scope::All).unwrap().is_empty());

    s.restore_note(&n.summary.id).unwrap();
    assert_eq!(s.list_notes(&Scope::All, SortBy::Updated).unwrap().len(), 1);
}

#[test]
fn deleting_a_folder_trashes_its_notes_and_its_children() {
    let s = Store::open_in_memory().unwrap();
    let parent = s.create_folder("Work", None).unwrap();
    let child = s.create_folder("2026", Some(&parent.id)).unwrap();
    s.create_note(Some(&child.id), Some(doc("q1 plan")))
        .unwrap();

    s.delete_folder(&parent.id).unwrap();
    assert!(s.list_folders().unwrap().is_empty());
    assert_eq!(
        s.list_notes(&Scope::Trash, SortBy::Updated).unwrap().len(),
        1
    );
}

#[test]
fn a_folder_cannot_be_moved_inside_itself() {
    let s = Store::open_in_memory().unwrap();
    let a = s.create_folder("a", None).unwrap();
    let b = s.create_folder("b", Some(&a.id)).unwrap();
    assert!(s.move_folder(&a.id, Some(&b.id)).is_err());
    assert!(s.move_folder(&a.id, Some(&a.id)).is_err());
}

#[test]
fn tags_are_case_insensitively_unique() {
    let s = Store::open_in_memory().unwrap();
    let a = s.ensure_tag("Work").unwrap();
    let b = s.ensure_tag("work").unwrap();
    assert_eq!(a.id, b.id);

    let n = s.create_note(None, Some(doc("standup"))).unwrap();
    s.update_note(
        &n.summary.id,
        NotePatch {
            tags: Some(vec![a.id.clone()]),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        s.list_notes(&Scope::Tag { id: a.id }, SortBy::Updated)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn pinned_notes_sort_first() {
    let s = Store::open_in_memory().unwrap();
    s.create_note(None, Some(doc("older"))).unwrap();
    let newer = s.create_note(None, Some(doc("newer"))).unwrap();
    let old = s.list_notes(&Scope::All, SortBy::Updated).unwrap();
    assert_eq!(old[0].title, "newer");

    let first = s.create_note(None, Some(doc("pinned one"))).unwrap();
    s.update_note(
        &first.summary.id,
        NotePatch {
            pinned: Some(true),
            ..Default::default()
        },
    )
    .unwrap();
    s.update_note(
        &newer.summary.id,
        NotePatch {
            doc: Some(doc("newer still")),
            ..Default::default()
        },
    )
    .unwrap();
    let listed = s.list_notes(&Scope::All, SortBy::Updated).unwrap();
    assert_eq!(listed[0].title, "pinned one");
}

#[test]
fn identical_attachments_share_one_file() {
    let dir = std::env::temp_dir().join(format!("snot-test-{}", uuid_ish()));
    let s = Store::open(&dir).unwrap();
    let (a, pa) = s
        .put_attachment(None, "a.png", "image/png", b"same-bytes")
        .unwrap();
    let (b, pb) = s
        .put_attachment(None, "b.png", "image/png", b"same-bytes")
        .unwrap();
    assert_ne!(a.id, b.id);
    assert_eq!(pa, pb);
    assert_eq!(s.attachment_path(&a.id).unwrap(), pa);
    std::fs::remove_dir_all(&dir).ok();
}

fn uuid_ish() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}

#[test]
fn markdown_export_round_trips_the_shapes_a_note_actually_uses() {
    let doc = json!({"type":"doc","content":[
        {"type":"heading","attrs":{"level":2},"content":[{"type":"text","text":"Groceries"}]},
        {"type":"taskList","content":[
            {"type":"taskItem","attrs":{"checked":true},
             "content":[{"type":"paragraph","content":[{"type":"text","text":"milk"}]}]},
            {"type":"taskItem","attrs":{"checked":false},
             "content":[{"type":"paragraph","content":[{"type":"text","text":"eggs"}]}]}
        ]},
        {"type":"paragraph","content":[
            {"type":"text","marks":[{"type":"bold"}],"text":"note"},
            {"type":"text","text":" the "},
            {"type":"text","marks":[{"type":"code"}],"text":"price"}
        ]}
    ]});
    let md = doc_to_markdown(&doc);
    assert!(!md.contains("ink"), "page ink is not part of the document");
    assert!(md.contains("## Groceries"));
    assert!(md.contains("- [x] milk"));
    assert!(md.contains("- [ ] eggs"));
    assert!(md.contains("**note** the `price`"));
}

#[test]
fn notes_made_in_the_same_millisecond_still_have_a_stable_order() {
    let s = Store::open_in_memory().unwrap();
    let ids: Vec<String> = (0..24)
        .map(|i| {
            s.create_note(None, Some(doc(&format!("note {i}"))))
                .unwrap()
                .summary
                .id
        })
        .collect();
    let listed: Vec<String> = s
        .list_notes(&Scope::All, SortBy::Updated)
        .unwrap()
        .into_iter()
        .map(|n| n.id)
        .collect();
    let mut expected = ids;
    expected.reverse();
    assert_eq!(
        listed, expected,
        "newest first, with no ties left to chance"
    );
}

#[test]
fn page_ink_round_trips_and_flags_the_summary() {
    let s = Store::open_in_memory().unwrap();
    let n = s.create_note(None, Some(doc("meeting"))).unwrap();
    assert!(!n.summary.has_ink);

    let strokes =
        json!([{ "points": [[1.0, 2.0, 0.5]], "color": "#111", "size": 4, "tool": "pen" }]);
    let n = s
        .update_note(
            &n.summary.id,
            NotePatch {
                ink: Some(strokes.clone()),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(n.ink, strokes);
    assert!(n.summary.has_ink);

    let listed = s.list_notes(&Scope::All, SortBy::Updated).unwrap();
    assert!(listed[0].has_ink);

    // Ink is independent of the text, so editing one must not drop the other.
    let n = s
        .update_note(
            &n.summary.id,
            NotePatch {
                doc: Some(doc("meeting notes")),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(n.ink, strokes);
    assert_eq!(n.summary.title, "meeting notes");
}

#[test]
fn a_note_with_no_text_keeps_the_name_it_was_given() {
    let s = Store::open_in_memory().unwrap();
    let n = s.create_note(None, Some(doc("scratch"))).unwrap();
    let id = n.summary.id;
    s.update_note(
        &id,
        NotePatch {
            title: Some("lease.pdf".into()),
            ..Default::default()
        },
    )
    .unwrap();

    // Saving an empty body — an imported PDF, or a page that is only ink —
    // must not blank the title.
    let empty = json!({"type":"doc","content":[{"type":"paragraph"}]});
    let n = s
        .update_note(
            &id,
            NotePatch {
                doc: Some(empty),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(n.summary.title, "lease.pdf");

    // Typing brings the title back in step with the first line.
    let n = s
        .update_note(
            &id,
            NotePatch {
                doc: Some(doc("Notes on the lease")),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(n.summary.title, "Notes on the lease");
}

#[test]
fn a_new_library_gets_the_demo_examples_exactly_once() {
    let s = Store::open_in_memory().unwrap();
    assert!(s.seed_if_new(true).unwrap(), "a fresh library is seeded");

    let notes = s.list_notes(&Scope::All, SortBy::Updated).unwrap();
    assert!(notes.len() >= 12, "enough to fill a list and scroll it");
    assert!(notes[0].pinned, "the welcome note is pinned to the top");
    assert!(
        s.list_folders().unwrap().len() >= 5,
        "including nested folders"
    );
    assert!(s.list_tags().unwrap().len() >= 5);
    assert!(!s
        .list_notes(&Scope::Trash, SortBy::Updated)
        .unwrap()
        .is_empty());
    assert!(
        s.list_notes(&Scope::Unfiled, SortBy::Updated)
            .unwrap()
            .len()
            >= 2,
        "some notes sit outside any folder"
    );

    // Dates should span more than a day, so the list is not a wall of
    // identical timestamps.
    let newest = notes.iter().map(|n| n.updated_at).max().unwrap();
    let oldest = notes.iter().map(|n| n.updated_at).min().unwrap();
    assert!(
        newest - oldest > 24 * 3_600_000,
        "examples are spread over time"
    );
    assert!(
        notes.iter().any(|n| n.has_ink),
        "one example is handwritten"
    );
    assert!(notes.iter().any(|n| n.favorite));
    assert!(!s.search("sourdough", &Scope::All).unwrap().is_empty());

    // Seeding is once per library, so clearing the examples out keeps them out.
    assert!(!s.seed_if_new(true).unwrap());
    for note in s.list_notes(&Scope::All, SortBy::Updated).unwrap() {
        s.purge_note(&note.id).unwrap();
    }
    assert!(!s.seed_if_new(true).unwrap());
    assert!(s
        .list_notes(&Scope::All, SortBy::Updated)
        .unwrap()
        .is_empty());
}

#[test]
fn a_library_that_already_has_notes_is_never_seeded() {
    let s = Store::open_in_memory().unwrap();
    s.create_note(None, Some(doc("mine"))).unwrap();
    assert!(!s.seed_if_new(false).unwrap());
    assert!(!s.seed_if_new(true).unwrap());
    assert_eq!(s.list_notes(&Scope::All, SortBy::Updated).unwrap().len(), 1);
}

#[test]
fn a_real_first_launch_gets_one_welcome_note_and_nothing_to_tidy_up() {
    let s = Store::open_in_memory().unwrap();
    assert!(s.seed_if_new(false).unwrap());

    let notes = s.list_notes(&Scope::All, SortBy::Updated).unwrap();
    assert_eq!(
        notes.len(),
        1,
        "a real user gets one note, not a fake library"
    );
    assert!(notes[0].title.contains("Welcome"));
    assert!(
        s.list_folders().unwrap().is_empty(),
        "no folders to clean up"
    );
    assert!(s.list_tags().unwrap().is_empty());
    assert!(s
        .list_notes(&Scope::Trash, SortBy::Updated)
        .unwrap()
        .is_empty());
}

/// A note can be nothing but a recording. It still has to be findable, and it
/// still has to say what it is when it leaves as Markdown.
#[test]
fn a_voice_recording_shows_up_in_the_list_in_search_and_in_the_export() {
    let s = Store::open_in_memory().unwrap();
    let doc = json!({"type":"doc","content":[
        {"type":"audio","attrs":{
            "src":"asset://localhost/recording-1.webm",
            "name":"recording-1.webm",
            "attachmentId":"att-1",
            "duration":72.4
        }},
    ]});
    let note = s.create_note(None, Some(doc.clone())).unwrap();

    assert_eq!(note.summary.title, "\u{1f3a4} recording-1.webm");
    let found = s.search("recording", &Scope::All).unwrap();
    assert_eq!(found.len(), 1, "a recording-only note should be searchable");
    assert_eq!(found[0].id, note.summary.id);

    let md = doc_to_markdown(&doc);
    assert!(
        md.contains("(asset://localhost/recording-1.webm)"),
        "the export must link the recording, not drop it: {md}"
    );
    assert!(md.contains("recording-1.webm (1:12)"), "{md}");
    assert!(md.contains("Markdown can link to but not play"), "{md}");
}

/// An unnamed recording still says what it is, in both places.
#[test]
fn an_unnamed_recording_still_names_itself() {
    let doc = json!({"type":"doc","content":[
        {"type":"paragraph","content":[{"type":"text","text":"before"}]},
        {"type":"audio","attrs":{"src":"asset://localhost/a.webm"}},
    ]});
    assert_eq!(doc_to_text(&doc), "before\n\u{1f3a4} voice recording");
    assert!(doc_to_markdown(&doc).contains("[\u{1f3a4} voice recording](asset://localhost/a.webm)"));
}
