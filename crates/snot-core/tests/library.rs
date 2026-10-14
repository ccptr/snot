use snot_core::doc::{Block, Content, Doc, Mark, ParaStyle};
use snot_core::editor::{EditSession, Insert};
use snot_core::ink::{Point, Stroke, Tool};
use snot_core::note::HIT_START;
use snot_core::{Library, Scope, SortBy};

fn open() -> (tempfile::TempDir, Library) {
    let dir = tempfile::tempdir().unwrap();
    let lib = Library::open(dir.path()).unwrap();
    (dir, lib)
}

fn write(lib: &Library, title: &str, lines: &[&str]) -> String {
    let note = lib.create_note(None).unwrap();
    let doc = Doc {
        blocks: lines.iter().map(|l| Block::text(*l)).collect(),
    };
    lib.save_content(&note.id, title, &doc).unwrap();
    note.id
}

#[test]
fn a_new_library_opens_on_the_welcome_note() {
    let (dir, lib) = open();
    let notes = lib.notes(&Scope::All, SortBy::Modified).unwrap();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].title, "Welcome to Snot");
    assert!(notes[0].has_ink);
    let welcome = lib.note(&notes[0].id).unwrap();
    let first = welcome.doc.blocks[0].as_text().unwrap();
    let bold: Vec<String> = first
        .runs()
        .iter()
        .filter(|r| r.bold)
        .map(|r| {
            first
                .text
                .chars()
                .skip(r.start as usize)
                .take((r.end - r.start) as usize)
                .collect()
        })
        .collect();
    assert_eq!(bold, ["the same page"]);
    drop(lib);
    // Reopening does not seed it again.
    let lib = Library::open(dir.path()).unwrap();
    assert_eq!(lib.notes(&Scope::All, SortBy::Modified).unwrap().len(), 1);
}

#[test]
fn notes_round_trip_through_the_database() {
    let (_d, lib) = open();
    let note = lib.create_note(None).unwrap();
    let mut s = EditSession::new(note);
    let first = s.doc().blocks[0].id.clone();
    s.set_text(&first, "Shopping for the weekend", None);
    s.toggle_mark(&first, 0..8, &Mark::Bold);
    let next = s.split(&first, 24).unwrap();
    s.set_style(&next.block, ParaStyle::Checklist);
    s.set_text(&next.block, "apples", None);
    s.toggle_checked(&next.block);
    let ink = s.insert(Some(&next.block), Insert::Ink);
    s.add_stroke(
        &ink,
        Stroke::new(
            Tool::Pen,
            "red",
            3.0,
            &[
                Point {
                    x: 1.0,
                    y: 2.0,
                    pressure: 0.3,
                },
                Point {
                    x: 50.0,
                    y: 60.0,
                    pressure: 0.9,
                },
            ],
        ),
    );
    lib.save_content(&s.note().id, s.title(), s.doc()).unwrap();

    let back = lib.note(&s.note().id).unwrap();
    assert_eq!(back.doc.blocks.len(), s.doc().blocks.len());
    assert_eq!(back.doc.blocks[0], s.doc().blocks[0]);
    assert_eq!(back.doc.blocks[1], s.doc().blocks[1]);
    let Content::Ink(ink) = &back.doc.blocks[2].content else {
        panic!("not ink")
    };
    assert_eq!(ink.strokes[0].points.len(), 2);
    let summary = lib
        .notes(&Scope::All, SortBy::Modified)
        .unwrap()
        .into_iter()
        .find(|n| n.id == back.id)
        .unwrap();
    assert_eq!(summary.title, "Shopping for the weekend");
    assert_eq!(summary.preview, "apples");
    assert!(summary.has_ink);
}

#[test]
fn saving_unchanged_content_keeps_the_modified_time() {
    let (_d, lib) = open();
    let id = write(&lib, "", &["hello"]);
    let note = lib.note(&id).unwrap();
    assert_eq!(lib.save_content(&id, &note.title, &note.doc).unwrap(), None);
    assert_eq!(lib.note(&id).unwrap().updated_at, note.updated_at);
}

#[test]
fn search_finds_words_prefixes_and_substrings() {
    let (_d, lib) = open();
    let a = write(&lib, "Trip", &["Book the ferry to Jeju", "pack sunscreen"]);
    let b = write(&lib, "", &["메모 앱을 만들었습니다"]);
    let c = write(&lib, "Footnotes", &["nothing else"]);

    let hits = lib.search("ferr").unwrap();
    assert_eq!(hits.iter().map(|h| &h.id).collect::<Vec<_>>(), [&a]);
    assert!(hits[0].snippet.as_ref().unwrap().contains(HIT_START));

    // Both words must appear, in any order.
    assert_eq!(lib.search("sunscreen book").unwrap().len(), 1);
    assert!(lib.search("sunscreen trains").unwrap().is_empty());

    // Korean attaches particles to words; a substring still matches.
    let hits = lib.search("메모").unwrap();
    assert_eq!(hits[0].id, b);
    let hits = lib.search("앱").unwrap();
    assert_eq!(hits[0].id, b);
    assert!(hits[0].snippet.as_ref().unwrap().contains(HIT_START));

    // Inside a word, not just at its start.
    assert!(lib.search("note").unwrap().iter().any(|h| h.id == c));

    // Punctuation the full-text syntax would choke on is harmless.
    assert!(lib.search("\"( *").unwrap().is_empty());

    lib.trash_note(&a).unwrap();
    assert!(lib.search("ferry").unwrap().is_empty());
}

#[test]
fn folders_trash_and_restore() {
    let (_d, lib) = open();
    let work = lib.create_folder("Work", None).unwrap();
    let sub = lib.create_folder("Meetings", Some(&work.id)).unwrap();
    let a = write(&lib, "a", &["x"]);
    let b = write(&lib, "b", &["y"]);
    lib.move_note(&a, Some(&work.id)).unwrap();
    lib.move_note(&b, Some(&sub.id)).unwrap();
    assert_eq!(
        lib.notes(&Scope::Folder(work.id.clone()), SortBy::Modified)
            .unwrap()
            .len(),
        1
    );
    let folders = lib.folders().unwrap();
    assert_eq!(
        folders.iter().find(|f| f.id == work.id).unwrap().note_count,
        1
    );

    lib.delete_folder(&work.id).unwrap();
    assert!(lib.folders().unwrap().is_empty());
    let counts = lib.counts().unwrap();
    assert_eq!(counts.trash, 2);
    lib.restore_note(&b).unwrap();
    let back = lib.note(&b).unwrap();
    assert_eq!(back.folder_id, None);
    assert_eq!(back.trashed_at, None);

    assert_eq!(lib.empty_trash().unwrap(), 1);
    assert!(lib.note(&a).is_err());
}

#[test]
fn pinned_notes_lead_and_favourites_filter() {
    let (_d, lib) = open();
    let old = write(&lib, "old", &["1"]);
    let _new = write(&lib, "new", &["2"]);
    lib.set_pinned(&old, true).unwrap();
    lib.set_favorite(&old, true).unwrap();
    let all = lib.notes(&Scope::All, SortBy::Modified).unwrap();
    assert_eq!(all[0].id, old);
    let favs = lib.notes(&Scope::Favorites, SortBy::Modified).unwrap();
    assert_eq!(favs.len(), 1);
    let by_title = lib.notes(&Scope::All, SortBy::Title).unwrap();
    assert_eq!(by_title[1].title, "new");
}

#[test]
fn attachments_are_shared_and_collected() {
    let (_d, lib) = open();
    let png = [0x89, b'P', b'N', b'G', 1, 2, 3];
    let one = lib.add_attachment(&png, "image/png", "a.png").unwrap();
    let two = lib.add_attachment(&png, "image/png", "b.png").unwrap();
    assert_eq!(one.id, two.id);
    let path = lib.attachment_path(&one.id);
    assert!(path.exists());

    let note = lib.create_note(None).unwrap();
    let mut s = EditSession::new(note);
    s.insert(
        None,
        Insert::Image {
            attachment: one.id.clone(),
            aspect: 1.0,
        },
    );
    lib.save_content(&s.note().id, "", s.doc()).unwrap();
    assert_eq!(lib.collect_garbage().unwrap(), 0);

    lib.trash_note(&s.note().id).unwrap();
    // Still referenced from the trash: it must survive until the trash goes.
    assert_eq!(lib.collect_garbage().unwrap(), 0);
    lib.empty_trash().unwrap();
    assert!(!path.exists());
}

#[test]
fn markdown_export_and_import_round_trip() {
    let (dir, lib) = open();
    let note = lib.create_note(None).unwrap();
    let mut s = EditSession::new(note);
    s.set_title("Plans");
    let b0 = s.doc().blocks[0].id.clone();
    s.set_text(&b0, "Do it now", None);
    s.toggle_mark(&b0, 3..5, &Mark::Bold);
    let b1 = s.split(&b0, 9).unwrap().block;
    s.set_style(&b1, ParaStyle::Checklist);
    s.set_text(&b1, "first", None);
    s.toggle_checked(&b1);
    let ink = s.insert(Some(&b1), Insert::Ink);
    s.add_stroke(
        &ink,
        Stroke::new(
            Tool::Pen,
            "ink",
            3.0,
            &[
                Point {
                    x: 1.0,
                    y: 2.0,
                    pressure: 0.5,
                },
                Point {
                    x: 30.0,
                    y: 40.0,
                    pressure: 0.5,
                },
            ],
        ),
    );
    lib.save_content(&s.note().id, s.title(), s.doc()).unwrap();

    let out = dir.path().join("export");
    let md_path = lib.export_markdown(&s.note().id, &out).unwrap();
    let md = std::fs::read_to_string(&md_path).unwrap();
    assert!(md.starts_with("# Plans\n"), "{md}");
    assert!(md.contains("Do **it** now"));
    assert!(md.contains("- [x] first"));
    assert!(md.contains("![drawing](Plans.assets/drawing-"));
    assert!(out.join("Plans.assets").read_dir().unwrap().count() == 1);

    let back = lib.import_markdown(&md_path, None).unwrap();
    assert_eq!(back.title, "Plans");
    let t = back.doc.blocks[0].as_text().unwrap();
    assert_eq!(t.text, "Do it now");
    assert_eq!(t.spans[0].mark, Mark::Bold);
    // The drawing comes back as a picture of itself only if SVG were
    // accepted; it is not, so it must at least leave a visible trace.
    assert!(back.doc.plain_text().contains("drawing"));
}

#[test]
fn a_newer_schema_is_refused_not_clobbered() {
    let dir = tempfile::tempdir().unwrap();
    drop(Library::open(dir.path()).unwrap());
    let conn = rusqlite_conn(dir.path());
    conn.pragma_update(None, "user_version", 99).unwrap();
    drop(conn);
    assert!(Library::open(dir.path()).is_err());
}

fn rusqlite_conn(dir: &std::path::Path) -> rusqlite::Connection {
    rusqlite::Connection::open(dir.join("snot.db")).unwrap()
}
