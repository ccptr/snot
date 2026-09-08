use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::Value;
use snot_core::*;

/// Every construct `doc_to_markdown` can write, one document per construct.
/// Each is already in the exporter's own normal form, so importing and
/// re-exporting has to give back exactly the same bytes.
const ROUND_TRIP: &[&str] = &[
    "# Title\n\nSome **bold**, *italic*, ~~struck~~ and `code` text with ==highlight==.\n",
    "## Second level\n\n### Third level\n",
    "- one\n- two\n  - nested\n",
    "1. first\n2. second\n",
    "- [ ] todo\n- [x] done\n",
    "> quoted line\n",
    "```rust\nfn main() {}\n```\n",
    "---\n",
    "![diagram](images/a.png)\n",
    "line one  \nline two\n",
    "**==both==**\n",
    "A paragraph.\n\nAnother paragraph, with a [^1] footnote reference left as text.\n",
];

#[test]
fn markdown_round_trips_every_construct_the_exporter_writes() {
    for source in ROUND_TRIP {
        let back = doc_to_markdown(&markdown_to_doc(source));
        assert_eq!(&back.as_str(), source, "round trip changed the document");
    }
}

/// Markdown that came from somewhere else entirely. Nothing here has to
/// survive as the same syntax — but nothing may be lost, and nothing may
/// panic on the way in.
#[test]
fn third_party_markdown_keeps_its_text() {
    let source = "\
Setext Title
============

Sub
---

Some [reference][ref] link and an ![img][iref].

<div class=\"note\">
  <p>raw html</p>
</div>

- item
  1. nested ordered
  2. another
- [ ] mixed task in a bullet list

| a | b |
|---|---|
| 1 | 2 |

    indented code

[ref]: https://example.com
[iref]: pic.png
";
    let doc = markdown_to_doc(source);
    let text = doc_to_text(&doc);
    for wanted in [
        "Setext Title",
        "Sub",
        "reference",
        "raw html",
        "item",
        "nested ordered",
        "another",
        "mixed task in a bullet list",
        "indented code",
    ] {
        assert!(text.contains(wanted), "lost {wanted:?} from:\n{text}");
    }
    // The table has no node to become, so its cells arrive as ordinary text.
    for cell in ["a", "b", "1", "2"] {
        assert!(text.contains(cell), "lost table cell {cell:?}");
    }
    // Re-exporting has to stay valid Markdown rather than panicking.
    assert!(doc_to_markdown(&doc).contains("raw html"));
}

#[test]
fn a_heading_deeper_than_the_editor_has_is_clamped_rather_than_stored() {
    let doc = markdown_to_doc("###### deep\n");
    assert_eq!(doc["content"][0]["attrs"]["level"], 3);
}

#[test]
fn an_unpaired_highlight_marker_stays_literal() {
    let doc = markdown_to_doc("two == three\n");
    assert_eq!(doc_to_markdown(&doc), "two == three\n");
}

// ------------------------------------------------------------ markdown trees

#[test]
fn a_markdown_directory_becomes_folders_and_notes() {
    let src = scratch("md-tree");
    write(&src.join("Welcome.md"), "# Hello\n\nthe body\n");
    write(
        &src.join("Work/Standup.md"),
        "---\ntitle: Monday standup\ntags: [work, meetings]\ncreated: 2021-03-04\n---\n\nwhat I did\n",
    );
    write(&src.join("Notes/Plain.txt"), "just text\nsecond line\n");
    // A dot-directory is the source app's bookkeeping, not the user's notes.
    write(&src.join(".obsidian/workspace.md"), "# nope\n");
    // A directory of nothing but pictures is not a folder of notes.
    write_bytes(&src.join("images/pic.png"), b"not really a png");

    let (store, _lib) = library("md-tree");
    let summary = import_markdown_dir(&store, &src, None).unwrap();
    assert_eq!(summary.notes, 3);
    assert_eq!(summary.folders, 2);
    assert!(summary.failures.is_empty());

    let folders = store.list_folders().unwrap();
    let mut names: Vec<&str> = folders.iter().map(|f| f.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, ["Notes", "Work"]);

    let notes = store.list_notes(&Scope::All, SortBy::Title).unwrap();
    let titles: Vec<&str> = notes.iter().map(|n| n.title.as_str()).collect();
    assert!(titles.contains(&"Hello"), "got {titles:?}");
    assert!(titles.contains(&"Monday standup"), "got {titles:?}");
    assert!(titles.contains(&"Plain"), "got {titles:?}");

    let standup = notes.iter().find(|n| n.title == "Monday standup").unwrap();
    assert_eq!(standup.tags.len(), 2);
    // 2021-03-04 in milliseconds.
    assert_eq!(standup.created_at, 1_614_816_000_000);

    // The leading `# Hello` became the title rather than staying in the body.
    let welcome = store
        .get_note(&notes.iter().find(|n| n.title == "Hello").unwrap().id)
        .unwrap();
    assert_eq!(doc_to_markdown(&welcome.doc), "the body\n");

    std::fs::remove_dir_all(&src).ok();
}

#[test]
fn an_exported_note_comes_back_without_growing_a_second_title() {
    let src = scratch("md-round");
    // Exactly what `export_markdown` writes: the title as an H1, then the body.
    write(&src.join("Lease.md"), "# Lease\n\nsigned on Tuesday\n");

    let (store, _lib) = library("md-round");
    import_markdown_dir(&store, &src, None).unwrap();
    let notes = store.list_notes(&Scope::All, SortBy::Title).unwrap();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].title, "Lease");
    let note = store.get_note(&notes[0].id).unwrap();
    assert_eq!(doc_to_markdown(&note.doc), "signed on Tuesday\n");

    std::fs::remove_dir_all(&src).ok();
}

#[test]
fn relative_images_are_stored_and_broken_ones_are_left_alone() {
    let src = scratch("md-images");
    write_bytes(&src.join("art/pic.png"), b"pretend png");
    write(
        &src.join("Sketch.md"),
        "![there](art/pic.png)\n\n![gone](art/missing.png)\n\n![web](https://example.com/x.png)\n",
    );

    let (store, lib) = library("md-images");
    let summary = import_markdown_dir(&store, &src, None).unwrap();
    assert_eq!(summary.attachments, 1);

    let notes = store.list_notes(&Scope::All, SortBy::Updated).unwrap();
    let note = store.get_note(&notes[0].id).unwrap();
    let sources = image_sources(&note.doc);
    assert!(
        sources[0].starts_with(lib.to_string_lossy().as_ref()),
        "expected a stored attachment, got {:?}",
        sources[0]
    );
    assert!(Path::new(&sources[0]).exists());
    // A link that does not resolve is better left readable than turned into
    // an attachment that is not there.
    assert_eq!(sources[1], "art/missing.png");
    assert_eq!(sources[2], "https://example.com/x.png");

    std::fs::remove_dir_all(&src).ok();
}

// -------------------------------------------------------------- samsung notes

#[test]
fn a_directory_of_exported_pdfs_becomes_annotatable_notes() {
    let src = scratch("samsung-pdf");
    write_bytes(&src.join("Recipe.pdf"), b"%PDF-1.4 pretend");

    let (store, _lib) = library("samsung-pdf");
    let summary = import_samsung_dir(&store, &src, None).unwrap();
    assert_eq!(summary.notes, 1);
    assert_eq!(summary.unconverted, 0);

    let notes = store.list_notes(&Scope::All, SortBy::Updated).unwrap();
    assert_eq!(notes[0].title, "Recipe");
    assert!(notes[0].has_background);
    let note = store.get_note(&notes[0].id).unwrap();
    assert_eq!(note.background["kind"], "pdf");

    std::fs::remove_dir_all(&src).ok();
}

#[test]
fn an_sdocx_gives_up_the_text_and_pictures_it_can() {
    let src = scratch("samsung-sdocx");
    // Roughly the shape of the real thing: binary fields with length-prefixed
    // UTF-16LE strings among them, one of which lands on an odd byte.
    let mut body = Vec::new();
    body.extend_from_slice(&[0x00, 0x00, 0x01, 0x00]);
    body.extend(utf16le("Shopping list"));
    body.extend_from_slice(&[0x00, 0x00, 0x0e, 0x00, 0x00]);
    body.extend(utf16le("milk and bread"));
    write_bytes(
        &src.join("Groceries.sdocx"),
        &zip_of(&[
            ("note.note", body.as_slice()),
            ("pageIdInfo.dat", &[0u8; 8]),
            ("media/photo.png", b"pretend png"),
        ]),
    );

    let (store, _lib) = library("samsung-sdocx");
    let summary = import_samsung_dir(&store, &src, None).unwrap();
    assert_eq!(summary.notes, 1);
    assert_eq!(summary.unconverted, 0);
    // The picture, plus the original container kept beside it.
    assert_eq!(summary.attachments, 2);

    let notes = store.list_notes(&Scope::All, SortBy::Updated).unwrap();
    assert_eq!(notes[0].title, "Groceries");
    let note = store.get_note(&notes[0].id).unwrap();
    let text = doc_to_text(&note.doc);
    assert!(text.contains("Shopping list"), "got {text:?}");
    assert!(text.contains("milk and bread"), "got {text:?}");
    assert_eq!(image_sources(&note.doc).len(), 1);

    std::fs::remove_dir_all(&src).ok();
}

#[test]
fn an_snb_gives_up_the_text_in_its_xml() {
    let src = scratch("samsung-snb");
    let xml = "<?xml version=\"1.0\"?><sn:snote xmlns:sn=\"x\">\
        <sn:l><sn:r><sn:t>first line</sn:t></sn:r></sn:l>\
        <sn:l><sn:r><sn:t>second &amp; last</sn:t></sn:r></sn:l></sn:snote>";
    write_bytes(
        &src.join("Old.snb"),
        &zip_of(&[("snote/snote.xml", xml.as_bytes())]),
    );

    let (store, _lib) = library("samsung-snb");
    import_samsung_dir(&store, &src, None).unwrap();
    let notes = store.list_notes(&Scope::All, SortBy::Updated).unwrap();
    let note = store.get_note(&notes[0].id).unwrap();
    let text = doc_to_text(&note.doc);
    assert!(text.contains("first line"), "got {text:?}");
    assert!(text.contains("second & last"), "got {text:?}");

    std::fs::remove_dir_all(&src).ok();
}

#[test]
fn a_file_that_cannot_be_read_still_reaches_the_library() {
    let src = scratch("samsung-opaque");
    write_bytes(&src.join("Locked.sdocx"), b"not a zip at all");
    write_bytes(&src.join("Voice.m4a"), b"pretend audio");

    let (store, _lib) = library("samsung-opaque");
    let summary = import_samsung_dir(&store, &src, None).unwrap();
    assert_eq!(summary.notes, 2);
    assert_eq!(summary.unconverted, 2);
    assert!(summary.failures.is_empty());

    let notes = store.list_notes(&Scope::All, SortBy::Title).unwrap();
    let titles: Vec<&str> = notes.iter().map(|n| n.title.as_str()).collect();
    assert!(titles.contains(&"Locked"), "got {titles:?}");
    assert!(titles.contains(&"Voice"), "got {titles:?}");
    // Every one of them carries the file it could not read.
    for note in &notes {
        let body = doc_to_text(&store.get_note(&note.id).unwrap().doc);
        assert!(body.contains("could not read"), "got {body:?}");
    }

    std::fs::remove_dir_all(&src).ok();
}

// ------------------------------------------------------------------- helpers

fn unique() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}

fn scratch(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("snot-{label}-{}", unique()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn library(label: &str) -> (Store, PathBuf) {
    let dir = scratch(&format!("lib-{label}"));
    let store = Store::open(&dir).unwrap();
    (store, dir)
}

fn write(path: &Path, contents: &str) {
    write_bytes(path, contents.as_bytes());
}

fn write_bytes(path: &Path, contents: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

fn utf16le(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, bytes) in entries {
        writer.start_file(*name, options).unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn image_sources(doc: &Value) -> Vec<String> {
    let mut found = vec![];
    let mut frontier = vec![doc];
    while let Some(node) = frontier.pop() {
        if node.get("type").and_then(Value::as_str) == Some("image") {
            found.push(node["attrs"]["src"].as_str().unwrap_or("").to_string());
        }
        if let Some(children) = node.get("content").and_then(Value::as_array) {
            frontier.extend(children.iter().rev());
        }
    }
    found
}
