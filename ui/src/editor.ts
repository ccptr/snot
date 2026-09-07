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
import { button, el, icon } from "./dom";
import { Ink } from "./ink";

const HIGHLIGHTS = ["#fef08a", "#bbf7d0", "#bfdbfe", "#fbcfe8", "#fed7aa"];

export interface NoteEditor {
  editor: Editor;
  toolbar: HTMLElement;
  destroy(): void;
}

/** Turns a picked or pasted file into an attachment and returns its webview URL. */
async function storeImage(noteId: string | null, file: File): Promise<string> {
  const bytes = new Uint8Array(await file.arrayBuffer());
  const stored = await api.putAttachment(noteId, file.name || "image.png", file.type || null, bytes);
  return convertFileSrc(stored.path);
}

export function createEditor(
  mount: HTMLElement,
  opts: { onChange: (doc: unknown) => void; noteId: () => string | null },
): NoteEditor {
  const editor = new Editor({
    element: mount,
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
      Placeholder.configure({ placeholder: "Start writing, or tap the pen to draw…" }),
      Ink,
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

  const toolbar = buildToolbar(editor, opts.noteId);
  const refresh = () => refreshToolbar(editor, toolbar);
  editor.on("transaction", refresh);
  editor.on("selectionUpdate", refresh);
  refresh();

  return {
    editor,
    toolbar,
    destroy() {
      editor.off("transaction", refresh);
      editor.off("selectionUpdate", refresh);
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

function buildToolbar(editor: Editor, noteId: () => string | null): HTMLElement {
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

  const inkBtn = button({
    label: "Insert drawing",
    icon: "pen",
    class: "tool",
    showLabel: false,
    onClick: () => (editor.chain().focus() as any).insertInk().run(),
  });
  inkBtn.addEventListener("mousedown", (e) => e.preventDefault());
  bar.appendChild(inkBtn);

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

  return bar;
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
