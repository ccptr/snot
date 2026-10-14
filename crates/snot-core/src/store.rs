//! The library on disk: one SQLite database and a folder of
//! content-addressed attachments.
//!
//! ```text
//! <library>/
//!   snot.db               notes, folders, attachments, full-text index
//!   attachments/ab/ab12…  one file per distinct attachment, named by SHA-256
//! ```

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use rusqlite::{Connection, OptionalExtension, Row, params};
use sha2::{Digest, Sha256};

use crate::doc::Doc;
use crate::error::{Error, Result};
use crate::note::{
    Counts, Folder, HIT_END, HIT_START, Millis, Note, NoteSummary, Scope, SortBy, display_title,
    now_ms, preview,
};

const SCHEMA_VERSION: i64 = 1;
/// Notes in the trash longer than this are deleted when the library opens.
pub const TRASH_DAYS: i64 = 30;

pub struct Library {
    root: PathBuf,
    conn: Mutex<Connection>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attachment {
    pub id: String,
    pub mime: String,
    pub name: String,
    pub size: i64,
}

const NOTE_COLUMNS: &str = "n.id, n.folder_id, n.title, n.plain, n.color, n.pinned, n.favorite, \
     n.has_ink, n.has_image, n.created_at, n.updated_at, n.trashed_at";

impl Library {
    /// Opens the library in `root`, creating it if needed. A brand-new
    /// library gets a welcome note.
    pub fn open(root: impl AsRef<Path>) -> Result<Library> {
        let root = root.as_ref().to_path_buf();
        std::fs::create_dir_all(root.join("attachments"))?;
        let conn = Connection::open(root.join("snot.db"))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        let lib = Library {
            root,
            conn: Mutex::new(conn),
        };
        let fresh = lib.migrate()?;
        if fresh {
            crate::welcome::seed(&lib)?;
        }
        lib.purge_trash(now_ms() - TRASH_DAYS * 86_400_000)?;
        Ok(lib)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn db(&self) -> MutexGuard<'_, Connection> {
        // A panic while holding the lock leaves the connection itself fine;
        // SQLite rolled back whatever was half done.
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Brings the schema up to date. Returns whether the database was new.
    fn migrate(&self) -> Result<bool> {
        let db = self.db();
        let version: i64 = db.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if version > SCHEMA_VERSION {
            return Err(Error::Invalid(format!(
                "this library was written by a newer Snot (schema {version})"
            )));
        }
        if version == SCHEMA_VERSION {
            return Ok(false);
        }
        db.execute_batch(
            r#"
            BEGIN;
            CREATE TABLE folders (
                id         TEXT PRIMARY KEY,
                parent_id  TEXT REFERENCES folders(id) ON DELETE CASCADE,
                name       TEXT NOT NULL,
                color      TEXT,
                position   INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );
            CREATE TABLE notes (
                rowid      INTEGER PRIMARY KEY,
                id         TEXT NOT NULL UNIQUE,
                folder_id  TEXT REFERENCES folders(id) ON DELETE SET NULL,
                title      TEXT NOT NULL DEFAULT '',
                doc        TEXT NOT NULL,
                plain      TEXT NOT NULL DEFAULT '',
                color      TEXT,
                pinned     INTEGER NOT NULL DEFAULT 0,
                favorite   INTEGER NOT NULL DEFAULT 0,
                has_ink    INTEGER NOT NULL DEFAULT 0,
                has_image  INTEGER NOT NULL DEFAULT 0,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                trashed_at INTEGER
            );
            CREATE INDEX notes_folder ON notes(folder_id, trashed_at);
            CREATE INDEX notes_updated ON notes(updated_at);
            CREATE VIRTUAL TABLE notes_fts USING fts5(
                title, plain,
                content = 'notes', content_rowid = 'rowid',
                tokenize = 'unicode61 remove_diacritics 2'
            );
            CREATE TRIGGER notes_ai AFTER INSERT ON notes BEGIN
                INSERT INTO notes_fts(rowid, title, plain) VALUES (new.rowid, new.title, new.plain);
            END;
            CREATE TRIGGER notes_ad AFTER DELETE ON notes BEGIN
                INSERT INTO notes_fts(notes_fts, rowid, title, plain)
                    VALUES ('delete', old.rowid, old.title, old.plain);
            END;
            CREATE TRIGGER notes_au AFTER UPDATE OF title, plain ON notes BEGIN
                INSERT INTO notes_fts(notes_fts, rowid, title, plain)
                    VALUES ('delete', old.rowid, old.title, old.plain);
                INSERT INTO notes_fts(rowid, title, plain) VALUES (new.rowid, new.title, new.plain);
            END;
            CREATE TABLE attachments (
                id         TEXT PRIMARY KEY,
                mime       TEXT NOT NULL,
                name       TEXT NOT NULL,
                size       INTEGER NOT NULL,
                created_at INTEGER NOT NULL
            );
            PRAGMA user_version = 1;
            COMMIT;
            "#,
        )?;
        Ok(true)
    }

    // ---- folders ---------------------------------------------------------

    pub fn folders(&self) -> Result<Vec<Folder>> {
        let db = self.db();
        let mut stmt = db.prepare(
            "SELECT f.id, f.parent_id, f.name, f.color, f.position,
                    (SELECT COUNT(*) FROM notes n WHERE n.folder_id = f.id AND n.trashed_at IS NULL)
             FROM folders f ORDER BY f.position, f.name COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Folder {
                id: r.get(0)?,
                parent_id: r.get(1)?,
                name: r.get(2)?,
                color: r.get(3)?,
                position: r.get(4)?,
                note_count: r.get(5)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn create_folder(&self, name: &str, parent_id: Option<&str>) -> Result<Folder> {
        let name = clean_name(name)?;
        let id = uuid::Uuid::now_v7().to_string();
        let now = now_ms();
        let db = self.db();
        let position: i64 = db.query_row(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM folders WHERE parent_id IS ?1",
            [parent_id],
            |r| r.get(0),
        )?;
        db.execute(
            "INSERT INTO folders (id, parent_id, name, position, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
            params![id, parent_id, name, position, now],
        )?;
        Ok(Folder {
            id,
            parent_id: parent_id.map(Into::into),
            name,
            color: None,
            position,
            note_count: 0,
        })
    }

    pub fn rename_folder(&self, id: &str, name: &str) -> Result<()> {
        let name = clean_name(name)?;
        self.update_one(
            "UPDATE folders SET name = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, name, now_ms()],
            id,
        )
    }

    pub fn set_folder_color(&self, id: &str, color: Option<&str>) -> Result<()> {
        self.update_one(
            "UPDATE folders SET color = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, color, now_ms()],
            id,
        )
    }

    /// Deletes a folder and its subfolders. The notes in them go to the
    /// trash rather than vanishing with the folder, and restore unfiled.
    pub fn delete_folder(&self, id: &str) -> Result<()> {
        let mut db = self.db();
        let tx = db.transaction()?;
        tx.execute(
            "WITH RECURSIVE sub(id) AS (
                 SELECT ?1 UNION ALL SELECT f.id FROM folders f JOIN sub ON f.parent_id = sub.id
             )
             UPDATE notes SET trashed_at = COALESCE(trashed_at, ?2), folder_id = NULL
             WHERE folder_id IN (SELECT id FROM sub)",
            params![id, now_ms()],
        )?;
        let n = tx.execute("DELETE FROM folders WHERE id = ?1", [id])?;
        if n == 0 {
            return Err(Error::NotFound(id.into()));
        }
        tx.commit()?;
        Ok(())
    }

    // ---- notes -----------------------------------------------------------

    pub fn counts(&self) -> Result<Counts> {
        Ok(self.db().query_row(
            "SELECT
                COALESCE(SUM(trashed_at IS NULL), 0),
                COALESCE(SUM(trashed_at IS NULL AND folder_id IS NULL), 0),
                COALESCE(SUM(trashed_at IS NULL AND favorite), 0),
                COALESCE(SUM(trashed_at IS NOT NULL), 0)
             FROM notes",
            [],
            |r| {
                Ok(Counts {
                    all: r.get(0)?,
                    unfiled: r.get(1)?,
                    favorites: r.get(2)?,
                    trash: r.get(3)?,
                })
            },
        )?)
    }

    pub fn notes(&self, scope: &Scope, sort: SortBy) -> Result<Vec<NoteSummary>> {
        let (filter, arg): (&str, Option<&str>) = match scope {
            Scope::All => ("n.trashed_at IS NULL", None),
            Scope::Folder(id) => ("n.trashed_at IS NULL AND n.folder_id = ?1", Some(id)),
            Scope::Unfiled => ("n.trashed_at IS NULL AND n.folder_id IS NULL", None),
            Scope::Favorites => ("n.trashed_at IS NULL AND n.favorite", None),
            Scope::Trash => ("n.trashed_at IS NOT NULL", None),
        };
        let order = match (scope, sort) {
            (Scope::Trash, _) => "n.trashed_at DESC",
            (_, SortBy::Modified) => "n.pinned DESC, n.updated_at DESC",
            (_, SortBy::Created) => "n.pinned DESC, n.created_at DESC",
            (_, SortBy::Title) => {
                "n.pinned DESC, CASE n.title WHEN '' THEN n.plain ELSE n.title END COLLATE NOCASE"
            }
        };
        let sql = format!("SELECT {NOTE_COLUMNS} FROM notes n WHERE {filter} ORDER BY {order}");
        let db = self.db();
        let mut stmt = db.prepare(&sql)?;
        let rows = match arg {
            Some(a) => stmt
                .query_map([a], summary_row)?
                .collect::<std::result::Result<Vec<_>, _>>(),
            None => stmt.query_map([], summary_row)?.collect(),
        };
        Ok(rows?)
    }

    pub fn create_note(&self, folder_id: Option<&str>) -> Result<Note> {
        self.insert_note(folder_id, String::new(), Doc::new())
    }

    pub(crate) fn insert_note(
        &self,
        folder_id: Option<&str>,
        title: String,
        mut doc: Doc,
    ) -> Result<Note> {
        doc.sanitize();
        let now = now_ms();
        let note = Note {
            id: uuid::Uuid::now_v7().to_string(),
            folder_id: folder_id.map(Into::into),
            title,
            doc,
            color: None,
            pinned: false,
            favorite: false,
            created_at: now,
            updated_at: now,
            trashed_at: None,
        };
        let json = serde_json::to_string(&note.doc)?;
        self.db().execute(
            "INSERT INTO notes (id, folder_id, title, doc, plain, has_ink, has_image, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
            params![
                note.id,
                note.folder_id,
                note.title,
                json,
                note.doc.plain_text(),
                note.doc.has_ink(),
                note.doc.has_image(),
                now
            ],
        )?;
        Ok(note)
    }

    pub fn note(&self, id: &str) -> Result<Note> {
        let db = self.db();
        let row = db
            .query_row(
                "SELECT id, folder_id, title, doc, color, pinned, favorite, created_at, updated_at, trashed_at
                 FROM notes WHERE id = ?1",
                [id],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, Option<String>>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, Option<String>>(4)?,
                        r.get::<_, bool>(5)?,
                        r.get::<_, bool>(6)?,
                        r.get::<_, Millis>(7)?,
                        r.get::<_, Millis>(8)?,
                        r.get::<_, Option<Millis>>(9)?,
                    ))
                },
            )
            .optional()?
            .ok_or_else(|| Error::NotFound(id.into()))?;
        let mut doc: Doc = serde_json::from_str(&row.3)?;
        doc.sanitize();
        Ok(Note {
            id: row.0,
            folder_id: row.1,
            title: row.2,
            doc,
            color: row.4,
            pinned: row.5,
            favorite: row.6,
            created_at: row.7,
            updated_at: row.8,
            trashed_at: row.9,
        })
    }

    /// Writes a note's title and body. Nothing else about the note is
    /// touched, so an editor holding an older copy cannot undo a pin or a
    /// move made elsewhere. Returns the new modified time, or `None` when
    /// nothing had changed — just opening a note never makes it "recent".
    pub fn save_content(&self, id: &str, title: &str, doc: &Doc) -> Result<Option<Millis>> {
        let json = serde_json::to_string(doc)?;
        let db = self.db();
        let same: Option<bool> = db
            .query_row(
                "SELECT title = ?2 AND doc = ?3 FROM notes WHERE id = ?1",
                params![id, title, json],
                |r| r.get(0),
            )
            .optional()?;
        match same {
            None => return Err(Error::NotFound(id.into())),
            Some(true) => return Ok(None),
            Some(false) => {}
        }
        let now = now_ms();
        db.execute(
            "UPDATE notes SET title = ?2, doc = ?3, plain = ?4, has_ink = ?5, has_image = ?6, updated_at = ?7
             WHERE id = ?1",
            params![id, title, json, doc.plain_text(), doc.has_ink(), doc.has_image(), now],
        )?;
        Ok(Some(now))
    }

    pub fn set_pinned(&self, id: &str, on: bool) -> Result<()> {
        self.update_one(
            "UPDATE notes SET pinned = ?2 WHERE id = ?1",
            params![id, on],
            id,
        )
    }

    pub fn set_favorite(&self, id: &str, on: bool) -> Result<()> {
        self.update_one(
            "UPDATE notes SET favorite = ?2 WHERE id = ?1",
            params![id, on],
            id,
        )
    }

    pub fn set_note_color(&self, id: &str, color: Option<&str>) -> Result<()> {
        self.update_one(
            "UPDATE notes SET color = ?2 WHERE id = ?1",
            params![id, color],
            id,
        )
    }

    pub fn move_note(&self, id: &str, folder_id: Option<&str>) -> Result<()> {
        self.update_one(
            "UPDATE notes SET folder_id = ?2 WHERE id = ?1",
            params![id, folder_id],
            id,
        )
    }

    pub fn trash_note(&self, id: &str) -> Result<()> {
        self.update_one(
            "UPDATE notes SET trashed_at = ?2, pinned = 0 WHERE id = ?1",
            params![id, now_ms()],
            id,
        )
    }

    /// Brings a note back from the trash. If its folder has gone meanwhile
    /// the foreign key has already unfiled it.
    pub fn restore_note(&self, id: &str) -> Result<()> {
        self.update_one(
            "UPDATE notes SET trashed_at = NULL WHERE id = ?1",
            params![id],
            id,
        )
    }

    pub fn delete_note(&self, id: &str) -> Result<()> {
        self.update_one("DELETE FROM notes WHERE id = ?1", params![id], id)?;
        self.collect_garbage()?;
        Ok(())
    }

    pub fn empty_trash(&self) -> Result<usize> {
        let n = self
            .db()
            .execute("DELETE FROM notes WHERE trashed_at IS NOT NULL", [])?;
        self.collect_garbage()?;
        Ok(n)
    }

    fn purge_trash(&self, before: Millis) -> Result<()> {
        let n = self
            .db()
            .execute("DELETE FROM notes WHERE trashed_at < ?1", [before])?;
        if n > 0 {
            self.collect_garbage()?;
        }
        Ok(())
    }

    pub fn duplicate_note(&self, id: &str) -> Result<Note> {
        let src = self.note(id)?;
        let mut doc = src.doc.clone();
        for b in &mut doc.blocks {
            b.id = crate::doc::new_block_id();
        }
        let title = if src.title.is_empty() {
            String::new()
        } else {
            format!("{} (copy)", src.title)
        };
        let note = self.insert_note(src.folder_id.as_deref(), title, doc)?;
        if src.color.is_some() {
            self.set_note_color(&note.id, src.color.as_deref())?;
        }
        self.note(&note.id)
    }

    fn update_one(&self, sql: &str, params: impl rusqlite::Params, id: &str) -> Result<()> {
        match self.db().execute(sql, params)? {
            0 => Err(Error::NotFound(id.into())),
            _ => Ok(()),
        }
    }

    // ---- search ----------------------------------------------------------

    /// Finds notes whose title or text contains every word of `query`.
    ///
    /// Full-text matches come first, ranked. Words are also matched as plain
    /// substrings, which the word index alone cannot do: Korean and Japanese
    /// attach particles to words, and someone searching "note" expects to
    /// find "notebook" and "footnote" alike.
    pub fn search(&self, query: &str) -> Result<Vec<NoteSummary>> {
        let words: Vec<String> = query.split_whitespace().map(str::to_string).collect();
        if words.is_empty() {
            return Ok(Vec::new());
        }
        let mut found: Vec<NoteSummary> = Vec::new();
        let mut seen = HashSet::new();
        let db = self.db();

        let fts_query = words
            .iter()
            .map(|w| format!("\"{}\"*", w.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(" ");
        let sql = format!(
            "SELECT {NOTE_COLUMNS},
                    snippet(notes_fts, 1, char(2), char(3), '\u{2026}', 16)
             FROM notes_fts JOIN notes n ON n.rowid = notes_fts.rowid
             WHERE notes_fts MATCH ?1 AND n.trashed_at IS NULL
             ORDER BY bm25(notes_fts, 4.0, 1.0) LIMIT 200"
        );
        // A query FTS cannot parse (stray punctuation, say) is not an error
        // to the person typing it; the substring pass below still runs.
        let fts: rusqlite::Result<Vec<NoteSummary>> = db.prepare(&sql).and_then(|mut stmt| {
            stmt.query_map([&fts_query], |r| {
                let mut s = summary_row(r)?;
                s.snippet = r
                    .get::<_, Option<String>>(12)?
                    .filter(|s| s.contains(HIT_START));
                Ok(s)
            })?
            .collect()
        });
        for row in fts.unwrap_or_default() {
            seen.insert(row.id.clone());
            found.push(row);
        }

        let mut sql = format!("SELECT {NOTE_COLUMNS} FROM notes n WHERE n.trashed_at IS NULL");
        for i in 1..=words.len() {
            sql.push_str(&format!(
                " AND (n.title || char(10) || n.plain) LIKE ?{i} ESCAPE '\\'"
            ));
        }
        sql.push_str(" ORDER BY n.updated_at DESC LIMIT 200");
        let patterns: Vec<String> = words
            .iter()
            .map(|w| {
                format!(
                    "%{}%",
                    w.replace('\\', "\\\\")
                        .replace('%', "\\%")
                        .replace('_', "\\_")
                )
            })
            .collect();
        let mut stmt = db.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(&patterns), |r| {
            Ok((summary_row(r)?, r.get::<_, String>(3)?))
        })?;
        for row in rows {
            let (mut s, plain) = row?;
            if seen.insert(s.id.clone()) {
                s.snippet = substring_snippet(&plain, &words);
                found.push(s);
            }
        }
        Ok(found)
    }

    // ---- attachments -----------------------------------------------------

    /// Stores `bytes` and returns the attachment. Storing the same bytes
    /// twice yields the same attachment and one file on disk.
    pub fn add_attachment(&self, bytes: &[u8], mime: &str, name: &str) -> Result<Attachment> {
        let id = format!("{:x}", Sha256::digest(bytes));
        let path = self.attachment_path(&id);
        if !path.exists() {
            std::fs::create_dir_all(path.parent().expect("attachment paths have a parent"))?;
            let tmp = path.with_extension("part");
            std::fs::write(&tmp, bytes)?;
            std::fs::rename(&tmp, &path)?;
        }
        self.db().execute(
            "INSERT OR IGNORE INTO attachments (id, mime, name, size, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, mime, name, bytes.len() as i64, now_ms()],
        )?;
        self.attachment(&id)
    }

    pub fn attachment(&self, id: &str) -> Result<Attachment> {
        self.db()
            .query_row(
                "SELECT id, mime, name, size FROM attachments WHERE id = ?1",
                [id],
                |r| {
                    Ok(Attachment {
                        id: r.get(0)?,
                        mime: r.get(1)?,
                        name: r.get(2)?,
                        size: r.get(3)?,
                    })
                },
            )
            .optional()?
            .ok_or_else(|| Error::NotFound(id.into()))
    }

    pub fn attachment_path(&self, id: &str) -> PathBuf {
        let shard = id.get(..2).unwrap_or("xx");
        self.root.join("attachments").join(shard).join(id)
    }

    /// Deletes attachments no note refers to any more, trash included.
    pub fn collect_garbage(&self) -> Result<usize> {
        let db = self.db();
        let mut used = HashSet::new();
        let mut stmt = db.prepare("SELECT doc FROM notes WHERE has_image")?;
        for json in stmt.query_map([], |r| r.get::<_, String>(0))? {
            if let Ok(doc) = serde_json::from_str::<Doc>(&json?) {
                used.extend(doc.attachments());
            }
        }
        let all: Vec<String> = db
            .prepare("SELECT id FROM attachments")?
            .query_map([], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        let mut removed = 0;
        for id in all.into_iter().filter(|id| !used.contains(id)) {
            db.execute("DELETE FROM attachments WHERE id = ?1", [&id])?;
            match std::fs::remove_file(self.attachment_path(&id)) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
            removed += 1;
        }
        Ok(removed)
    }
}

fn clean_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::Invalid("a folder needs a name".into()));
    }
    Ok(name.chars().take(80).collect())
}

fn summary_row(r: &Row) -> rusqlite::Result<NoteSummary> {
    let title: String = r.get(2)?;
    let plain: String = r.get(3)?;
    Ok(NoteSummary {
        id: r.get(0)?,
        folder_id: r.get(1)?,
        title: display_title(&title, &plain),
        preview: preview(&title, &plain),
        color: r.get(4)?,
        pinned: r.get(5)?,
        favorite: r.get(6)?,
        has_ink: r.get(7)?,
        has_image: r.get(8)?,
        created_at: r.get(9)?,
        updated_at: r.get(10)?,
        trashed_at: r.get(11)?,
        snippet: None,
    })
}

/// A short passage around the first word found, with every word marked.
fn substring_snippet(plain: &str, words: &[String]) -> Option<String> {
    let lower: Vec<char> = plain.chars().flat_map(char::to_lowercase).collect();
    let chars: Vec<char> = plain.chars().collect();
    // Lowercasing can change a string's length (İ, ß); without a one-to-one
    // mapping the offsets would point at the wrong text, so skip marking.
    if lower.len() != chars.len() {
        return None;
    }
    let needles: Vec<Vec<char>> = words
        .iter()
        .map(|w| w.chars().flat_map(char::to_lowercase).collect())
        .collect();
    let find = |from: usize, needle: &[char]| -> Option<usize> {
        (from..=lower.len().checked_sub(needle.len())?)
            .find(|&i| lower[i..i + needle.len()] == *needle)
    };
    let first = needles.iter().filter_map(|n| find(0, n)).min()?;
    let start = first.saturating_sub(40);
    let end = (first + 120).min(chars.len());
    let mut marked = vec![false; chars.len()];
    for n in needles.iter().filter(|n| !n.is_empty()) {
        let mut at = start;
        while let Some(i) = find(at, n) {
            if i >= end {
                break;
            }
            marked[i..i + n.len()].iter_mut().for_each(|m| *m = true);
            at = i + n.len();
        }
    }
    let mut out = String::new();
    if start > 0 {
        out.push('\u{2026}');
    }
    let mut open = false;
    for i in start..end {
        if marked[i] != open {
            out.push(if marked[i] { HIT_START } else { HIT_END });
            open = marked[i];
        }
        out.push(if chars[i] == '\n' { ' ' } else { chars[i] });
    }
    if open {
        out.push(HIT_END);
    }
    if end < chars.len() {
        out.push('\u{2026}');
    }
    Some(out)
}
