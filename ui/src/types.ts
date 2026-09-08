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
  hasInk: boolean;
  hasBackground: boolean;
  snippet?: string;
}

export interface Note extends NoteSummary {
  doc: unknown;
  /** Page handwriting: an array of vector strokes drawn across the note. */
  ink: unknown;
  /** What the page is written on: null, or an imported document descriptor. */
  background: unknown;
}

/** Omitted fields are left alone; an explicit `null` clears the field. */
export interface NotePatch {
  title?: string;
  doc?: unknown;
  ink?: unknown;
  background?: unknown;
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

/** One file an import could not read at all, and why. */
export interface ImportFailure {
  path: string;
  reason: string;
}

/** What an import did, in the terms the user cares about. */
export interface ImportSummary {
  notes: number;
  folders: number;
  attachments: number;
  /** Files kept as attachments because nothing could be read out of them. */
  unconverted: number;
  failures: ImportFailure[];
}

export interface Stats {
  notes: number;
  trashed: number;
  folders: number;
}
