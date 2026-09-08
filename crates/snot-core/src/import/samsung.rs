//! Samsung Notes exports.
//!
//! What is verified about the formats, from published reverse-engineering
//! work and from two independent open parsers:
//!
//! * A `.sdocx` is a ZIP holding `note.note` (document metadata and the
//!   flowing rich text), `pageIdInfo.dat` (the page order), one
//!   `<uuid>.page` per page (layers and a recursive tree of stroke, image,
//!   text-box and shape objects), `media/` (PNG, JPEG, PDF, audio) and
//!   `end_tag.bin`. Every one of those members is little-endian **binary** —
//!   the widely copied claim that a `.sdocx` contains an XML file and a Media
//!   folder is simply wrong. Strings inside them are a length followed by
//!   UTF-16LE code units, and the object records are laid out by property
//!   bitmasks rather than at fixed offsets.
//! * The Windows Samsung Notes app keeps unexported documents as a plain
//!   directory with those same members as loose files, so a directory that
//!   contains a `note.note` is read here as one document.
//! * A `.snb` — the older S Note format, and unrelated to `.sdocx` despite
//!   the family resemblance — is a ZIP of genuine XML: `snote/snote.xml`
//!   with namespaced `sn:l` lines and `sn:t` text runs, `snote/styles.xml`,
//!   and images alongside.
//! * Samsung Notes can also export a note as PDF, and a directory of those
//!   is the export people most often actually have.
//!
//! What is best-effort here, and honestly so: this reads the media and the
//! XML, and for the binary members it sweeps for runs of printable UTF-16LE
//! and keeps them as text. That recovers typed words. It does **not** decode
//! the object frames, so handwriting, shapes and text layout do not survive,
//! and the sweep can pick up a stray internal string. Anything that yields
//! nothing at all is kept whole as an attachment instead, so no export file
//! is ever lost on the way in.

use std::io::{Cursor, Read};
use std::path::Path;

use serde_json::{json, Value};

use crate::error::Result;
use crate::model::NotePatch;
use crate::store::Store;

use super::{
    attach_unconverted, import_pdf_file, is_image, mime_for, read_dir_sorted, text_to_doc,
    ImportSummary, MAX_DEPTH,
};

/// Container extensions worth opening as a ZIP.
const CONTAINERS: [&str; 4] = ["sdocx", "sdoc", "snb", "spd"];

/// A single member of an export, however it was stored.
struct Member {
    name: String,
    bytes: Vec<u8>,
}

/// Refuses to inflate a single member past this. An export is a note, not a
/// disk image, and a ZIP that claims otherwise is not one we want to trust.
const MAX_MEMBER: u64 = 128 * 1024 * 1024;

/// Imports a directory of Samsung Notes exports.
pub fn import_samsung_dir(
    store: &Store,
    dir: &Path,
    parent_folder: Option<&str>,
) -> Result<ImportSummary> {
    let mut summary = ImportSummary::default();
    walk(store, dir, parent_folder, 0, &mut summary)?;
    Ok(summary)
}

fn walk(
    store: &Store,
    dir: &Path,
    folder: Option<&str>,
    depth: usize,
    summary: &mut ImportSummary,
) -> Result<()> {
    if depth > MAX_DEPTH {
        return Ok(());
    }
    // A directory holding a `note.note` is not a folder of notes: it is one
    // note, unpacked.
    if dir.join("note.note").is_file() {
        let members = read_loose(dir)?;
        let title = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Samsung note".into());
        if let Err(err) = convert(store, &members, &title, None, folder, summary) {
            summary.fail(dir, err.to_string());
        }
        return Ok(());
    }

    for entry in read_dir_sorted(dir)? {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            let child = store.create_folder(&name, folder)?;
            summary.folders += 1;
            walk(store, &path, Some(&child.id), depth + 1, summary)?;
        } else if let Err(err) = import_file(store, &path, folder, summary) {
            summary.fail(&path, err.to_string());
        }
    }
    Ok(())
}

fn import_file(
    store: &Store,
    path: &Path,
    folder: Option<&str>,
    summary: &mut ImportSummary,
) -> Result<()> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        // The PDF export is the one Samsung Notes always offers, so it is the
        // one most exports are made of. It comes in as a page to annotate.
        "pdf" => import_pdf_file(store, path, folder, summary),
        "txt" => import_plain(store, path, folder, summary),
        ext if CONTAINERS.contains(&ext) => {
            let bytes = std::fs::read(path)?;
            let title = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Samsung note".into());
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "note.sdocx".into());
            match read_zip(&bytes) {
                Ok(members) if !members.is_empty() => convert(
                    store,
                    &members,
                    &title,
                    Some((&name, &bytes)),
                    folder,
                    summary,
                ),
                // Not a ZIP after all, or one we cannot walk. The file itself
                // is still the user's note, so it comes in whole.
                _ => attach_unconverted(store, path, folder, summary),
            }
        }
        _ => attach_unconverted(store, path, folder, summary),
    }
}

fn import_plain(
    store: &Store,
    path: &Path,
    folder: Option<&str>,
    summary: &mut ImportSummary,
) -> Result<()> {
    let raw = std::fs::read(path)?;
    let text = String::from_utf8_lossy(&raw);
    let title = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Samsung note".into());
    let note = store.create_note(folder, None)?;
    store.update_note(
        &note.summary.id,
        NotePatch {
            title: Some(title),
            doc: Some(text_to_doc(text.strip_prefix('\u{feff}').unwrap_or(&text))),
            ..Default::default()
        },
    )?;
    summary.notes += 1;
    Ok(())
}

// ------------------------------------------------------------------ members

fn read_zip(bytes: &[u8]) -> std::result::Result<Vec<Member>, zip::result::ZipError> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;
    let mut members = vec![];
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        if !entry.is_file() || entry.size() > MAX_MEMBER {
            continue;
        }
        let name = entry.name().to_string();
        let mut buf = Vec::with_capacity(entry.size() as usize);
        if entry.read_to_end(&mut buf).is_ok() {
            members.push(Member { name, bytes: buf });
        }
    }
    Ok(members)
}

/// Reads an unpacked document — the shape the Windows app leaves on disk.
fn read_loose(dir: &Path) -> Result<Vec<Member>> {
    let mut members = vec![];
    let mut frontier = vec![(dir.to_path_buf(), String::new())];
    while let Some((path, prefix)) = frontier.pop() {
        for entry in read_dir_sorted(&path)? {
            let name = entry.file_name().to_string_lossy().into_owned();
            let full = format!("{prefix}{name}");
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                frontier.push((entry.path(), format!("{full}/")));
            } else if kind.is_file() {
                if entry.metadata().map(|m| m.len()).unwrap_or(0) > MAX_MEMBER {
                    continue;
                }
                members.push(Member {
                    name: full,
                    bytes: std::fs::read(entry.path())?,
                });
            }
        }
    }
    Ok(members)
}

// --------------------------------------------------------------- conversion

fn convert(
    store: &Store,
    members: &[Member],
    title: &str,
    original: Option<(&str, &[u8])>,
    folder: Option<&str>,
    summary: &mut ImportSummary,
) -> Result<()> {
    let mut blocks: Vec<Value> = vec![];
    let mut images: Vec<&Member> = vec![];
    let mut background: Option<&Member> = None;

    for member in members {
        let mime = mime_for(&member.name);
        let lower = member.name.to_ascii_lowercase();
        if is_image(mime) {
            images.push(member);
        } else if mime == "application/pdf" {
            // Samsung Notes can hold an imported PDF inside a note; that is
            // the page the writing was on, so it becomes ours too.
            background.get_or_insert(member);
        } else if lower.ends_with(".xml") || lower.ends_with(".html") || lower.ends_with(".htm") {
            blocks.extend(markup_to_blocks(&String::from_utf8_lossy(&member.bytes)));
        } else if lower.ends_with(".txt") {
            let text = String::from_utf8_lossy(&member.bytes);
            if let Some(content) = text_to_doc(&text)["content"].as_array() {
                blocks.extend(content.iter().cloned());
            }
        } else if lower.ends_with("note.note") || lower.ends_with(".page") {
            blocks.extend(scavenge_utf16(&member.bytes).into_iter().map(|line| {
                json!({
                    "type": "paragraph",
                    "content": [{"type": "text", "text": line}],
                })
            }));
        }
    }

    let recovered = !blocks.is_empty() || !images.is_empty() || background.is_some();
    let note = store.create_note(folder, None)?;
    let id = note.summary.id;

    for image in &images {
        let name = image.name.rsplit('/').next().unwrap_or(&image.name);
        let (_, path) = store.put_attachment(Some(&id), name, mime_for(name), &image.bytes)?;
        blocks.push(json!({
            "type": "image",
            "attrs": {"src": path.to_string_lossy(), "alt": name},
        }));
        summary.attachments += 1;
    }

    let mut patch = NotePatch {
        title: Some(title.to_string()),
        ..Default::default()
    };
    if let Some(pdf) = background {
        let name = pdf.name.rsplit('/').next().unwrap_or(&pdf.name).to_string();
        let (attachment, _) =
            store.put_attachment(Some(&id), &name, "application/pdf", &pdf.bytes)?;
        patch.background = Some(json!({
            "kind": "pdf",
            "attachmentId": attachment.id,
            "name": name,
        }));
        summary.attachments += 1;
    }

    // The original goes in beside whatever was recovered from it. Conversion
    // is partial by construction — handwriting and layout do not survive —
    // so the file the user exported stays in the library either way. An
    // unpacked document has no single file to keep, and its directory is
    // still sitting where the user left it.
    if let Some((name, bytes)) = original {
        store.put_attachment(Some(&id), name, mime_for(name), bytes)?;
        summary.attachments += 1;
    }

    if !recovered {
        let name = original.map_or(title, |(name, _)| name);
        blocks.push(json!({
            "type": "paragraph",
            "content": [{
                "type": "text",
                "text": format!(
                    "Snot could not read anything out of {name}. The original \
                     is kept with this note in the library's attachments folder."
                ),
            }],
        }));
        summary.unconverted += 1;
    }
    patch.doc = Some(json!({"type": "doc", "content": blocks}));
    store.update_note(&id, patch)?;
    summary.notes += 1;
    Ok(())
}

/// Sweeps a binary member for runs of printable UTF-16LE.
///
/// Samsung writes note text as UTF-16LE code units inside records whose
/// layout is driven by property bitmasks. Walking those records properly is a
/// large piece of work against a format with no published specification, so
/// this instead takes the text it can positively recognise and leaves the
/// rest. It finds typed words; it does not find their formatting, and it can
/// surface the odd internal string.
///
/// A string can start at an odd byte as easily as an even one, so both
/// alignments are read. The wrong alignment over ASCII text produces code
/// units whose low byte is always zero — readable-looking CJK that is really
/// a misread — and those are thrown out before anything is believed.
fn scavenge_utf16(bytes: &[u8]) -> Vec<String> {
    let mut kept = runs_at(bytes, 0);
    for run in runs_at(bytes, 1) {
        // Where both alignments claim the same bytes, only one of them can be
        // telling the truth, and the even one is the likelier.
        if !kept.iter().any(|(s, e, _)| run.0 < *e && *s < run.1) {
            kept.push(run);
        }
    }
    kept.sort_by_key(|(start, _, _)| *start);
    let mut out: Vec<String> = vec![];
    for (_, _, text) in kept {
        if out.last() != Some(&text) {
            out.push(text);
        }
    }
    out
}

/// Runs of printable text read from one byte alignment, each with the range
/// of the file it came from.
fn runs_at(bytes: &[u8], offset: usize) -> Vec<(usize, usize, String)> {
    let mut out = vec![];
    let mut run: Vec<u16> = vec![];
    let mut start = 0usize;
    let mut flush = |run: &mut Vec<u16>, start: usize, end: usize| {
        let believable = run.len() >= 4 && !run.iter().all(|unit| unit & 0x00ff == 0);
        if believable {
            let text = String::from_utf16_lossy(run).trim().to_string();
            if text.chars().any(char::is_alphanumeric) {
                out.push((offset + start * 2, offset + end * 2, text));
            }
        }
        run.clear();
    };
    for (i, pair) in bytes[offset.min(bytes.len())..]
        .as_chunks::<2>()
        .0
        .iter()
        .enumerate()
    {
        let unit = u16::from_le_bytes(*pair);
        // Keep the printable plane and the surrogates that pair up into it;
        // anything else ends the run.
        let printable = match char::from_u32(unit as u32) {
            Some(c) => !c.is_control() || c == '\t',
            // A lone surrogate is half of a character above the BMP, which
            // `from_u32` refuses; the run carries on and lossy decoding
            // sorts it out at the end.
            None => (0xd800..0xe000).contains(&unit),
        };
        if printable {
            if run.is_empty() {
                start = i;
            }
            run.push(unit);
        } else {
            flush(&mut run, start, i);
        }
    }
    let end = bytes[offset.min(bytes.len())..].len() / 2;
    flush(&mut run, start, end);
    out
}

// ------------------------------------------------------------ markup to text

/// Turns XML or HTML into paragraphs and headings by taking the text and the
/// places it breaks. The editor's schema has no tags to keep, so this keeps
/// what a reader would see and drops the rest.
fn markup_to_blocks(src: &str) -> Vec<Value> {
    let mut blocks = vec![];
    let mut buf = String::new();
    let mut heading: Option<u64> = None;
    let mut skipping: Option<String> = None;
    let bytes = src.as_bytes();
    let mut i = 0;

    let mut flush = |buf: &mut String, heading: &mut Option<u64>| {
        let text = decode_entities(buf).trim().to_string();
        buf.clear();
        if text.is_empty() {
            *heading = None;
            return;
        }
        blocks.push(match heading.take() {
            Some(level) => json!({
                "type": "heading",
                "attrs": {"level": level.clamp(1, 3)},
                "content": [{"type": "text", "text": text}],
            }),
            None => json!({
                "type": "paragraph",
                "content": [{"type": "text", "text": text}],
            }),
        });
    };

    while i < bytes.len() {
        if bytes[i] != b'<' {
            let end = src[i..].find('<').map(|p| i + p).unwrap_or(src.len());
            if skipping.is_none() {
                buf.push_str(&src[i..end]);
            }
            i = end;
            continue;
        }
        let end = src[i..].find('>').map(|p| i + p).unwrap_or(bytes.len() - 1);
        let tag = &src[i + 1..end.min(src.len())];
        i = end + 1;
        let closing = tag.starts_with('/');
        let name = tag
            .trim_start_matches('/')
            .split(|c: char| c.is_whitespace() || c == '/' || c == '>')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        // A namespace prefix is noise here: `sn:l` and `l` mean the same
        // thing to a reader.
        let local = name.rsplit(':').next().unwrap_or(&name).to_string();

        if let Some(open) = &skipping {
            if closing && *open == local {
                skipping = None;
            }
            continue;
        }
        match local.as_str() {
            "script" | "style" if !closing => skipping = Some(local),
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                flush(&mut buf, &mut heading);
                if !closing {
                    heading = local[1..].parse().ok();
                }
            }
            "p" | "div" | "br" | "li" | "tr" | "blockquote" | "section" | "article" | "l"
            | "paraend" | "line" | "para" => flush(&mut buf, &mut heading),
            "td" | "th" => buf.push(' '),
            _ => {}
        }
    }
    flush(&mut buf, &mut heading);
    blocks
}

fn decode_entities(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut rest = src;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        let Some(end) = rest[..rest.len().min(12)].find(';') else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let entity = &rest[1..end];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some('\u{a0}'),
            _ => entity
                .strip_prefix('#')
                .and_then(|n| match n.strip_prefix(['x', 'X']) {
                    Some(hex) => u32::from_str_radix(hex, 16).ok(),
                    None => n.parse().ok(),
                })
                .and_then(char::from_u32),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}
