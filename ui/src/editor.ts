import { Editor } from "@tiptap/core";
import StarterKit from "@tiptap/starter-kit";
import Highlight from "@tiptap/extension-highlight";
import Image from "@tiptap/extension-image";
import Placeholder from "@tiptap/extension-placeholder";
import TaskItem from "@tiptap/extension-task-item";
import TaskList from "@tiptap/extension-task-list";
import TextAlign from "@tiptap/extension-text-align";
import { convertFileSrc } from "@tauri-apps/api/core";

import { api } from "./api";
import { AudioNote, recordButton } from "./audio";
import { button, el, icon } from "./dom";
import {
  DEFAULT_INK, INK_COLORS, INK_SIZES, InkLayer, MARKER_COLORS, type ToolState,
} from "./ink";
import { PdfBackground } from "./pdf";

const HIGHLIGHTS = ["#fef08a", "#bbf7d0", "#bfdbfe", "#fbcfe8", "#fed7aa"];

export interface NoteEditor {
  editor: Editor;
  toolbar: HTMLElement;
  ink: InkLayer;
  /** Loads a different note's handwriting and leaves pen mode. */
  setInk(strokes: unknown): void;
  /** Shows an imported document behind the page, or clears it. */
  setBackground(background: unknown): Promise<void>;
  destroy(): void;
}

/** Pen settings persist across notes, the way a real pen tray does. */
const tools: ToolState = {
  tool: "pen",
  color: DEFAULT_INK,
  markerColor: MARKER_COLORS[0]!,
  size: 4,
};

/** Turns a picked or pasted file into an attachment and returns its webview URL. */
async function storeImage(noteId: string | null, file: File): Promise<string> {
  const bytes = new Uint8Array(await file.arrayBuffer());
  const stored = await api.putAttachment(noteId, file.name || "image.png", file.type || null, bytes);
  return convertFileSrc(stored.path);
}

export function createEditor(
  mount: HTMLElement,
  opts: {
    onChange: (doc: unknown) => void;
    onInkChange: () => void;
    noteId: () => string | null;
  },
): NoteEditor {
  // The page holds the text and the handwriting layer stacked on top of each
  // other, so ink can be drawn anywhere across the note rather than inside a
  // box carved out of the text flow.
  const page = el("div", { class: "page" });
  mount.appendChild(page);

  const background = new PdfBackground(() => syncPage());
  page.appendChild(background.el);

  const editor = new Editor({
    element: page,
    extensions: [
      StarterKit.configure({
        heading: { levels: [1, 2, 3] },
        link: { openOnClick: false, autolink: true, HTMLAttributes: { rel: "noopener noreferrer" } },
        codeBlock: { HTMLAttributes: { spellcheck: "false" } },
      }),
      Highlight.configure({ multicolor: true }),
      TaskList,
      TaskItem.configure({ nested: true }),
      TextAlign.configure({ types: ["heading", "paragraph"] }),
      Image.configure({ inline: false, allowBase64: false }),
      AudioNote,
      Placeholder.configure({ placeholder: "Start writing, or pick up the pen to draw…" }),
    ],
    editorProps: {
      attributes: { class: "prose", spellcheck: "true" },
      handlePaste: (_view, event) => {
        const files = Array.from(event.clipboardData?.files ?? []);
        const images = files.filter((f) => f.type.startsWith("image/"));
        if (images.length === 0) return false;
        event.preventDefault();
        void (async () => {
          for (const file of images) {
            const src = await storeImage(opts.noteId(), file);
            editor.chain().focus().setImage({ src, alt: file.name }).run();
          }
        })();
        return true;
      },
      handleDrop: (_view, event) => {
        const files = Array.from((event as DragEvent).dataTransfer?.files ?? []);
        const images = files.filter((f) => f.type.startsWith("image/"));
        if (images.length === 0) return false;
        event.preventDefault();
        void (async () => {
          for (const file of images) {
            const src = await storeImage(opts.noteId(), file);
            editor.chain().focus().setImage({ src, alt: file.name }).run();
          }
        })();
        return true;
      },
    },
    onUpdate: ({ editor: e }) => opts.onChange(e.getJSON()),
  });

  const ink = new InkLayer(page, tools, opts.onInkChange);
  page.appendChild(ink.el);

  /**
   * Keeps the page tall enough for everything on it: the text, an imported
   * document behind it, and any ink drawn below both.
   */
  const syncPage = () => {
    const prose = page.querySelector<HTMLElement>(".ProseMirror");
    const needed = Math.max(
      mount.clientHeight,
      (prose?.scrollHeight ?? 0) + 40,
      background.el.offsetHeight,
      ink.inkDepth() + 160,
    );
    const next = `${Math.round(needed)}px`;
    if (page.style.minHeight !== next) page.style.minHeight = next;
    background.rescale();
    ink.resize();
  };

  // Only the pane is watched. Observing the page as well meant reacting to
  // the height this very function sets, which the browser reports as a
  // "ResizeObserver loop" and which nothing needs.
  const observer = new ResizeObserver(() => syncPage());
  observer.observe(mount);

  const { toolbar, tray, setPenMode } = buildToolbar(editor, ink, opts.noteId, syncPage);
  const refresh = () => refreshToolbar(editor, toolbar);
  editor.on("transaction", refresh);
  editor.on("selectionUpdate", refresh);
  editor.on("update", syncPage);
  refresh();
  syncPage();

  return {
    editor,
    toolbar: el("div", { class: "toolbars" }, toolbar, tray),
    ink,
    setInk(strokes: unknown) {
      setPenMode(false);
      ink.setStrokes(strokes);
      syncPage();
    },
    async setBackground(descriptor: unknown) {
      const bg = descriptor as { kind?: string; attachmentId?: string } | null;
      if (!bg || bg.kind !== "pdf" || !bg.attachmentId) {
        background.destroy();
        page.classList.remove("has-background");
        syncPage();
        return;
      }
      page.classList.add("has-background");
      const path = await api.attachmentPath(bg.attachmentId);
      await background.load(convertFileSrc(path));
    },
    destroy() {
      background.destroy();
      observer.disconnect();
      editor.off("transaction", refresh);
      editor.off("selectionUpdate", refresh);
      editor.off("update", syncPage);
      editor.destroy();
    },
  };
}

interface ToolSpec {
  name: string;
  label: string;
  icon: string;
  run: (e: Editor) => void;
  active?: (e: Editor) => boolean;
  enabled?: (e: Editor) => boolean;
}

const TOOLS: (ToolSpec | "sep")[] = [
  {
    name: "undo", label: "Undo", icon: "undo",
    run: (e) => e.chain().focus().undo().run(),
    enabled: (e) => e.can().undo(),
  },
  {
    name: "redo", label: "Redo", icon: "redo",
    run: (e) => e.chain().focus().redo().run(),
    enabled: (e) => e.can().redo(),
  },
  "sep",
  {
    name: "h1", label: "Title", icon: "note",
    run: (e) => e.chain().focus().toggleHeading({ level: 1 }).run(),
    active: (e) => e.isActive("heading", { level: 1 }),
  },
  {
    name: "h2", label: "Heading", icon: "note",
    run: (e) => e.chain().focus().toggleHeading({ level: 2 }).run(),
    active: (e) => e.isActive("heading", { level: 2 }),
  },
  "sep",
  {
    name: "bold", label: "Bold", icon: "bold",
    run: (e) => e.chain().focus().toggleBold().run(),
    active: (e) => e.isActive("bold"),
  },
  {
    name: "italic", label: "Italic", icon: "italic",
    run: (e) => e.chain().focus().toggleItalic().run(),
    active: (e) => e.isActive("italic"),
  },
  {
    name: "underline", label: "Underline", icon: "underline",
    run: (e) => e.chain().focus().toggleUnderline().run(),
    active: (e) => e.isActive("underline"),
  },
  {
    name: "strike", label: "Strikethrough", icon: "strike",
    run: (e) => e.chain().focus().toggleStrike().run(),
    active: (e) => e.isActive("strike"),
  },
  "sep",
  {
    name: "checklist", label: "Checklist", icon: "checklist",
    run: (e) => e.chain().focus().toggleTaskList().run(),
    active: (e) => e.isActive("taskList"),
  },
  {
    name: "bullets", label: "Bulleted list", icon: "listUl",
    run: (e) => e.chain().focus().toggleBulletList().run(),
    active: (e) => e.isActive("bulletList"),
  },
  {
    name: "numbers", label: "Numbered list", icon: "listOl",
    run: (e) => e.chain().focus().toggleOrderedList().run(),
    active: (e) => e.isActive("orderedList"),
  },
  "sep",
  {
    name: "quote", label: "Quote", icon: "quote",
    run: (e) => e.chain().focus().toggleBlockquote().run(),
    active: (e) => e.isActive("blockquote"),
  },
  {
    name: "code", label: "Code block", icon: "code",
    run: (e) => e.chain().focus().toggleCodeBlock().run(),
    active: (e) => e.isActive("codeBlock"),
  },
  {
    name: "rule", label: "Divider", icon: "rule",
    run: (e) => e.chain().focus().setHorizontalRule().run(),
  },
];

interface Toolbars {
  toolbar: HTMLElement;
  tray: HTMLElement;
  setPenMode: (on: boolean) => void;
}

function buildToolbar(
  editor: Editor,
  ink: InkLayer,
  noteId: () => string | null,
  syncPage: () => void,
): Toolbars {
  const bar = el("div", { class: "toolbar", attrs: { role: "toolbar", "aria-label": "Formatting" } });

  for (const spec of TOOLS) {
    if (spec === "sep") {
      bar.appendChild(el("span", { class: "toolbar-sep" }));
      continue;
    }
    const b = button({
      label: spec.label,
      icon: spec.icon,
      class: "tool",
      showLabel: false,
      onClick: () => spec.run(editor),
    });
    b.dataset.tool = spec.name;
    if (spec.name === "h1") b.replaceChildren(el("span", { class: "tool-text", text: "H1" }));
    if (spec.name === "h2") b.replaceChildren(el("span", { class: "tool-text", text: "H2" }));
    // Keep the caret where it is when a tool is pressed.
    b.addEventListener("mousedown", (e) => e.preventDefault());
    bar.appendChild(b);
  }

  bar.appendChild(el("span", { class: "toolbar-sep" }));
  bar.appendChild(highlightMenu(editor));

  const imageBtn = button({
    label: "Insert image",
    icon: "image",
    class: "tool",
    showLabel: false,
    onClick: () => pickImage(editor, noteId),
  });
  imageBtn.addEventListener("mousedown", (e) => e.preventDefault());
  bar.appendChild(imageBtn);

  bar.appendChild(recordButton(editor, noteId));

  const penBtn = button({
    label: "Draw on the page",
    icon: "pen",
    class: "tool",
    showLabel: false,
    onClick: () => setPenMode(!ink.isEnabled()),
  });
  penBtn.dataset.tool = "pen";
  penBtn.addEventListener("mousedown", (e) => e.preventDefault());
  bar.appendChild(penBtn);

  bar.appendChild(el("span", { class: "toolbar-sep" }));
  for (const [name, label, iconName] of [
    ["left", "Align left", "alignLeft"],
    ["center", "Align centre", "alignCenter"],
    ["right", "Align right", "alignRight"],
  ] as const) {
    const b = button({
      label,
      icon: iconName,
      class: "tool",
      showLabel: false,
      onClick: () => editor.chain().focus().setTextAlign(name).run(),
    });
    b.dataset.align = name;
    b.addEventListener("mousedown", (e) => e.preventDefault());
    bar.appendChild(b);
  }

  const tray = buildPenTray(ink, syncPage, () => setPenMode(false));

  const setPenMode = (on: boolean) => {
    ink.setEnabled(on);
    penBtn.classList.toggle("active", on);
    tray.hidden = !on;
    editor.view.dom.classList.toggle("pen-mode", on);
    if (!on) editor.commands.focus();
  };
  setPenMode(false);

  return { toolbar: bar, tray, setPenMode };
}

/**
 * The pen tray: which pen, what colour, how thick. It only appears while pen
 * mode is on, so the writing toolbar is not permanently crowded by tools that
 * are only meaningful with a pen in hand.
 */
function buildPenTray(ink: InkLayer, syncPage: () => void, done: () => void): HTMLElement {
  const tray = el("div", { class: "pen-tray", attrs: { role: "toolbar", "aria-label": "Pen" } });
  const swatches: HTMLElement[] = [];
  const sizeButtons: HTMLElement[] = [];
  const toolButtons: Partial<Record<ToolState["tool"], HTMLElement>> = {};

  const refresh = () => {
    for (const [name, b] of Object.entries(toolButtons)) {
      b?.classList.toggle("active", tools.tool === name);
    }
    const active = tools.tool === "marker" ? tools.markerColor : tools.color;
    const palette = tools.tool === "marker" ? MARKER_COLORS : INK_COLORS;
    for (const s of swatches) {
      const color = s.dataset.color!;
      s.hidden = !palette.includes(color);
      s.classList.toggle("active", color === active);
    }
    for (const b of sizeButtons) {
      b.classList.toggle("active", Number(b.dataset.size) === tools.size);
    }
  };

  const press = (label: string, cls: string, onClick: () => void) => {
    const b = el("button", {
      class: cls,
      title: label,
      attrs: { type: "button", "aria-label": label },
      on: { click: onClick },
    });
    b.addEventListener("mousedown", (e) => e.preventDefault());
    return b;
  };

  const group = el("div", { class: "pen-group" });
  for (const tool of ["pen", "marker", "eraser"] as const) {
    const b = press(
      { pen: "Pen", marker: "Marker", eraser: "Eraser" }[tool],
      "pen-tool",
      () => {
        tools.tool = tool;
        refresh();
      },
    );
    b.appendChild(icon(tool === "eraser" ? "eraser" : tool === "marker" ? "highlight" : "pen", 17));
    toolButtons[tool] = b;
    group.appendChild(b);
  }

  const colors = el("div", { class: "pen-colors" });
  for (const color of [...INK_COLORS, ...MARKER_COLORS]) {
    const b = press(color, "pen-swatch", () => {
      if (tools.tool === "eraser") tools.tool = "pen";
      if (tools.tool === "marker") tools.markerColor = color;
      else tools.color = color;
      refresh();
    });
    b.style.background = color === DEFAULT_INK ? "var(--ink-default)" : color;
    b.dataset.color = color;
    swatches.push(b);
    colors.appendChild(b);
  }

  const sizes = el("div", { class: "pen-sizes" });
  for (const size of INK_SIZES) {
    const b = press(`Thickness ${size}`, "pen-size", () => {
      tools.size = size;
      refresh();
    });
    b.appendChild(el("i", { style: { width: `${3 + size}px`, height: `${3 + size}px` } }));
    b.dataset.size = String(size);
    sizeButtons.push(b);
    sizes.appendChild(b);
  }

  const undo = press("Undo stroke", "pen-tool", () => {
    ink.undo();
    syncPage();
  });
  undo.appendChild(icon("undo", 17));

  const clear = press("Erase all handwriting", "pen-tool danger", () => {
    ink.clear();
    syncPage();
  });
  clear.appendChild(icon("trash", 17));

  const finish = press("Done drawing", "pen-done", done);
  finish.appendChild(el("span", { text: "Done" }));

  tray.append(group, colors, sizes, undo, clear, finish);
  refresh();
  return tray;
}

function highlightMenu(editor: Editor): HTMLElement {
  const wrap = el("div", { class: "tool-menu" });
  const trigger = button({
    label: "Highlight",
    icon: "highlight",
    class: "tool",
    showLabel: false,
    onClick: () => wrap.classList.toggle("open"),
  });
  trigger.dataset.tool = "highlight";
  trigger.addEventListener("mousedown", (e) => e.preventDefault());

  const menu = el("div", { class: "tool-popover" });
  for (const color of HIGHLIGHTS) {
    const swatch = el("button", {
      class: "hl-swatch",
      title: color,
      attrs: { type: "button", "aria-label": `Highlight ${color}` },
      style: { background: color },
      on: {
        click: () => {
          editor.chain().focus().setHighlight({ color }).run();
          wrap.classList.remove("open");
        },
      },
    });
    swatch.addEventListener("mousedown", (e) => e.preventDefault());
    menu.appendChild(swatch);
  }
  const none = el("button", {
    class: "hl-swatch hl-none",
    title: "Remove highlight",
    attrs: { type: "button" },
    on: {
      click: () => {
        editor.chain().focus().unsetHighlight().run();
        wrap.classList.remove("open");
      },
    },
  });
  none.appendChild(icon("close", 14));
  menu.appendChild(none);

  document.addEventListener("click", (ev) => {
    if (!wrap.contains(ev.target as Node)) wrap.classList.remove("open");
  });

  wrap.append(trigger, menu);
  return wrap;
}

function pickImage(editor: Editor, noteId: () => string | null): void {
  const input = el("input", { attrs: { type: "file", accept: "image/*", multiple: true } });
  input.addEventListener("change", () => {
    void (async () => {
      for (const file of Array.from(input.files ?? [])) {
        const src = await storeImage(noteId(), file);
        editor.chain().focus().setImage({ src, alt: file.name }).run();
      }
    })();
  });
  input.click();
}

function refreshToolbar(editor: Editor, bar: HTMLElement): void {
  for (const spec of TOOLS) {
    if (spec === "sep") continue;
    const b = bar.querySelector<HTMLElement>(`[data-tool="${spec.name}"]`);
    if (!b) continue;
    b.classList.toggle("active", spec.active?.(editor) ?? false);
    if (spec.enabled) (b as HTMLButtonElement).disabled = !spec.enabled(editor);
  }
  const hl = bar.querySelector<HTMLElement>('[data-tool="highlight"]');
  hl?.classList.toggle("active", editor.isActive("highlight"));
  for (const align of ["left", "center", "right"]) {
    bar
      .querySelector<HTMLElement>(`[data-align="${align}"]`)
      ?.classList.toggle("active", editor.isActive({ textAlign: align }));
  }
}
