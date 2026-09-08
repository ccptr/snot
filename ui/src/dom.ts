type Child = Node | string | number | null | undefined | false;

interface Props {
  class?: string;
  text?: string;
  title?: string;
  attrs?: Record<string, string | number | boolean | null>;
  data?: Record<string, string>;
  style?: Partial<CSSStyleDeclaration>;
  on?: Partial<{ [K in keyof HTMLElementEventMap]: (ev: HTMLElementEventMap[K]) => void }>;
}

/** Minimal element builder — the whole UI is imperative, so this is the view layer. */
export function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  props: Props = {},
  ...children: Child[]
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (props.class) node.className = props.class;
  if (props.text !== undefined) node.textContent = props.text;
  if (props.title) node.title = props.title;
  for (const [k, v] of Object.entries(props.attrs ?? {})) {
    if (v === null || v === false) node.removeAttribute(k);
    else node.setAttribute(k, String(v));
  }
  for (const [k, v] of Object.entries(props.data ?? {})) node.dataset[k] = v;
  Object.assign(node.style, props.style ?? {});
  for (const [k, v] of Object.entries(props.on ?? {})) {
    node.addEventListener(k, v as EventListener);
  }
  append(node, children);
  return node;
}

export function append(parent: Node, children: Child[]): void {
  for (const child of children) {
    if (child === null || child === undefined || child === false) continue;
    parent.appendChild(typeof child === "object" ? child : document.createTextNode(String(child)));
  }
}

export function clear(node: Node): void {
  while (node.firstChild) node.removeChild(node.firstChild);
}

const NS = "http://www.w3.org/2000/svg";

/**
 * Icons are 24×24 stroke paths on a shared grid, so every glyph in the app
 * has the same weight regardless of which one it is.
 */
const PATHS: Record<string, string> = {
  plus: "M12 5v14M5 12h14",
  search: "M11 19a8 8 0 1 0 0-16 8 8 0 0 0 0 16zM21 21l-4.3-4.3",
  note: "M6 3h8l4 4v14H6zM14 3v4h4",
  folder: "M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z",
  star: "M12 3.5l2.7 5.5 6 .9-4.35 4.2 1 6-5.35-2.8-5.35 2.8 1-6L3.3 9.9l6-.9z",
  trash: "M4 7h16M9 7V5h6v2M6 7l1 13h10l1-13M10 11v6M14 11v6",
  tag: "M3 12V5a2 2 0 0 1 2-2h7l9 9-9 9zM8 8h.01",
  pin: "M9 3h6l-1 6 4 4H6l4-4z M12 13v8",
  chevron: "M9 6l6 6-6 6",
  back: "M15 6l-6 6 6 6",
  close: "M6 6l12 12M18 6L6 18",
  check: "M5 13l4 4 10-10",
  undo: "M9 10H5V6M5.5 10a8 8 0 1 1 1.6 6",
  redo: "M15 10h4V6M18.5 10a8 8 0 1 0-1.6 6",
  bold: "M7 4h6.5a4 4 0 0 1 0 8H7zM7 12h7.5a4 4 0 0 1 0 8H7z",
  italic: "M10 4h8M6 20h8M14.5 4l-5 16",
  underline: "M7 4v7a5 5 0 0 0 10 0V4M5 20h14",
  strike: "M4 12h16M8 8a4 3 0 0 1 8-1M16 15a4 3 0 0 1-8 1",
  highlight: "M4 20h16M6 16l7-11 5 3-7 11H6z",
  listUl: "M9 6h11M9 12h11M9 18h11M4.5 6h.01M4.5 12h.01M4.5 18h.01",
  listOl: "M10 6h10M10 12h10M10 18h10M4 5h1.5v4M4 15h2v1.5H4.5V18H6",
  checklist: "M4 6.5l1.5 1.5L8 5.5M4 17.5L5.5 19 8 16.5M11 7h9M11 17h9",
  quote: "M7 7h4v5a4 4 0 0 1-4 4zM15 7h4v5a4 4 0 0 1-4 4z",
  code: "M9 8l-5 4 5 4M15 8l5 4-5 4",
  rule: "M4 12h16",
  image: "M4 5h16v14H4zM4 16l4.5-4.5 4 4L16 12l4 4M9 9.5h.01",
  mic: "M12 3a3 3 0 0 1 3 3v5a3 3 0 0 1-6 0V6a3 3 0 0 1 3-3zM5 11a7 7 0 0 0 14 0M12 18v3M8.5 21h7",
  stop: "M7.5 7.5h9v9h-9z",
  pen: "M4 20l1-4L16 5l3 3L8 19zM14.5 6.5l3 3",
  eraser: "M8 20h11M6.5 17.5l-3-3a2 2 0 0 1 0-2.8l8-8a2 2 0 0 1 2.8 0l4.2 4.2a2 2 0 0 1 0 2.8L14 17.5zM9 9l6 6",
  alignLeft: "M4 6h16M4 12h10M4 18h14",
  alignCenter: "M4 6h16M7 12h10M5 18h14",
  alignRight: "M4 6h16M10 12h10M6 18h14",
  more: "M12 6h.01M12 12h.01M12 18h.01",
  sun: "M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8zM12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4",
  moon: "M20 14.5A8.5 8.5 0 0 1 9.5 4a8.5 8.5 0 1 0 10.5 10.5z",
  sidebar: "M4 5h16v14H4zM10 5v14",
  export: "M12 15V4M8 8l4-4 4 4M5 15v3a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2v-3",
  import: "M12 4v11M8 11l4 4 4-4M5 15v3a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2v-3",
  restore: "M4 10h6V4M4.6 10a8 8 0 1 1 .4 6",
  sort: "M7 4v14M4 15l3 3 3-3M14 6h6M14 12h5M14 18h4",
};

export function icon(name: keyof typeof PATHS | string, size = 20): SVGSVGElement {
  const svg = document.createElementNS(NS, "svg");
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("width", String(size));
  svg.setAttribute("height", String(size));
  svg.setAttribute("fill", "none");
  svg.setAttribute("stroke", "currentColor");
  svg.setAttribute("stroke-width", "1.7");
  svg.setAttribute("stroke-linecap", "round");
  svg.setAttribute("stroke-linejoin", "round");
  svg.setAttribute("aria-hidden", "true");
  const path = document.createElementNS(NS, "path");
  path.setAttribute("d", PATHS[name] ?? PATHS.note!);
  svg.appendChild(path);
  return svg;
}

interface ButtonOptions {
  label: string;
  icon?: string;
  class?: string;
  showLabel?: boolean;
  onClick: (ev: MouseEvent) => void;
}

export function button(opts: ButtonOptions): HTMLButtonElement {
  const b = el("button", {
    class: opts.class ?? "btn",
    title: opts.label,
    attrs: { type: "button", "aria-label": opts.label },
    on: { click: opts.onClick },
  });
  if (opts.icon) b.appendChild(icon(opts.icon));
  if (opts.showLabel !== false || !opts.icon) {
    b.appendChild(el("span", { class: "btn-label", text: opts.label }));
  }
  return b;
}

/** Human dates the way a notes list wants them: relative today, plain after. */
export function formatDate(ms: number): string {
  const d = new Date(ms);
  const now = new Date();
  const sameDay = d.toDateString() === now.toDateString();
  if (sameDay) return d.toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" });
  const yesterday = new Date(now);
  yesterday.setDate(now.getDate() - 1);
  if (d.toDateString() === yesterday.toDateString()) return "Yesterday";
  if (d.getFullYear() === now.getFullYear())
    return d.toLocaleDateString(undefined, { month: "short", day: "numeric" });
  return d.toLocaleDateString(undefined, { year: "numeric", month: "short", day: "numeric" });
}

export function debounce<A extends unknown[]>(fn: (...args: A) => void, ms: number) {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const wrapped = (...args: A) => {
    if (timer) clearTimeout(timer);
    timer = setTimeout(() => fn(...args), ms);
  };
  wrapped.flush = (...args: A) => {
    if (timer) clearTimeout(timer);
    timer = undefined;
    fn(...args);
  };
  wrapped.cancel = () => {
    if (timer) clearTimeout(timer);
    timer = undefined;
  };
  return wrapped;
}
