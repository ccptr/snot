//! Markdown in and out: the way notes leave Snot, and the way a folder of
//! notes from somewhere else comes in.

use std::path::{Path, PathBuf};

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::doc::{
    Block, Content, Doc, ImageBlock, Mark, ParaStyle, Span, TextBlock, char_len, new_block_id,
};
use crate::error::Result;
use crate::note::Note;
use crate::store::Library;

/// A note rendered as Markdown, plus the files it links to.
pub struct Exported {
    pub markdown: String,
    /// `(relative path, bytes)` for each picture and the handwriting.
    pub files: Vec<(String, Vec<u8>)>,
}

/// Renders a note as Markdown. Handwriting becomes an SVG file and pictures
/// are copied out, both under `assets_dir` (a path relative to the
/// Markdown).
pub fn export(note: &Note, lib: &Library, assets_dir: &str) -> Exported {
    let mut md = String::new();
    let mut files = Vec::new();
    if !note.title.trim().is_empty() {
        md.push_str(&format!("# {}\n\n", escape_line(note.title.trim())));
    }
    let numbers = note.doc.list_numbers();
    let mut prev_list = false;
    for (i, block) in note.doc.blocks.iter().enumerate() {
        let is_list = block.as_text().is_some_and(|t| t.style.is_list());
        // List items sit tight together; everything else gets a blank line.
        if i > 0 && !(is_list && prev_list) {
            md.push('\n');
        }
        prev_list = is_list;
        match &block.content {
            Content::Text(t) => {
                let pad = "   ".repeat(t.indent as usize);
                let body = inline(t);
                match t.style {
                    ParaStyle::Body => md.push_str(&body),
                    ParaStyle::Heading1 => md.push_str(&format!("## {body}")),
                    ParaStyle::Heading2 => md.push_str(&format!("### {body}")),
                    ParaStyle::Heading3 => md.push_str(&format!("#### {body}")),
                    ParaStyle::Bullet => md.push_str(&format!("{pad}- {body}")),
                    ParaStyle::Numbered => {
                        md.push_str(&format!("{pad}{}. {body}", numbers[i].unwrap_or(1)))
                    }
                    ParaStyle::Checklist => md.push_str(&format!(
                        "{pad}- [{}] {body}",
                        if t.checked { "x" } else { " " }
                    )),
                    ParaStyle::Quote => md.push_str(&format!("> {body}")),
                    ParaStyle::Code => md.push_str(&format!("```\n{}\n```", t.text)),
                }
                md.push('\n');
            }
            Content::Divider => md.push_str("---\n"),
            Content::Image(img) => {
                let Ok(bytes) = std::fs::read(lib.attachment_path(&img.attachment)) else {
                    md.push_str("*(missing picture)*\n");
                    continue;
                };
                let ext = lib
                    .attachment(&img.attachment)
                    .ok()
                    .and_then(|a| extension_for(&a.mime))
                    .unwrap_or("bin");
                let path = format!(
                    "{assets_dir}/{}.{ext}",
                    &img.attachment[..16.min(img.attachment.len())]
                );
                md.push_str(&format!(
                    "![{}]({})\n",
                    escape_line(&img.caption),
                    path.replace(' ', "%20")
                ));
                files.push((path, bytes));
            }
        }
    }
    // Markdown has no layer to lay handwriting over the text, so the page's
    // handwriting follows it as one picture.
    if note.doc.has_ink() {
        let path = format!("{assets_dir}/handwriting.svg");
        md.push_str(&format!("\n![handwriting]({})\n", path.replace(' ', "%20")));
        files.push((path, note.doc.ink.to_svg(false).into_bytes()));
    }
    Exported {
        markdown: md,
        files,
    }
}

fn extension_for(mime: &str) -> Option<&'static str> {
    Some(match mime {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/svg+xml" => "svg",
        _ => return None,
    })
}

fn escape_line(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(
            c,
            '\\' | '*' | '_' | '`' | '[' | ']' | '~' | '<' | '>' | '#'
        ) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// A text block's contents with bold, italic and strikethrough as
/// Markdown. Underline, colour and highlight have no Markdown and are
/// dropped; the text itself never is.
fn inline(t: &TextBlock) -> String {
    let chars: Vec<char> = t.text.chars().collect();
    let mut out = String::new();
    for run in t.runs() {
        let text: String = chars[run.start as usize..run.end as usize].iter().collect();
        let escaped = escape_line(&text);
        // Emphasis markers must hug the text, so surrounding spaces go
        // outside them.
        let core = escaped.trim();
        if core.is_empty() {
            out.push_str(&escaped);
            continue;
        }
        let lead = &escaped[..escaped.len() - escaped.trim_start().len()];
        let trail = &escaped[escaped.trim_end().len()..];
        let mut wrapped = core.to_string();
        if run.strike {
            wrapped = format!("~~{wrapped}~~");
        }
        if run.italic {
            wrapped = format!("*{wrapped}*");
        }
        if run.bold {
            wrapped = format!("**{wrapped}**");
        }
        out.push_str(lead);
        out.push_str(&wrapped);
        out.push_str(trail);
    }
    out
}

/// Parses Markdown into a title (a leading top-level heading, if any) and
/// a document. Pictures are resolved against `base` and stored in `lib`.
pub fn import(md: &str, base: Option<&Path>, lib: Option<&Library>) -> (String, Doc) {
    let opts = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let mut b = Builder::default();
    for event in Parser::new_ext(md, opts) {
        b.event(event, base, lib);
    }
    b.flush();
    let mut blocks = b.blocks;
    let mut title = String::new();
    if let Some(Content::Text(t)) = blocks.first().map(|b| &b.content)
        && t.style == ParaStyle::Heading1
        && b.saw_h1_first
    {
        title = t.text.clone();
        blocks.remove(0);
    }
    let mut doc = Doc::of(blocks);
    doc.sanitize();
    (title, doc)
}

#[derive(Default)]
struct Builder {
    blocks: Vec<Block>,
    current: Option<TextBlock>,
    marks: Vec<(Mark, u32)>,
    lists: Vec<bool>,
    quote: u32,
    code: bool,
    pending_check: Option<bool>,
    saw_h1_first: bool,
    image: Option<(String, String)>,
}

impl Builder {
    fn start_block(&mut self, style: ParaStyle) {
        self.flush();
        let indent = self
            .lists
            .len()
            .saturating_sub(1)
            .min(crate::editor::MAX_INDENT as usize) as u8;
        self.current = Some(TextBlock {
            style,
            indent: if style.is_list() { indent } else { 0 },
            ..Default::default()
        });
    }

    fn cur(&mut self) -> &mut TextBlock {
        if self.current.is_none() {
            let style = match (self.lists.last(), self.quote > 0) {
                (Some(true), _) => ParaStyle::Numbered,
                (Some(false), _) => ParaStyle::Bullet,
                (None, true) => ParaStyle::Quote,
                _ => ParaStyle::Body,
            };
            self.start_block(style);
        }
        self.current.as_mut().expect("just set")
    }

    fn flush(&mut self) {
        if let Some(mut t) = self.current.take() {
            if let Some(checked) = self.pending_check.take() {
                t.style = ParaStyle::Checklist;
                t.checked = checked;
            }
            let trimmed = t.text.trim_end().len();
            if trimmed < t.text.len() {
                t.text.truncate(trimmed);
            }
            t.normalize();
            if !(t.text.is_empty() && t.style == ParaStyle::Body) {
                self.blocks.push(Block {
                    id: new_block_id(),
                    content: Content::Text(t),
                });
            }
        }
    }

    fn push_text(&mut self, s: &str) {
        let t = self.cur();
        t.text.push_str(s);
    }

    fn event(&mut self, e: Event, base: Option<&Path>, lib: Option<&Library>) {
        match e {
            Event::Start(Tag::Paragraph) => match self.lists.last() {
                None => self.start_block(if self.quote > 0 {
                    ParaStyle::Quote
                } else {
                    ParaStyle::Body
                }),
                // A second paragraph inside one list item: a new item line.
                Some(_) if self.current.as_ref().is_some_and(|t| !t.text.is_empty()) => {
                    let style = self
                        .current
                        .as_ref()
                        .map(|t| t.style)
                        .unwrap_or(ParaStyle::Bullet);
                    self.start_block(style);
                }
                Some(_) => {}
            },
            Event::End(TagEnd::Paragraph) => {
                if self.lists.is_empty() {
                    self.flush();
                }
            }
            Event::Start(Tag::Heading { level, .. }) => {
                if self.blocks.is_empty() && self.current.is_none() && level == HeadingLevel::H1 {
                    self.saw_h1_first = true;
                }
                self.start_block(match level {
                    HeadingLevel::H1 | HeadingLevel::H2 => ParaStyle::Heading1,
                    HeadingLevel::H3 => ParaStyle::Heading2,
                    _ => ParaStyle::Heading3,
                });
            }
            Event::End(TagEnd::Heading(_)) => self.flush(),
            Event::Start(Tag::BlockQuote(_)) => {
                self.flush();
                self.quote += 1;
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                self.flush();
                self.quote = self.quote.saturating_sub(1);
            }
            Event::Start(Tag::CodeBlock(_)) => {
                self.start_block(ParaStyle::Code);
                self.code = true;
            }
            Event::End(TagEnd::CodeBlock) => {
                self.code = false;
                self.flush();
            }
            Event::Start(Tag::List(first)) => {
                self.flush();
                self.lists.push(first.is_some());
            }
            Event::End(TagEnd::List(_)) => {
                self.flush();
                self.lists.pop();
            }
            Event::Start(Tag::Item) => {
                let style = if self.lists.last() == Some(&true) {
                    ParaStyle::Numbered
                } else {
                    ParaStyle::Bullet
                };
                self.start_block(style);
            }
            Event::End(TagEnd::Item) => self.flush(),
            Event::TaskListMarker(checked) => self.pending_check = Some(checked),
            Event::Start(Tag::Strong) => self.open_mark(Mark::Bold),
            Event::Start(Tag::Emphasis) => self.open_mark(Mark::Italic),
            Event::Start(Tag::Strikethrough) => self.open_mark(Mark::Strike),
            Event::End(TagEnd::Strong) => self.close_mark(Mark::Bold),
            Event::End(TagEnd::Emphasis) => self.close_mark(Mark::Italic),
            Event::End(TagEnd::Strikethrough) => self.close_mark(Mark::Strike),
            Event::Start(Tag::Image { dest_url, .. }) => {
                self.image = Some((dest_url.to_string(), String::new()));
            }
            Event::End(TagEnd::Image) => {
                if let Some((url, alt)) = self.image.take() {
                    self.image_block(&url, &alt, base, lib);
                }
            }
            Event::Text(s) | Event::Code(s) => {
                if let Some((_, alt)) = &mut self.image {
                    alt.push_str(&s);
                } else if self.code {
                    // A code block's text arrives with its line breaks; a
                    // block holds one line, so each becomes its own.
                    let mut lines = s.trim_end_matches('\n').split('\n').peekable();
                    while let Some(line) = lines.next() {
                        self.push_text(line);
                        if lines.peek().is_some() {
                            self.start_block(ParaStyle::Code);
                        }
                    }
                } else {
                    self.push_text(&s);
                }
            }
            Event::SoftBreak => self.push_text(" "),
            Event::HardBreak => {
                let style = self.cur().style;
                self.start_block(style);
            }
            Event::Rule => {
                self.flush();
                self.blocks.push(Block {
                    id: new_block_id(),
                    content: Content::Divider,
                });
            }
            _ => {}
        }
    }

    fn open_mark(&mut self, mark: Mark) {
        let at = char_len(&self.cur().text);
        self.marks.push((mark, at));
    }

    fn close_mark(&mut self, mark: Mark) {
        let Some(pos) = self.marks.iter().rposition(|(m, _)| *m == mark) else {
            return;
        };
        let (mark, start) = self.marks.remove(pos);
        let t = self.cur();
        let end = char_len(&t.text);
        if end > start {
            t.spans.push(Span { start, end, mark });
        }
    }

    fn image_block(&mut self, url: &str, alt: &str, base: Option<&Path>, lib: Option<&Library>) {
        let stored = (|| {
            let lib = lib?;
            if url.contains("://") {
                return None;
            }
            let rel = percent_decode(url);
            let path: PathBuf = match base {
                Some(b) => b.join(&rel),
                None => PathBuf::from(&rel),
            };
            let bytes = std::fs::read(&path).ok()?;
            let mime = sniff_image(&bytes)?;
            let att = lib
                .add_attachment(&bytes, mime, path.file_name()?.to_str()?)
                .ok()?;
            Some((att.id, image_aspect(&bytes).unwrap_or(0.75)))
        })();
        match stored {
            Some((attachment, aspect)) => {
                self.flush();
                self.blocks.push(Block {
                    id: new_block_id(),
                    content: Content::Image(ImageBlock {
                        attachment,
                        aspect,
                        caption: alt.to_string(),
                    }),
                });
            }
            // A picture that cannot be brought in still leaves a trace.
            None => {
                let label = if alt.is_empty() {
                    url.to_string()
                } else {
                    format!("{alt} ({url})")
                };
                self.push_text(&label);
            }
        }
    }
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Recognises the picture formats every front-end can show.
pub fn sniff_image(bytes: &[u8]) -> Option<&'static str> {
    Some(match bytes {
        [0x89, b'P', b'N', b'G', ..] => "image/png",
        [0xff, 0xd8, 0xff, ..] => "image/jpeg",
        [b'G', b'I', b'F', b'8', ..] => "image/gif",
        [
            b'R',
            b'I',
            b'F',
            b'F',
            _,
            _,
            _,
            _,
            b'W',
            b'E',
            b'B',
            b'P',
            ..,
        ] => "image/webp",
        _ => return None,
    })
}

/// Height over width of a PNG, JPEG or GIF, read from its header.
pub fn image_aspect(bytes: &[u8]) -> Option<f32> {
    let (w, h) = image_size(bytes)?;
    (w > 0 && h > 0).then(|| h as f32 / w as f32)
}

fn image_size(b: &[u8]) -> Option<(u32, u32)> {
    match sniff_image(b)? {
        "image/png" if b.len() >= 24 => Some((
            u32::from_be_bytes(b[16..20].try_into().ok()?),
            u32::from_be_bytes(b[20..24].try_into().ok()?),
        )),
        "image/gif" if b.len() >= 10 => Some((
            u16::from_le_bytes([b[6], b[7]]) as u32,
            u16::from_le_bytes([b[8], b[9]]) as u32,
        )),
        "image/jpeg" => {
            let mut i = 2;
            while i + 9 < b.len() {
                if b[i] != 0xff {
                    i += 1;
                    continue;
                }
                let marker = b[i + 1];
                let len = u16::from_be_bytes([b[i + 2], b[i + 3]]) as usize;
                // Start-of-frame markers carry the dimensions.
                if matches!(marker, 0xc0..=0xcf) && !matches!(marker, 0xc4 | 0xc8 | 0xcc) {
                    let h = u16::from_be_bytes([b[i + 5], b[i + 6]]) as u32;
                    let w = u16::from_be_bytes([b[i + 7], b[i + 8]]) as u32;
                    return Some((w, h));
                }
                i += 2 + len;
            }
            None
        }
        _ => None,
    }
}

impl Library {
    /// Writes a note to `dir` as `<title>.md`, with its pictures and
    /// drawings in `<title>.assets/`. Returns the Markdown file's path.
    pub fn export_markdown(&self, id: &str, dir: &Path) -> Result<PathBuf> {
        let note = self.note(id)?;
        let stem = file_stem(&note.display_title());
        let mut path = dir.join(format!("{stem}.md"));
        let mut n = 2;
        while path.exists() {
            path = dir.join(format!("{stem} {n}.md"));
            n += 1;
        }
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("note")
            .to_string();
        let assets = format!("{stem}.assets");
        let out = export(&note, self, &assets);
        std::fs::create_dir_all(dir)?;
        for (rel, bytes) in &out.files {
            let p = dir.join(rel);
            std::fs::create_dir_all(p.parent().expect("asset paths have a parent"))?;
            std::fs::write(p, bytes)?;
        }
        std::fs::write(&path, out.markdown)?;
        Ok(path)
    }

    /// Imports one Markdown file as a note.
    pub fn import_markdown(&self, path: &Path, folder_id: Option<&str>) -> Result<Note> {
        let text = std::fs::read_to_string(path)?;
        let (mut title, doc) = import(&text, path.parent(), Some(self));
        if title.is_empty() && doc.plain_text().is_empty() {
            title = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string();
        }
        let note = self.insert_note(folder_id, title, doc)?;
        Ok(note)
    }
}

/// A title made safe to use as a file name on every platform.
fn file_stem(title: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control() {
                '-'
            } else {
                c
            }
        })
        .take(80)
        .collect();
    let cleaned = cleaned.trim().trim_matches('.').to_string();
    if cleaned.is_empty() {
        "Untitled".into()
    } else {
        cleaned
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(doc: &Doc) -> Vec<(ParaStyle, String)> {
        doc.blocks
            .iter()
            .filter_map(|b| b.as_text().map(|t| (t.style, t.text.clone())))
            .collect()
    }

    #[test]
    fn reads_the_common_shapes() {
        let (title, doc) = import(
            "# Groceries\n\nSome **bold** words.\n\n- [x] milk\n- [ ] eggs\n\n1. one\n2. two\n\n> quoted\n\n---\n\n```\nfn a()\nfn b()\n```\n",
            None,
            None,
        );
        assert_eq!(title, "Groceries");
        let t = texts(&doc);
        assert_eq!(t[0], (ParaStyle::Body, "Some bold words.".into()));
        assert_eq!(t[1], (ParaStyle::Checklist, "milk".into()));
        assert_eq!(t[3], (ParaStyle::Numbered, "one".into()));
        assert_eq!(t[5], (ParaStyle::Quote, "quoted".into()));
        assert_eq!(t[6], (ParaStyle::Code, "fn a()".into()));
        assert_eq!(t[7], (ParaStyle::Code, "fn b()".into()));
        let first = doc.blocks[0].as_text().unwrap();
        assert_eq!(
            first.spans,
            vec![Span {
                start: 5,
                end: 9,
                mark: Mark::Bold
            }]
        );
        assert!(doc.blocks[1].as_text().unwrap().checked);
        assert!(
            doc.blocks
                .iter()
                .any(|b| matches!(b.content, Content::Divider))
        );
    }

    #[test]
    fn a_heading_that_is_not_first_is_not_the_title() {
        let (title, doc) = import("intro\n\n# Part one\n", None, None);
        assert!(title.is_empty());
        assert_eq!(texts(&doc)[1], (ParaStyle::Heading1, "Part one".into()));
        let (title, _) = import("## Sub\n", None, None);
        assert!(title.is_empty());
    }

    #[test]
    fn nested_lists_indent() {
        let (_, doc) = import("- a\n  - b\n    - c\n- d\n", None, None);
        let indents: Vec<u8> = doc
            .blocks
            .iter()
            .filter_map(|b| b.as_text())
            .map(|t| t.indent)
            .collect();
        assert_eq!(&indents[..4], &[0, 1, 2, 0]);
    }

    #[test]
    fn emphasis_hugs_its_text() {
        let mut t = TextBlock {
            text: "say hello there".into(),
            ..Default::default()
        };
        t.toggle_mark(3..10, &Mark::Bold);
        assert_eq!(inline(&t), "say **hello** there");
    }

    #[test]
    fn reads_image_headers() {
        let mut png = vec![
            0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, b'I', b'H', b'D', b'R',
        ];
        png.extend(200u32.to_be_bytes());
        png.extend(100u32.to_be_bytes());
        assert_eq!(image_aspect(&png), Some(0.5));
    }
}
