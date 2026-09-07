use std::path::{Path, PathBuf};

use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Row};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};
use crate::model::*;

const SCHEMA: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS folders (
    id         TEXT PRIMARY KEY,
    parent_id  TEXT REFERENCES folders(id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    color      TEXT,
    position   INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS folders_parent ON folders(parent_id, position);

CREATE TABLE IF NOT EXISTS notes (
    id         TEXT PRIMARY KEY,
    folder_id  TEXT REFERENCES folders(id) ON DELETE SET NULL,
    title      TEXT NOT NULL DEFAULT '',
    doc        TEXT NOT NULL,
    ink        TEXT NOT NULL DEFAULT '[]',
    preview    TEXT NOT NULL DEFAULT '',
    color      TEXT,
    pinned     INTEGER NOT NULL DEFAULT 0,
    favorite   INTEGER NOT NULL DEFAULT 0,
    locked     INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    trashed_at INTEGER
);
CREATE INDEX IF NOT EXISTS notes_folder  ON notes(folder_id, trashed_at, updated_at DESC);
CREATE INDEX IF NOT EXISTS notes_updated ON notes(trashed_at, updated_at DESC);

CREATE TABLE IF NOT EXISTS tags (
    id    TEXT PRIMARY KEY,
    name  TEXT NOT NULL UNIQUE COLLATE NOCASE,
    color TEXT
);

CREATE TABLE IF NOT EXISTS note_tags (
    note_id TEXT NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
    tag_id  TEXT NOT NULL REFERENCES tags(id)  ON DELETE CASCADE,
    PRIMARY KEY (note_id, tag_id)
);
CREATE INDEX IF NOT EXISTS note_tags_tag ON note_tags(tag_id);

CREATE TABLE IF NOT EXISTS attachments (
    id         TEXT PRIMARY KEY,
    note_id    TEXT REFERENCES notes(id) ON DELETE SET NULL,
    mime       TEXT NOT NULL,
    name       TEXT NOT NULL,
    size       INTEGER NOT NULL,
    sha256     TEXT NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS attachments_note ON attachments(note_id);
CREATE INDEX IF NOT EXISTS attachments_sha  ON attachments(sha256);

CREATE VIRTUAL TABLE IF NOT EXISTS notes_fts USING fts5(
    note_id UNINDEXED,
    title,
    body,
    tokenize = "unicode61 remove_diacritics 2"
);
"#;

/// The whole library: one SQLite file plus a content-addressed blob directory.
pub struct Store {
    conn: Connection,
    root: PathBuf,
}

impl Store {
    /// Opens (creating if needed) a library rooted at `root`. The directory
    /// holds `snot.db` and an `attachments/` tree.
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        std::fs::create_dir_all(root.join("attachments"))?;
        let conn = Connection::open(root.join("snot.db"))?;
        conn.execute_batch(SCHEMA)?;
        let store = Store { conn, root };
        store.set_meta("schema_version", "1")?;
        Ok(store)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        Ok(Store {
            conn,
            root: PathBuf::from("."),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO meta(key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    // ---------------------------------------------------------------- folders

    pub fn list_folders(&self) -> Result<Vec<Folder>> {
        let mut stmt = self.conn.prepare(
            "SELECT f.id, f.parent_id, f.name, f.color, f.position, f.created_at, f.updated_at,
                    (SELECT COUNT(*) FROM notes n
                      WHERE n.folder_id = f.id AND n.trashed_at IS NULL)
             FROM folders f
             ORDER BY f.position, f.name COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Folder {
                id: r.get(0)?,
                parent_id: r.get(1)?,
                name: r.get(2)?,
                color: r.get(3)?,
                position: r.get(4)?,
                created_at: r.get(5)?,
                updated_at: r.get(6)?,
                note_count: r.get(7)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn create_folder(&self, name: &str, parent_id: Option<&str>) -> Result<Folder> {
        let name = name.trim();
        if name.is_empty() {
            return Err(Error::Invalid("folder name must not be empty".into()));
        }
        let id = uuid::Uuid::new_v4().to_string();
        let now = now_ms();
        let position: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM folders
             WHERE parent_id IS ?1",
            params![parent_id],
            |r| r.get(0),
        )?;
        self.conn.execute(
            "INSERT INTO folders(id, parent_id, name, color, position, created_at, updated_at)
             VALUES (?1, ?2, ?3, NULL, ?4, ?5, ?5)",
            params![id, parent_id, name, position, now],
        )?;
        Ok(Folder {
            id,
            parent_id: parent_id.map(str::to_string),
            name: name.to_string(),
            color: None,
            position,
            created_at: now,
            updated_at: now,
            note_count: 0,
        })
    }

    pub fn rename_folder(&self, id: &str, name: &str) -> Result<()> {
        let name = name.trim();
        if name.is_empty() {
            return Err(Error::Invalid("folder name must not be empty".into()));
        }
        let n = self.conn.execute(
            "UPDATE folders SET name = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, name, now_ms()],
        )?;
        if n == 0 {
            return Err(Error::NotFound(format!("folder {id}")));
        }
        Ok(())
    }

    pub fn set_folder_color(&self, id: &str, color: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE folders SET color = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, color, now_ms()],
        )?;
        Ok(())
    }

    /// Reparents a folder, refusing a move that would put it inside itself.
    pub fn move_folder(&self, id: &str, parent_id: Option<&str>) -> Result<()> {
        if let Some(parent) = parent_id {
            if parent == id || self.is_descendant(parent, id)? {
                return Err(Error::Invalid(
                    "a folder cannot be moved into itself".into(),
                ));
            }
        }
        self.conn.execute(
            "UPDATE folders SET parent_id = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, parent_id, now_ms()],
        )?;
        Ok(())
    }

    fn is_descendant(&self, candidate: &str, ancestor: &str) -> Result<bool> {
        let mut cur = Some(candidate.to_string());
        while let Some(id) = cur {
            if id == ancestor {
                return Ok(true);
            }
            cur = self
                .conn
                .query_row(
                    "SELECT parent_id FROM folders WHERE id = ?1",
                    params![id],
                    |r| r.get::<_, Option<String>>(0),
                )
                .optional()?
                .flatten();
        }
        Ok(false)
    }

    /// Deletes a folder and its subtree. Notes inside are sent to the trash
    /// rather than destroyed, which is what "delete folder" means everywhere
    /// a user has met it before.
    pub fn delete_folder(&self, id: &str) -> Result<()> {
        let mut subtree = vec![id.to_string()];
        let mut frontier = vec![id.to_string()];
        while let Some(parent) = frontier.pop() {
            let mut stmt = self
                .conn
                .prepare("SELECT id FROM folders WHERE parent_id = ?1")?;
            let kids: Vec<String> = stmt
                .query_map(params![parent], |r| r.get(0))?
                .collect::<std::result::Result<_, _>>()?;
            frontier.extend(kids.iter().cloned());
            subtree.extend(kids);
        }
        let placeholders = vec!["?"; subtree.len()].join(",");
        let now = now_ms();
        self.conn.execute(
            &format!(
                "UPDATE notes SET trashed_at = ?1, folder_id = NULL
                 WHERE trashed_at IS NULL AND folder_id IN ({placeholders})"
            ),
            params_from_iter(std::iter::once(now.to_string()).chain(subtree.iter().cloned())),
        )?;
        self.conn
            .execute("DELETE FROM folders WHERE id = ?1", params![id])?;
        Ok(())
    }

    // ------------------------------------------------------------------ notes

    pub fn create_note(&self, folder_id: Option<&str>, doc: Option<Value>) -> Result<Note> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = now_ms();
        let doc = doc.unwrap_or_else(empty_doc);
        let text = doc_to_text(&doc);
        let title = derive_title(&text);
        let preview = derive_preview(&text);
        self.conn.execute(
            "INSERT INTO notes(id, folder_id, title, doc, preview, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
            params![
                id,
                folder_id,
                title,
                serde_json::to_string(&doc)?,
                preview,
                now
            ],
        )?;
        self.reindex(&id, &title, &text)?;
        self.get_note(&id)
    }

    pub fn get_note(&self, id: &str) -> Result<Note> {
        let (doc, ink): (String, String) = self
            .conn
            .query_row(
                "SELECT doc, ink FROM notes WHERE id = ?1",
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .ok_or_else(|| Error::NotFound(format!("note {id}")))?;
        let summary = self.get_summary(id)?;
        Ok(Note {
            summary,
            doc: serde_json::from_str(&doc)?,
            ink: serde_json::from_str(&ink)?,
        })
    }

    fn get_summary(&self, id: &str) -> Result<NoteSummary> {
        let mut summary = self
            .conn
            .query_row(
                "SELECT id, folder_id, title, preview, color, pinned, favorite, locked,
                        created_at, updated_at, trashed_at, ink <> '[]'
                 FROM notes WHERE id = ?1",
                params![id],
                row_to_summary,
            )
            .optional()?
            .ok_or_else(|| Error::NotFound(format!("note {id}")))?;
        summary.tags = self.tags_of(id)?;
        Ok(summary)
    }

    fn tags_of(&self, note_id: &str) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT t.id FROM tags t
             JOIN note_tags nt ON nt.tag_id = t.id
             WHERE nt.note_id = ?1
             ORDER BY t.name COLLATE NOCASE",
        )?;
        let rows = stmt.query_map(params![note_id], |r| r.get(0))?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn update_note(&self, id: &str, patch: NotePatch) -> Result<Note> {
        let exists: bool = self
            .conn
            .query_row("SELECT 1 FROM notes WHERE id = ?1", params![id], |_| {
                Ok(true)
            })
            .optional()?
            .unwrap_or(false);
        if !exists {
            return Err(Error::NotFound(format!("note {id}")));
        }
        let now = now_ms();

        if let Some(doc) = &patch.doc {
            let text = doc_to_text(doc);
            // An explicit title in the same patch wins; otherwise the first
            // line of the body keeps the title in step with the content.
            let title = patch.title.clone().unwrap_or_else(|| derive_title(&text));
            let preview = derive_preview(&text);
            self.conn.execute(
                "UPDATE notes SET doc = ?2, title = ?3, preview = ?4, updated_at = ?5 WHERE id = ?1",
                params![id, serde_json::to_string(doc)?, title, preview, now],
            )?;
            self.reindex(id, &title, &text)?;
        } else if let Some(title) = &patch.title {
            self.conn.execute(
                "UPDATE notes SET title = ?2, updated_at = ?3 WHERE id = ?1",
                params![id, title, now],
            )?;
            let body: String = self
                .conn
                .query_row(
                    "SELECT body FROM notes_fts WHERE note_id = ?1",
                    params![id],
                    |r| r.get(0),
                )
                .optional()?
                .unwrap_or_default();
            self.reindex(id, title, &body)?;
        }

        if let Some(ink) = &patch.ink {
            self.conn.execute(
                "UPDATE notes SET ink = ?2, updated_at = ?3 WHERE id = ?1",
                params![id, serde_json::to_string(ink)?, now],
            )?;
        }
        if let Some(folder_id) = &patch.folder_id {
            self.conn.execute(
                "UPDATE notes SET folder_id = ?2, updated_at = ?3 WHERE id = ?1",
                params![id, folder_id.as_deref(), now],
            )?;
        }
        if let Some(color) = &patch.color {
            self.conn.execute(
                "UPDATE notes SET color = ?2, updated_at = ?3 WHERE id = ?1",
                params![id, color.as_deref(), now],
            )?;
        }
        for (field, value) in [
            ("pinned", patch.pinned),
            ("favorite", patch.favorite),
            ("locked", patch.locked),
        ] {
            if let Some(v) = value {
                self.conn.execute(
                    &format!("UPDATE notes SET {field} = ?2, updated_at = ?3 WHERE id = ?1"),
                    params![id, v as i64, now],
                )?;
            }
        }
        if let Some(tags) = &patch.tags {
            self.set_tags(id, tags)?;
        }
        self.get_note(id)
    }

    fn reindex(&self, note_id: &str, title: &str, body: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM notes_fts WHERE note_id = ?1", params![note_id])?;
        self.conn.execute(
            "INSERT INTO notes_fts(note_id, title, body) VALUES (?1, ?2, ?3)",
            params![note_id, title, body],
        )?;
        Ok(())
    }

    /// Moves a note to the trash. Reversible until `purge_note`.
    pub fn trash_note(&self, id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE notes SET trashed_at = ?2, updated_at = ?2 WHERE id = ?1",
            params![id, now_ms()],
        )?;
        Ok(())
    }

    pub fn restore_note(&self, id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE notes SET trashed_at = NULL, updated_at = ?2 WHERE id = ?1",
            params![id, now_ms()],
        )?;
        Ok(())
    }

    pub fn purge_note(&self, id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM notes_fts WHERE note_id = ?1", params![id])?;
        self.conn
            .execute("DELETE FROM notes WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn empty_trash(&self) -> Result<usize> {
        let ids: Vec<String> = {
            let mut stmt = self
                .conn
                .prepare("SELECT id FROM notes WHERE trashed_at IS NOT NULL")?;
            let rows = stmt.query_map([], |r| r.get(0))?;
            rows.collect::<std::result::Result<_, _>>()?
        };
        for id in &ids {
            self.purge_note(id)?;
        }
        Ok(ids.len())
    }

    pub fn list_notes(&self, scope: &Scope, sort: SortBy) -> Result<Vec<NoteSummary>> {
        let (where_clause, args): (&str, Vec<String>) = match scope {
            Scope::All => ("n.trashed_at IS NULL", vec![]),
            Scope::Unfiled => ("n.trashed_at IS NULL AND n.folder_id IS NULL", vec![]),
            Scope::Favorites => ("n.trashed_at IS NULL AND n.favorite = 1", vec![]),
            Scope::Trash => ("n.trashed_at IS NOT NULL", vec![]),
            Scope::Folder { id } => (
                "n.trashed_at IS NULL AND n.folder_id = ?1",
                vec![id.clone()],
            ),
            Scope::Tag { id } => (
                "n.trashed_at IS NULL AND EXISTS
                 (SELECT 1 FROM note_tags nt WHERE nt.note_id = n.id AND nt.tag_id = ?1)",
                vec![id.clone()],
            ),
        };
        // Pinned notes float to the top of every scope except the trash, where
        // recency of deletion is the only ordering that makes sense.
        // Every ordering ends in a total tiebreak so a list never reshuffles
        // between two reads of an unchanged library.
        let order = match (scope, sort) {
            (Scope::Trash, _) => "n.trashed_at DESC, n.id",
            (_, SortBy::Updated) => "n.pinned DESC, n.updated_at DESC, n.id",
            (_, SortBy::Created) => "n.pinned DESC, n.created_at DESC, n.id",
            (_, SortBy::Title) => "n.pinned DESC, n.title COLLATE NOCASE ASC, n.id",
        };
        let sql = format!(
            "SELECT n.id, n.folder_id, n.title, n.preview, n.color, n.pinned, n.favorite,
                    n.locked, n.created_at, n.updated_at, n.trashed_at, n.ink <> '[]'
             FROM notes n WHERE {where_clause} ORDER BY {order}"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(args.iter()), row_to_summary)?;
        let mut out: Vec<NoteSummary> = rows.collect::<std::result::Result<_, _>>()?;
        for note in &mut out {
            note.tags = self.tags_of(&note.id)?;
        }
        Ok(out)
    }

    /// Full-text search over titles and bodies, newest match first.
    pub fn search(&self, query: &str, scope: &Scope) -> Result<Vec<NoteSummary>> {
        let Some(fts) = to_fts_query(query) else {
            return Ok(vec![]);
        };
        let trash_clause = if matches!(scope, Scope::Trash) {
            "n.trashed_at IS NOT NULL"
        } else {
            "n.trashed_at IS NULL"
        };
        let sql = format!(
            "SELECT n.id, n.folder_id, n.title, n.preview, n.color, n.pinned, n.favorite,
                    n.locked, n.created_at, n.updated_at, n.trashed_at, n.ink <> '[]',
                    snippet(notes_fts, 2, '\u{2039}', '\u{203a}', '\u{2026}', 14)
             FROM notes_fts
             JOIN notes n ON n.id = notes_fts.note_id
             WHERE notes_fts MATCH ?1 AND {trash_clause}
             ORDER BY bm25(notes_fts, 0.0, 8.0, 1.0), n.updated_at DESC
             LIMIT 300"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params![fts], |r| {
            let mut s = row_to_summary(r)?;
            s.snippet = r.get(12)?;
            Ok(s)
        })?;
        let mut out: Vec<NoteSummary> = rows.collect::<std::result::Result<_, _>>()?;
        for note in &mut out {
            note.tags = self.tags_of(&note.id)?;
        }
        Ok(out)
    }

    // ------------------------------------------------------------------- tags

    pub fn list_tags(&self) -> Result<Vec<Tag>> {
        let mut stmt = self.conn.prepare(
            "SELECT t.id, t.name, t.color,
                    (SELECT COUNT(*) FROM note_tags nt JOIN notes n ON n.id = nt.note_id
                      WHERE nt.tag_id = t.id AND n.trashed_at IS NULL)
             FROM tags t ORDER BY t.name COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Tag {
                id: r.get(0)?,
                name: r.get(1)?,
                color: r.get(2)?,
                note_count: r.get(3)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Finds a tag by name or creates it. Tag names are case-insensitively
    /// unique, so typing "Work" when "work" exists reuses the existing tag.
    pub fn ensure_tag(&self, name: &str) -> Result<Tag> {
        let name = name.trim().trim_start_matches('#');
        if name.is_empty() {
            return Err(Error::Invalid("tag name must not be empty".into()));
        }
        if let Some(tag) = self
            .conn
            .query_row(
                "SELECT id, name, color FROM tags WHERE name = ?1 COLLATE NOCASE",
                params![name],
                |r| {
                    Ok(Tag {
                        id: r.get(0)?,
                        name: r.get(1)?,
                        color: r.get(2)?,
                        note_count: 0,
                    })
                },
            )
            .optional()?
        {
            return Ok(tag);
        }
        let id = uuid::Uuid::new_v4().to_string();
        self.conn.execute(
            "INSERT INTO tags(id, name, color) VALUES (?1, ?2, NULL)",
            params![id, name],
        )?;
        Ok(Tag {
            id,
            name: name.to_string(),
            color: None,
            note_count: 0,
        })
    }

    pub fn delete_tag(&self, id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM tags WHERE id = ?1", params![id])?;
        Ok(())
    }

    fn set_tags(&self, note_id: &str, tag_ids: &[String]) -> Result<()> {
        self.conn
            .execute("DELETE FROM note_tags WHERE note_id = ?1", params![note_id])?;
        for tag_id in tag_ids {
            self.conn.execute(
                "INSERT OR IGNORE INTO note_tags(note_id, tag_id) VALUES (?1, ?2)",
                params![note_id, tag_id],
            )?;
        }
        Ok(())
    }

    // ------------------------------------------------------------ attachments

    /// Writes a blob into the content-addressed store and records it. Two
    /// notes pasting the same image share one file on disk.
    pub fn put_attachment(
        &self,
        note_id: Option<&str>,
        name: &str,
        mime: &str,
        bytes: &[u8],
    ) -> Result<(Attachment, PathBuf)> {
        let sha = hex(&Sha256::digest(bytes));
        let ext = Path::new(name)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| format!(".{}", e.to_lowercase()))
            .unwrap_or_default();
        let dir = self.root.join("attachments").join(&sha[..2]);
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{sha}{ext}"));
        if !path.exists() {
            std::fs::write(&path, bytes)?;
        }
        let att = Attachment {
            id: uuid::Uuid::new_v4().to_string(),
            note_id: note_id.map(str::to_string),
            mime: mime.to_string(),
            name: name.to_string(),
            size: bytes.len() as i64,
            sha256: sha,
            created_at: now_ms(),
        };
        self.conn.execute(
            "INSERT INTO attachments(id, note_id, mime, name, size, sha256, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                att.id,
                att.note_id,
                att.mime,
                att.name,
                att.size,
                att.sha256,
                att.created_at
            ],
        )?;
        Ok((att, path))
    }

    pub fn attachment_path(&self, id: &str) -> Result<PathBuf> {
        let (sha, name): (String, String) = self
            .conn
            .query_row(
                "SELECT sha256, name FROM attachments WHERE id = ?1",
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .ok_or_else(|| Error::NotFound(format!("attachment {id}")))?;
        let ext = Path::new(&name)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| format!(".{}", e.to_lowercase()))
            .unwrap_or_default();
        Ok(self
            .root
            .join("attachments")
            .join(&sha[..2])
            .join(format!("{sha}{ext}")))
    }

    pub fn stats(&self) -> Result<(i64, i64, i64)> {
        let notes: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM notes WHERE trashed_at IS NULL",
            [],
            |r| r.get(0),
        )?;
        let trashed: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM notes WHERE trashed_at IS NOT NULL",
            [],
            |r| r.get(0),
        )?;
        let folders: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM folders", [], |r| r.get(0))?;
        Ok((notes, trashed, folders))
    }
}

fn row_to_summary(r: &Row<'_>) -> rusqlite::Result<NoteSummary> {
    Ok(NoteSummary {
        id: r.get(0)?,
        folder_id: r.get(1)?,
        title: r.get(2)?,
        preview: r.get(3)?,
        color: r.get(4)?,
        pinned: r.get::<_, i64>(5)? != 0,
        favorite: r.get::<_, i64>(6)? != 0,
        locked: r.get::<_, i64>(7)? != 0,
        created_at: r.get(8)?,
        updated_at: r.get(9)?,
        trashed_at: r.get(10)?,
        has_ink: r.get::<_, i64>(11)? != 0,
        tags: vec![],
        snippet: None,
    })
}

pub fn empty_doc() -> Value {
    serde_json::json!({ "type": "doc", "content": [{ "type": "paragraph" }] })
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Turns whatever the user typed into a safe FTS5 MATCH expression: every
/// token is quoted (so punctuation can't become syntax) and given a prefix
/// wildcard, which is what makes search feel like it responds as you type.
fn to_fts_query(input: &str) -> Option<String> {
    let terms: Vec<String> = input
        .split(|c: char| !c.is_alphanumeric() && c != '_' && c != '\'')
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{}\"*", t.replace('"', "")))
        .collect();
    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" "))
    }
}
