import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { readFile } from "@tauri-apps/plugin-fs";

import { api } from "./api";
import { button, clear, debounce, el, formatDate, icon } from "./dom";
import { createEditor, type NoteEditor } from "./editor";
import { askText, confirmAction, menu, toast } from "./modal";
import type { Folder, Note, NoteSummary, Scope, SortBy, Tag } from "./types";

type Theme = "system" | "light" | "dark";

const SCOPE_LABELS: Record<string, string> = {
  all: "All notes",
  favorites: "Favourites",
  unfiled: "Unfiled",
  trash: "Trash",
};

/**
 * The name to give an imported file. A desktop path ends in a filename; an
 * Android content:// URI often does not, and then the note is named for what
 * it is until the first line of writing renames it.
 */
function fileNameOf(picked: string): string {
  const tail = decodeURIComponent(picked).split(/[/\\]/).pop() ?? "";
  return /\.pdf$/i.test(tail) ? tail : "Imported document.pdf";
}

function scopeKey(scope: Scope): string {
  return "id" in scope ? `${scope.kind}:${scope.id}` : scope.kind;
}

export class App {
  private scope: Scope = { kind: "all" };
  private sort: SortBy = "updated";
  private query = "";
  private folders: Folder[] = [];
  private tags: Tag[] = [];
  private notes: NoteSummary[] = [];
  private current: Note | null = null;
  private editor: NoteEditor | null = null;

  private root: HTMLElement;
  private sidebarNav = el("nav", { class: "sidebar-nav" });
  private listEl = el("div", { class: "note-list", attrs: { role: "list" } });
  private listTitle = el("h1", { class: "list-title", text: "All notes" });
  private listCount = el("span", { class: "list-count" });
  private searchInput = el("input", {
    class: "search-input",
    attrs: { type: "search", placeholder: "Search notes", "aria-label": "Search notes" },
  });
  private editorMount = el("div", { class: "editor-mount" });
  private editorHeader = el("header", { class: "editor-header" });
  private toolbarSlot = el("div", { class: "toolbar-slot" });
  private tagBar = el("div", { class: "tag-bar" });
  private saveState = el("span", { class: "save-state", text: "" });
  private emptyState = el("div", { class: "empty-state" });

  private saveSoon = debounce(() => void this.persist(), 700);

  constructor(mount: HTMLElement) {
    this.root = el("div", { class: "app", data: { pane: "list" } });
    mount.appendChild(this.root);
    this.build();
    this.applyTheme(this.storedTheme());
    this.bindShortcuts();
  }

  async start(): Promise<void> {
    await this.reloadSidebar();
    await this.reloadList();
    if (this.notes[0]) await this.open(this.notes[0].id);
    else this.showEmptyEditor();
  }

  // ------------------------------------------------------------- layout

  private build(): void {
    const sidebar = el("aside", { class: "sidebar" },
      el("div", { class: "brand" },
        el("span", { class: "brand-mark" }, icon("pen", 18)),
        el("span", { class: "brand-name", text: "Snot" }),
      ),
      this.sidebarNav,
      el("div", { class: "sidebar-foot" },
        button({
          label: "New folder", icon: "plus", class: "btn ghost wide",
          onClick: () => void this.newFolder(),
        }),
        button({
          label: "Theme", icon: "moon", class: "btn ghost icon-only", showLabel: false,
          onClick: (ev) => this.themeMenu(ev.currentTarget as HTMLElement),
        }),
      ),
    );

    const listPane = el("section", { class: "list-pane" },
      el("header", { class: "list-header" },
        button({
          label: "Menu", icon: "sidebar", class: "btn ghost icon-only only-narrow",
          showLabel: false,
          onClick: () => this.root.classList.toggle("sidebar-open"),
        }),
        el("div", { class: "list-heading" }, this.listTitle, this.listCount),
        button({
          label: "Sort", icon: "sort", class: "btn ghost icon-only", showLabel: false,
          onClick: (ev) => this.sortMenu(ev.currentTarget as HTMLElement),
        }),
        button({
          label: "Import a PDF", icon: "import", class: "btn ghost icon-only", showLabel: false,
          onClick: () => void this.importPdf(),
        }),
        button({
          label: "New note", icon: "plus", class: "btn primary icon-only", showLabel: false,
          onClick: () => void this.newNote(),
        }),
      ),
      el("div", { class: "search-row" }, icon("search", 16), this.searchInput),
      this.listEl,
    );

    const editorPane = el("section", { class: "editor-pane" },
      this.editorHeader, this.toolbarSlot, this.editorMount, this.tagBar, this.emptyState,
    );

    this.root.append(sidebar, listPane, editorPane);
    this.root.appendChild(el("div", {
      class: "scrim",
      on: { click: () => this.root.classList.remove("sidebar-open") },
    }));

    const runSearch = debounce(() => void this.reloadList(), 180);
    this.searchInput.addEventListener("input", () => {
      this.query = this.searchInput.value;
      runSearch();
    });
  }

  private bindShortcuts(): void {
    window.addEventListener("keydown", (ev) => {
      const mod = ev.ctrlKey || ev.metaKey;
      if (mod && ev.key.toLowerCase() === "n") {
        ev.preventDefault();
        void this.newNote();
      } else if (mod && ev.key.toLowerCase() === "f") {
        ev.preventDefault();
        this.searchInput.focus();
        this.searchInput.select();
      } else if (mod && ev.key.toLowerCase() === "s") {
        ev.preventDefault();
        this.saveSoon.flush();
      } else if (ev.key === "Escape") {
        if (this.root.classList.contains("sidebar-open")) {
          this.root.classList.remove("sidebar-open");
        } else if (this.root.dataset.pane === "editor") {
          this.root.dataset.pane = "list";
        }
      }
    });
  }

  // ------------------------------------------------------------ sidebar

  private async reloadSidebar(): Promise<void> {
    [this.folders, this.tags] = await Promise.all([api.listFolders(), api.listTags()]);
    this.renderSidebar();
  }

  private renderSidebar(): void {
    clear(this.sidebarNav);
    const active = scopeKey(this.scope);

    for (const kind of ["all", "favorites", "unfiled", "trash"] as const) {
      this.sidebarNav.appendChild(this.navRow({
        label: SCOPE_LABELS[kind]!,
        iconName: { all: "note", favorites: "star", unfiled: "folder", trash: "trash" }[kind],
        scope: { kind } as Scope,
        active: active === kind,
      }));
    }

    this.sidebarNav.appendChild(el("div", { class: "nav-section", text: "Folders" }));
    const byParent = new Map<string | null, Folder[]>();
    for (const f of this.folders) {
      const list = byParent.get(f.parentId) ?? [];
      list.push(f);
      byParent.set(f.parentId, list);
    }
    const renderLevel = (parentId: string | null, depth: number) => {
      for (const folder of byParent.get(parentId) ?? []) {
        const row = this.navRow({
          label: folder.name,
          iconName: "folder",
          scope: { kind: "folder", id: folder.id },
          active: active === `folder:${folder.id}`,
          count: folder.noteCount,
          depth,
          onMenu: (anchor) => this.folderMenu(folder, anchor),
        });
        this.makeDropTarget(row, folder.id);
        this.sidebarNav.appendChild(row);
        renderLevel(folder.id, depth + 1);
      }
    };
    renderLevel(null, 0);
    if (this.folders.length === 0) {
      this.sidebarNav.appendChild(el("p", { class: "nav-hint", text: "No folders yet" }));
    }

    if (this.tags.length) {
      this.sidebarNav.appendChild(el("div", { class: "nav-section", text: "Tags" }));
      for (const tag of this.tags) {
        this.sidebarNav.appendChild(this.navRow({
          label: `#${tag.name}`,
          iconName: "tag",
          scope: { kind: "tag", id: tag.id },
          active: active === `tag:${tag.id}`,
          count: tag.noteCount,
          onMenu: (anchor) => menu(anchor, [{
            label: "Delete tag", danger: true,
            onSelect: () => void this.deleteTag(tag),
          }]),
        }));
      }
    }
  }

  private navRow(opts: {
    label: string;
    iconName: string;
    scope: Scope;
    active: boolean;
    count?: number;
    depth?: number;
    onMenu?: (anchor: HTMLElement) => void;
  }): HTMLElement {
    const row = el("div", {
      class: `nav-row${opts.active ? " active" : ""}`,
      style: { paddingLeft: `${12 + (opts.depth ?? 0) * 14}px` },
    });
    const main = el("button", {
      class: "nav-main",
      attrs: { type: "button" },
      on: { click: () => void this.setScope(opts.scope) },
    }, icon(opts.iconName, 17), el("span", { class: "nav-label", text: opts.label }));
    row.appendChild(main);
    if (opts.count !== undefined && opts.count > 0) {
      row.appendChild(el("span", { class: "nav-count", text: String(opts.count) }));
    }
    if (opts.onMenu) {
      const more = button({
        label: "More", icon: "more", class: "btn ghost row-more", showLabel: false,
        onClick: (ev) => {
          ev.stopPropagation();
          opts.onMenu!(ev.currentTarget as HTMLElement);
        },
      });
      row.appendChild(more);
    }
    return row;
  }

  private makeDropTarget(row: HTMLElement, folderId: string | null): void {
    row.addEventListener("dragover", (ev) => {
      ev.preventDefault();
      row.classList.add("drop");
    });
    row.addEventListener("dragleave", () => row.classList.remove("drop"));
    row.addEventListener("drop", (ev) => {
      ev.preventDefault();
      row.classList.remove("drop");
      const noteId = ev.dataTransfer?.getData("text/snot-note");
      if (noteId) void this.moveNote(noteId, folderId);
    });
  }

  private async setScope(scope: Scope): Promise<void> {
    this.saveSoon.flush();
    this.scope = scope;
    this.query = "";
    this.searchInput.value = "";
    this.root.classList.remove("sidebar-open");
    this.renderSidebar();
    await this.reloadList();
  }

  private scopeLabel(): string {
    const scope = this.scope;
    if (scope.kind === "folder") {
      return this.folders.find((f) => f.id === scope.id)?.name ?? "Folder";
    }
    if (scope.kind === "tag") {
      return `#${this.tags.find((t) => t.id === scope.id)?.name ?? "tag"}`;
    }
    return SCOPE_LABELS[scope.kind] ?? "Notes";
  }

  // --------------------------------------------------------------- list

  private async reloadList(): Promise<void> {
    this.notes = this.query.trim()
      ? await api.searchNotes(this.query, this.scope)
      : await api.listNotes(this.scope, this.sort);
    this.listTitle.textContent = this.query.trim() ? `Results for “${this.query.trim()}”` : this.scopeLabel();
    this.listCount.textContent = this.notes.length
      ? `${this.notes.length} ${this.notes.length === 1 ? "note" : "notes"}`
      : "";
    this.renderList();
  }

  private renderList(): void {
    clear(this.listEl);
    if (this.notes.length === 0) {
      this.listEl.appendChild(el("div", { class: "list-empty" },
        icon("note", 28),
        el("p", { text: this.query.trim() ? "Nothing matched." : "No notes here yet." }),
        this.scope.kind !== "trash" && button({
          label: "New note", icon: "plus", class: "btn primary",
          onClick: () => void this.newNote(),
        }),
      ));
      return;
    }
    if (this.scope.kind === "trash") {
      this.listEl.appendChild(el("div", { class: "trash-banner" },
        el("span", { text: "Deleted notes stay here until you empty the trash." }),
        button({
          label: "Empty trash", class: "btn danger small", icon: "trash", showLabel: true,
          onClick: () => void this.emptyTrash(),
        }),
      ));
    }
    for (const note of this.notes) this.listEl.appendChild(this.noteCard(note));
  }

  private noteCard(note: NoteSummary): HTMLElement {
    const card = el("article", {
      class: `note-card${this.current?.id === note.id ? " active" : ""}`,
      attrs: { role: "listitem", tabindex: "0", draggable: "true" },
      data: { id: note.id },
      on: {
        click: () => void this.open(note.id),
        keydown: (ev) => {
          if (ev.key === "Enter" || ev.key === " ") {
            ev.preventDefault();
            void this.open(note.id);
          }
        },
        dragstart: (ev) => {
          (ev as DragEvent).dataTransfer?.setData("text/snot-note", note.id);
        },
      },
    });
    if (note.color) card.style.borderLeftColor = note.color;

    const heading = el("div", { class: "card-head" },
      note.pinned ? icon("pin", 14) : null,
      el("h3", { class: "card-title", text: note.title || "Untitled note" }),
      note.hasInk ? icon("pen", 14) : null,
      note.favorite ? icon("star", 14) : null,
    );
    const preview = el("p", { class: "card-preview" });
    if (note.snippet) {
      // The backend marks matches with ‹…›; render them as highlights without
      // ever putting backend text through innerHTML.
      for (const part of note.snippet.split(/(‹[^›]*›)/)) {
        if (!part) continue;
        if (part.startsWith("‹")) {
          preview.appendChild(el("mark", { text: part.slice(1, -1) }));
        } else {
          preview.appendChild(document.createTextNode(part));
        }
      }
    } else {
      preview.textContent = note.preview || "No additional text";
    }

    const meta = el("div", { class: "card-meta" },
      el("time", { text: formatDate(note.trashedAt ?? note.updatedAt) }),
    );
    for (const tagId of note.tags) {
      const tag = this.tags.find((t) => t.id === tagId);
      if (tag) meta.appendChild(el("span", { class: "chip", text: `#${tag.name}` }));
    }

    const more = button({
      label: "Note actions", icon: "more", class: "btn ghost card-more", showLabel: false,
      onClick: (ev) => {
        ev.stopPropagation();
        this.noteMenu(note, ev.currentTarget as HTMLElement);
      },
    });

    card.append(heading, preview, meta, more);
    return card;
  }

  private noteMenu(note: NoteSummary, anchor: HTMLElement): void {
    if (note.trashedAt) {
      menu(anchor, [
        { label: "Restore", onSelect: () => void this.restoreNote(note.id) },
        { label: "Delete forever", danger: true, onSelect: () => void this.purgeNote(note.id) },
      ]);
      return;
    }
    menu(anchor, [
      { label: note.pinned ? "Unpin" : "Pin to top",
        onSelect: () => void this.patch(note.id, { pinned: !note.pinned }) },
      { label: note.favorite ? "Remove favourite" : "Add to favourites",
        onSelect: () => void this.patch(note.id, { favorite: !note.favorite }) },
      "sep",
      { label: "Move to folder…", onSelect: () => this.folderPicker(note.id, anchor) },
      { label: "Export as Markdown…", onSelect: () => void this.exportNote(note) },
      "sep",
      { label: "Move to trash", danger: true, onSelect: () => void this.trashNote(note.id) },
    ]);
  }

  private folderPicker(noteId: string, anchor: HTMLElement): void {
    menu(anchor, [
      { label: "No folder", onSelect: () => void this.moveNote(noteId, null) },
      ...this.folders.map((f) => ({
        label: f.name,
        onSelect: () => void this.moveNote(noteId, f.id),
      })),
    ]);
  }

  private sortMenu(anchor: HTMLElement): void {
    const pick = (sort: SortBy) => () => {
      this.sort = sort;
      void this.reloadList();
    };
    menu(anchor, [
      { label: "Recently updated", onSelect: pick("updated") },
      { label: "Recently created", onSelect: pick("created") },
      { label: "Title A–Z", onSelect: pick("title") },
    ]);
  }

  private themeMenu(anchor: HTMLElement): void {
    const pick = (theme: Theme) => () => {
      localStorage.setItem("snot.theme", theme);
      this.applyTheme(theme);
    };
    menu(anchor, [
      { label: "Match system", onSelect: pick("system") },
      { label: "Light", onSelect: pick("light") },
      { label: "Dark", onSelect: pick("dark") },
    ]);
  }

  private storedTheme(): Theme {
    const stored = localStorage.getItem("snot.theme");
    return stored === "light" || stored === "dark" ? stored : "system";
  }

  private applyTheme(theme: Theme): void {
    if (theme === "system") document.documentElement.removeAttribute("data-theme");
    else document.documentElement.setAttribute("data-theme", theme);
  }

  // -------------------------------------------------------------- notes

  private async newNote(): Promise<void> {
    this.saveSoon.flush();
    const folderId = this.scope.kind === "folder" ? this.scope.id : null;
    const note = await api.createNote(folderId);
    if (this.scope.kind === "trash") this.scope = { kind: "all" };
    await this.reloadList();
    await this.open(note.id);
    this.editor?.editor.commands.focus("start");
  }

  /** Brings a PDF in as a note: the document becomes the page to write on. */
  private async importPdf(): Promise<void> {
    this.saveSoon.flush();
    const picked = await openDialog({
      multiple: false,
      filters: [{ name: "PDF", extensions: ["pdf"] }],
    });
    if (typeof picked !== "string") return;
    try {
      // Read here rather than in the backend: Android's picker returns a
      // content:// URI with no filesystem path behind it, and only the
      // platform side can open one.
      const bytes = await readFile(picked);
      const folderId = this.scope.kind === "folder" ? this.scope.id : null;
      const note = await api.importPdf(fileNameOf(picked), bytes, folderId);
      if (this.scope.kind === "trash") this.scope = { kind: "all" };
      await this.reloadList();
      await this.open(note.id);
      toast("Imported — pick up the pen to annotate it");
    } catch (err) {
      toast(`Could not import that PDF: ${err}`, "error");
    }
  }

  private async open(id: string): Promise<void> {
    this.saveSoon.flush();
    try {
      this.current = await api.getNote(id);
    } catch (err) {
      toast(String(err), "error");
      return;
    }
    this.root.dataset.pane = "editor";
    this.emptyState.replaceChildren();
    this.emptyState.classList.remove("visible");

    if (!this.editor) {
      const touched = () => {
        this.saveState.textContent = "Unsaved";
        this.saveSoon();
      };
      this.editor = createEditor(this.editorMount, {
        noteId: () => this.current?.id ?? null,
        onChange: touched,
        onInkChange: touched,
      });
      this.toolbarSlot.appendChild(this.editor.toolbar);
    }
    this.editor.editor.commands.setContent(this.current.doc as never, { emitUpdate: false });
    this.editor.setInk(this.current.ink);
    await this.editor.setBackground(this.current.background);
    this.editor.editor.setEditable(!this.current.trashedAt);
    this.saveState.textContent = "Saved";
    this.renderEditorChrome();
    for (const card of Array.from(this.listEl.querySelectorAll<HTMLElement>(".note-card"))) {
      card.classList.toggle("active", card.dataset.id === id);
    }
  }

  private showEmptyEditor(): void {
    this.root.dataset.pane = "list";
    this.emptyState.classList.add("visible");
    this.emptyState.replaceChildren(
      icon("note", 34),
      el("h2", { text: "Nothing open" }),
      el("p", { text: "Pick a note on the left, or start a new one." }),
      button({ label: "New note", icon: "plus", class: "btn primary",
        onClick: () => void this.newNote() }),
    );
  }

  private renderEditorChrome(): void {
    const note = this.current;
    clear(this.editorHeader);
    clear(this.tagBar);
    if (!note) return;

    this.editorHeader.append(
      button({
        label: "Back", icon: "back", class: "btn ghost icon-only only-stacked", showLabel: false,
        onClick: () => (this.root.dataset.pane = "list"),
      }),
      el("div", { class: "editor-meta" },
        el("h2", { class: "editor-title", text: note.title || "Untitled note" }),
        el("span", { class: "editor-sub", text: `Edited ${formatDate(note.updatedAt)}` }),
      ),
      this.saveState,
    );

    if (note.trashedAt) {
      this.editorHeader.append(
        button({ label: "Restore", icon: "restore", class: "btn ghost",
          onClick: () => void this.restoreNote(note.id) }),
        button({ label: "Delete forever", icon: "trash", class: "btn danger",
          onClick: () => void this.purgeNote(note.id) }),
      );
      this.tagBar.appendChild(el("span", {
        class: "tag-hint", text: "This note is in the trash and cannot be edited.",
      }));
      return;
    }

    this.editorHeader.append(
      button({
        label: note.pinned ? "Unpin" : "Pin", icon: "pin",
        class: `btn ghost icon-only${note.pinned ? " active" : ""}`, showLabel: false,
        onClick: () => void this.patch(note.id, { pinned: !note.pinned }),
      }),
      button({
        label: note.favorite ? "Remove favourite" : "Favourite", icon: "star",
        class: `btn ghost icon-only${note.favorite ? " active" : ""}`, showLabel: false,
        onClick: () => void this.patch(note.id, { favorite: !note.favorite }),
      }),
      button({
        label: "Note actions", icon: "more", class: "btn ghost icon-only", showLabel: false,
        onClick: (ev) => this.noteMenu(note, ev.currentTarget as HTMLElement),
      }),
    );

    for (const tagId of note.tags) {
      const tag = this.tags.find((t) => t.id === tagId);
      if (!tag) continue;
      this.tagBar.appendChild(el("span", { class: "chip removable" },
        el("span", { text: `#${tag.name}` }),
        el("button", {
          class: "chip-x", text: "×",
          attrs: { type: "button", "aria-label": `Remove ${tag.name}` },
          on: { click: () => void this.setTags(note.tags.filter((t) => t !== tagId)) },
        }),
      ));
    }
    this.tagBar.appendChild(button({
      label: "Add tag", icon: "tag", class: "btn ghost small",
      onClick: () => void this.addTag(),
    }));

    const folder = this.folders.find((f) => f.id === note.folderId);
    this.tagBar.appendChild(button({
      label: folder ? folder.name : "No folder", icon: "folder", class: "btn ghost small folder-pill",
      onClick: (ev) => this.folderPicker(note.id, ev.currentTarget as HTMLElement),
    }));
  }

  private async persist(): Promise<void> {
    if (!this.current || !this.editor || this.current.trashedAt) return;
    const doc = this.editor.editor.getJSON();
    const ink = this.editor.ink.getStrokes();
    this.saveState.textContent = "Saving…";
    try {
      const updated = await api.updateNote(this.current.id, { doc, ink });
      this.current = updated;
      this.saveState.textContent = "Saved";
      this.refreshCard(updated);
      const titleEl = this.editorHeader.querySelector<HTMLElement>(".editor-title");
      if (titleEl) titleEl.textContent = updated.title || "Untitled note";
    } catch (err) {
      this.saveState.textContent = "Not saved";
      toast(`Could not save: ${err}`, "error");
    }
  }

  /** Updates one card in place so a keystroke never rebuilds the whole list. */
  private refreshCard(note: NoteSummary): void {
    const index = this.notes.findIndex((n) => n.id === note.id);
    if (index === -1) return;
    this.notes[index] = { ...note, snippet: undefined };
    const existing = this.listEl.querySelector<HTMLElement>(`.note-card[data-id="${note.id}"]`);
    if (existing) existing.replaceWith(this.noteCard(this.notes[index]!));
  }

  private async patch(id: string, patch: Parameters<typeof api.updateNote>[1]): Promise<void> {
    const updated = await api.updateNote(id, patch);
    if (this.current?.id === id) this.current = updated;
    await this.reloadList();
    if (this.current?.id === id) this.renderEditorChrome();
  }

  private async moveNote(id: string, folderId: string | null): Promise<void> {
    await api.updateNote(id, { folderId });
    await this.reloadSidebar();
    await this.reloadList();
    if (this.current?.id === id) {
      this.current = await api.getNote(id);
      this.renderEditorChrome();
    }
    toast(folderId ? "Moved" : "Removed from folder");
  }

  private async trashNote(id: string): Promise<void> {
    this.saveSoon.cancel();
    await api.trashNote(id);
    if (this.current?.id === id) {
      this.current = null;
      this.editor?.editor.commands.clearContent();
      this.showEmptyEditor();
    }
    await this.reloadSidebar();
    await this.reloadList();
    toast("Moved to trash");
  }

  private async restoreNote(id: string): Promise<void> {
    await api.restoreNote(id);
    await this.reloadSidebar();
    await this.reloadList();
    if (this.current?.id === id) await this.open(id);
    toast("Restored");
  }

  private async purgeNote(id: string): Promise<void> {
    const ok = await confirmAction(
      "Delete forever?",
      "This note will be gone for good. There is no undo.",
      { confirmLabel: "Delete forever", danger: true },
    );
    if (!ok) return;
    await api.purgeNote(id);
    if (this.current?.id === id) {
      this.current = null;
      this.showEmptyEditor();
    }
    await this.reloadList();
  }

  private async emptyTrash(): Promise<void> {
    const ok = await confirmAction(
      "Empty the trash?",
      "Every note in the trash will be deleted for good.",
      { confirmLabel: "Empty trash", danger: true },
    );
    if (!ok) return;
    const count = await api.emptyTrash();
    this.current = null;
    this.showEmptyEditor();
    await this.reloadList();
    toast(`Deleted ${count} ${count === 1 ? "note" : "notes"}`);
  }

  private async exportNote(note: NoteSummary): Promise<void> {
    const markdown = await api.exportMarkdown(note.id);
    const suggested = `${(note.title || "note").replace(/[^\w\- ]+/g, "").trim() || "note"}.md`;
    const path = await saveDialog({
      defaultPath: suggested,
      filters: [{ name: "Markdown", extensions: ["md"] }],
    });
    if (!path) return;
    await api.writeTextFile(path, markdown);
    toast("Exported");
  }

  // --------------------------------------------------------- tags/folders

  private async addTag(): Promise<void> {
    if (!this.current) return;
    const name = await askText("Add tag", { placeholder: "work, recipes, 2026…", confirmLabel: "Add" });
    if (!name) return;
    const tag = await api.ensureTag(name);
    if (this.current.tags.includes(tag.id)) return;
    await this.setTags([...this.current.tags, tag.id]);
  }

  private async setTags(tags: string[]): Promise<void> {
    if (!this.current) return;
    this.current = await api.updateNote(this.current.id, { tags });
    await this.reloadSidebar();
    await this.reloadList();
    this.renderEditorChrome();
  }

  private async deleteTag(tag: Tag): Promise<void> {
    const ok = await confirmAction(
      `Delete #${tag.name}?`,
      "The tag is removed from every note. The notes themselves are untouched.",
      { confirmLabel: "Delete tag", danger: true },
    );
    if (!ok) return;
    await api.deleteTag(tag.id);
    if (this.scope.kind === "tag") this.scope = { kind: "all" };
    await this.reloadSidebar();
    await this.reloadList();
    if (this.current) this.current = await api.getNote(this.current.id);
    this.renderEditorChrome();
  }

  private async newFolder(): Promise<void> {
    const name = await askText("New folder", { placeholder: "Folder name", confirmLabel: "Create" });
    if (!name) return;
    const folder = await api.createFolder(name);
    await this.reloadSidebar();
    await this.setScope({ kind: "folder", id: folder.id });
  }

  private folderMenu(folder: Folder, anchor: HTMLElement): void {
    menu(anchor, [
      { label: "Rename…", onSelect: () => void this.renameFolder(folder) },
      { label: "New subfolder…", onSelect: () => void this.newSubfolder(folder) },
      ...(folder.parentId
        ? [{ label: "Move to top level", onSelect: () => void this.reparent(folder.id, null) }]
        : []),
      "sep" as const,
      { label: "Delete folder", danger: true, onSelect: () => void this.deleteFolder(folder) },
    ]);
  }

  private async renameFolder(folder: Folder): Promise<void> {
    const name = await askText("Rename folder", { initial: folder.name });
    if (!name) return;
    await api.renameFolder(folder.id, name);
    await this.reloadSidebar();
    await this.reloadList();
  }

  private async newSubfolder(parent: Folder): Promise<void> {
    const name = await askText(`New folder in ${parent.name}`, { confirmLabel: "Create" });
    if (!name) return;
    await api.createFolder(name, parent.id);
    await this.reloadSidebar();
  }

  private async reparent(id: string, parentId: string | null): Promise<void> {
    try {
      await api.moveFolder(id, parentId);
      await this.reloadSidebar();
    } catch (err) {
      toast(String(err), "error");
    }
  }

  private async deleteFolder(folder: Folder): Promise<void> {
    const ok = await confirmAction(
      `Delete ${folder.name}?`,
      "Subfolders go too. Notes inside are moved to the trash, not deleted.",
      { confirmLabel: "Delete folder", danger: true },
    );
    if (!ok) return;
    await api.deleteFolder(folder.id);
    if (this.scope.kind === "folder") this.scope = { kind: "all" };
    await this.reloadSidebar();
    await this.reloadList();
  }
}
