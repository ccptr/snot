export interface Folder {
  id: string;
  parentId: string | null;
  name: string;
  color: string | null;
  position: number;
  createdAt: number;
  updatedAt: number;
  noteCount: number;
}

export interface Tag {
  id: string;
  name: string;
  color: string | null;
  noteCount: number;
}

export interface NoteSummary {
  id: string;
  folderId: string | null;
  title: string;
  preview: string;
  color: string | null;
  pinned: boolean;
  favorite: boolean;
  locked: boolean;
  createdAt: number;
  updatedAt: number;
  trashedAt: number | null;
  tags: string[];
  snippet?: string;
}

export interface Note extends NoteSummary {
  doc: unknown;
}

/** Omitted fields are left alone; an explicit `null` clears the field. */
export interface NotePatch {
  title?: string;
  doc?: unknown;
  folderId?: string | null;
  color?: string | null;
  pinned?: boolean;
  favorite?: boolean;
  locked?: boolean;
  tags?: string[];
}

export type Scope =
  | { kind: "all" }
  | { kind: "unfiled" }
  | { kind: "favorites" }
  | { kind: "trash" }
  | { kind: "folder"; id: string }
  | { kind: "tag"; id: string };

export type SortBy = "updated" | "created" | "title";

export interface StoredAttachment {
  id: string;
  noteId: string | null;
  mime: string;
  name: string;
  size: number;
  sha256: string;
  createdAt: number;
  path: string;
}

export interface Stats {
  notes: number;
  trashed: number;
  folders: number;
}
