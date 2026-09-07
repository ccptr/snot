import { invoke } from "@tauri-apps/api/core";
import type {
  Folder, Note, NotePatch, NoteSummary, Scope, SortBy, Stats, StoredAttachment, Tag,
} from "./types";

/**
 * Every backend call the UI is allowed to make. Keeping them in one typed
 * module means a renamed Rust command breaks the build rather than the app.
 */
export const api = {
  libraryRoot: () => invoke<string>("library_root"),
  stats: () => invoke<Stats>("stats"),

  listFolders: () => invoke<Folder[]>("list_folders"),
  createFolder: (name: string, parentId: string | null = null) =>
    invoke<Folder>("create_folder", { name, parentId }),
  renameFolder: (id: string, name: string) => invoke<void>("rename_folder", { id, name }),
  setFolderColor: (id: string, color: string | null) =>
    invoke<void>("set_folder_color", { id, color }),
  moveFolder: (id: string, parentId: string | null) =>
    invoke<void>("move_folder", { id, parentId }),
  deleteFolder: (id: string) => invoke<void>("delete_folder", { id }),

  listNotes: (scope: Scope, sort: SortBy) => invoke<NoteSummary[]>("list_notes", { scope, sort }),
  searchNotes: (query: string, scope: Scope) =>
    invoke<NoteSummary[]>("search_notes", { query, scope }),
  getNote: (id: string) => invoke<Note>("get_note", { id }),
  createNote: (folderId: string | null, doc?: unknown) =>
    invoke<Note>("create_note", { folderId, doc: doc ?? null }),
  updateNote: (id: string, patch: NotePatch) => invoke<Note>("update_note", { id, patch }),
  trashNote: (id: string) => invoke<void>("trash_note", { id }),
  restoreNote: (id: string) => invoke<void>("restore_note", { id }),
  purgeNote: (id: string) => invoke<void>("purge_note", { id }),
  emptyTrash: () => invoke<number>("empty_trash"),

  listTags: () => invoke<Tag[]>("list_tags"),
  ensureTag: (name: string) => invoke<Tag>("ensure_tag", { name }),
  deleteTag: (id: string) => invoke<void>("delete_tag", { id }),

  putAttachment: (noteId: string | null, name: string, mime: string | null, bytes: Uint8Array) =>
    invoke<StoredAttachment>("put_attachment", { noteId, name, mime, bytes: Array.from(bytes) }),
  attachmentPath: (id: string) => invoke<string>("attachment_path", { id }),

  importPdf: (path: string, folderId: string | null) =>
    invoke<Note>("import_pdf", { path, folderId }),

  exportMarkdown: (id: string) => invoke<string>("export_markdown", { id }),
  writeTextFile: (path: string, contents: string) =>
    invoke<void>("write_text_file", { path, contents }),
};
