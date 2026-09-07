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
    let note = s.create_note(None, Some(doc("Shopping list\nmilk"))).unwrap();
    assert_eq!(note.summary.title, "Shopping list");
    let again = s.get_note(&note.summary.id).unwrap();
    assert_eq!(again.doc, note.doc);
}

#[test]
fn title_follows_the_first_line_until_it_is_overridden() {
    let s = Store::open_in_memory().unwrap();
    let n = s.create_note(None, Some(doc("draft"))).unwrap();
    let id = &n.summary.id;

    let n = s.update_note(id, NotePatch { doc: Some(doc("second draft")), ..Default::default() })
        .unwrap();
    assert_eq!(n.summary.title, "second draft");

    let n = s.update_note(id, NotePatch {
        title: Some("Pinned title".into()),
        doc: Some(doc("body changed again")),
        ..Default::default()
    }).unwrap();
    assert_eq!(n.summary.title, "Pinned title");
}

#[test]
fn search_matches_titles_and_bodies_by_prefix() {
    let s = Store::open_in_memory().unwrap();
    s.create_note(None, Some(doc("Roast chicken\nheat the oven to 200C"))).unwrap();
    s.create_note(None, Some(doc("Tax return\ndeadline in January"))).unwrap();

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
    s.update_note(&n.summary.id, NotePatch { doc: Some(doc("buffalo")), ..Default::default() })
        .unwrap();
    assert!(s.search("aardvark", &Scope::All).unwrap().is_empty());
    assert_eq!(s.search("buffalo", &Scope::All).unwrap().len(), 1);
}

#[test]
fn trash_hides_a_note_without_destroying_it() {
    let s = Store::open_in_memory().unwrap();
    let n = s.create_note(None, Some(doc("temporary"))).unwrap();
    s.trash_note(&n.summary.id).unwrap();

    assert!(s.list_notes(&Scope::All, SortBy::Updated).unwrap().is_empty());
    assert_eq!(s.list_notes(&Scope::Trash, SortBy::Updated).unwrap().len(), 1);
    assert!(s.search("temporary", &Scope::All).unwrap().is_empty());

    s.restore_note(&n.summary.id).unwrap();
    assert_eq!(s.list_notes(&Scope::All, SortBy::Updated).unwrap().len(), 1);
}

#[test]
fn deleting_a_folder_trashes_its_notes_and_its_children() {
    let s = Store::open_in_memory().unwrap();
    let parent = s.create_folder("Work", None).unwrap();
    let child = s.create_folder("2026", Some(&parent.id)).unwrap();
    s.create_note(Some(&child.id), Some(doc("q1 plan"))).unwrap();

    s.delete_folder(&parent.id).unwrap();
    assert!(s.list_folders().unwrap().is_empty());
    assert_eq!(s.list_notes(&Scope::Trash, SortBy::Updated).unwrap().len(), 1);
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
    s.update_note(&n.summary.id, NotePatch { tags: Some(vec![a.id.clone()]), ..Default::default() })
        .unwrap();
    assert_eq!(s.list_notes(&Scope::Tag { id: a.id }, SortBy::Updated).unwrap().len(), 1);
}

#[test]
fn pinned_notes_sort_first() {
    let s = Store::open_in_memory().unwrap();
    s.create_note(None, Some(doc("older"))).unwrap();
    let newer = s.create_note(None, Some(doc("newer"))).unwrap();
    let old = s.list_notes(&Scope::All, SortBy::Updated).unwrap();
    assert_eq!(old[0].title, "newer");

    let first = s.create_note(None, Some(doc("pinned one"))).unwrap();
    s.update_note(&first.summary.id, NotePatch { pinned: Some(true), ..Default::default() })
        .unwrap();
    s.update_note(&newer.summary.id, NotePatch { doc: Some(doc("newer still")), ..Default::default() })
        .unwrap();
    let listed = s.list_notes(&Scope::All, SortBy::Updated).unwrap();
    assert_eq!(listed[0].title, "pinned one");
}

#[test]
fn identical_attachments_share_one_file() {
    let dir = std::env::temp_dir().join(format!("snot-test-{}", uuid_ish()));
    let s = Store::open(&dir).unwrap();
    let (a, pa) = s.put_attachment(None, "a.png", "image/png", b"same-bytes").unwrap();
    let (b, pb) = s.put_attachment(None, "b.png", "image/png", b"same-bytes").unwrap();
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
    assert!(md.contains("## Groceries"));
    assert!(md.contains("- [x] milk"));
    assert!(md.contains("- [ ] eggs"));
    assert!(md.contains("**note** the `price`"));
}
