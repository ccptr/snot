import { Node, mergeAttributes } from "@tiptap/core";
import type { NodeViewRendererProps } from "@tiptap/core";
import { getStroke } from "perfect-freehand";

const NS = "http://www.w3.org/2000/svg";
/**
 * Ink lives in its own coordinate space 1000 units wide, so a drawing made on
 * a phone reopens on a desktop at the same proportions and stays crisp at any
 * zoom — the drawing is vectors, never a bitmap.
 */
const CANVAS_WIDTH = 1000;
const DEFAULT_HEIGHT = 420;

export type Point = [number, number, number];

export interface Stroke {
  points: Point[];
  color: string;
  size: number;
  tool: "pen" | "marker";
}

export const INK_COLORS = [
  "#1f2937", "#dc2626", "#ea580c", "#ca8a04",
  "#16a34a", "#0891b2", "#2563eb", "#7c3aed",
];
const MARKER_COLORS = ["#fde047", "#86efac", "#93c5fd", "#f9a8d4"];

interface ToolState {
  tool: "pen" | "marker" | "eraser";
  color: string;
  markerColor: string;
  size: number;
}

/** Pen settings are shared by every ink block, the way a real pen tray is. */
const tools: ToolState = { tool: "pen", color: INK_COLORS[0]!, markerColor: MARKER_COLORS[0]!, size: 4 };

function strokeOptions(stroke: Stroke) {
  const marker = stroke.tool === "marker";
  return {
    size: stroke.size * (marker ? 4 : 1),
    thinning: marker ? 0 : 0.62,
    smoothing: 0.55,
    streamline: 0.42,
    simulatePressure: false,
    last: true,
  };
}

function toPath(stroke: Stroke): string {
  const outline = getStroke(stroke.points, strokeOptions(stroke));
  if (outline.length === 0) return "";
  // Round the corners of the outline polygon so strokes read as ink, not as
  // a many-sided polygon, at high zoom.
  let d = `M ${outline[0]![0]!.toFixed(2)} ${outline[0]![1]!.toFixed(2)}`;
  for (let i = 1; i < outline.length; i++) {
    const a = outline[i]!;
    const b = outline[(i + 1) % outline.length]!;
    d += ` Q ${a[0]!.toFixed(2)} ${a[1]!.toFixed(2)} ${((a[0]! + b[0]!) / 2).toFixed(2)} ${((a[1]! + b[1]!) / 2).toFixed(2)}`;
  }
  return d + " Z";
}

function renderStroke(stroke: Stroke): SVGPathElement {
  const path = document.createElementNS(NS, "path");
  path.setAttribute("d", toPath(stroke));
  path.setAttribute("fill", stroke.tool === "marker" ? stroke.color : stroke.color);
  if (stroke.tool === "marker") {
    path.setAttribute("opacity", "0.38");
    path.setAttribute("style", "mix-blend-mode: multiply");
  }
  return path;
}

function distanceToStroke(stroke: Stroke, x: number, y: number): number {
  let best = Infinity;
  for (const [px, py] of stroke.points) {
    const d = Math.hypot(px - x, py - y);
    if (d < best) best = d;
  }
  return best;
}

export const Ink = Node.create({
  name: "ink",
  group: "block",
  atom: true,
  draggable: true,
  selectable: true,

  addAttributes() {
    return {
      strokes: {
        default: [] as Stroke[],
        parseHTML: (element) => {
          try {
            return JSON.parse(element.getAttribute("data-strokes") ?? "[]");
          } catch {
            return [];
          }
        },
        renderHTML: (attrs) => ({ "data-strokes": JSON.stringify(attrs.strokes ?? []) }),
      },
      height: {
        default: DEFAULT_HEIGHT,
        parseHTML: (element) => Number(element.getAttribute("data-height")) || DEFAULT_HEIGHT,
        renderHTML: (attrs) => ({ "data-height": String(attrs.height) }),
      },
      ruled: {
        default: false,
        parseHTML: (element) => element.getAttribute("data-ruled") === "true",
        renderHTML: (attrs) => ({ "data-ruled": String(!!attrs.ruled) }),
      },
    };
  },

  parseHTML() {
    return [{ tag: "div[data-ink]" }];
  },

  renderHTML({ HTMLAttributes }) {
    return ["div", mergeAttributes(HTMLAttributes, { "data-ink": "true" })];
  },

  addCommands() {
    return {
      insertInk:
        (attrs?: { height?: number; ruled?: boolean }) =>
        ({ commands }: { commands: any }) =>
          commands.insertContent({
            type: this.name,
            attrs: { strokes: [], height: attrs?.height ?? DEFAULT_HEIGHT, ruled: !!attrs?.ruled },
          }),
    } as any;
  },

  addNodeView() {
    return (props: NodeViewRendererProps) => new InkView(props);
  },
});

class InkView {
  dom: HTMLElement;
  private svg: SVGSVGElement;
  private strokeLayer: SVGGElement;
  private liveLayer: SVGGElement;
  private toolbar: HTMLElement;
  private node: NodeViewRendererProps["node"];
  private editor: NodeViewRendererProps["editor"];
  private getPos: () => number | undefined;
  private drawing = false;
  private current: Stroke | null = null;
  private livePath: SVGPathElement | null = null;
  private activePointer: number | null = null;
  /** Once a stylus has touched this block, fingers scroll instead of drawing. */
  private sawPen = false;
  private editMode = false;

  constructor({ node, editor, getPos }: NodeViewRendererProps) {
    this.node = node;
    this.editor = editor;
    this.getPos = getPos as () => number | undefined;

    this.svg = document.createElementNS(NS, "svg");
    this.svg.setAttribute("class", "ink-surface");
    this.svg.setAttribute("preserveAspectRatio", "none");
    this.strokeLayer = document.createElementNS(NS, "g");
    this.liveLayer = document.createElementNS(NS, "g");
    this.svg.append(this.strokeLayer, this.liveLayer);

    this.toolbar = document.createElement("div");
    this.toolbar.className = "ink-toolbar";

    const grip = document.createElement("div");
    grip.className = "ink-grip";
    grip.title = "Drag to resize";
    grip.addEventListener("pointerdown", this.onGripDown);

    this.dom = document.createElement("div");
    this.dom.className = "ink-block";
    this.dom.append(this.toolbar, this.svg, grip);

    this.svg.addEventListener("pointerdown", this.onPointerDown);
    this.svg.addEventListener("pointermove", this.onPointerMove);
    this.svg.addEventListener("pointerup", this.onPointerUp);
    this.svg.addEventListener("pointercancel", this.onPointerUp);
    this.svg.addEventListener("pointerleave", this.onPointerUp);

    this.buildToolbar();
    this.sync();
  }

  private get strokes(): Stroke[] {
    return (this.node.attrs.strokes ?? []) as Stroke[];
  }

  private get height(): number {
    return (this.node.attrs.height as number) || DEFAULT_HEIGHT;
  }

  private sync(): void {
    const h = this.height;
    this.svg.setAttribute("viewBox", `0 0 ${CANVAS_WIDTH} ${h}`);
    this.svg.style.aspectRatio = `${CANVAS_WIDTH} / ${h}`;
    this.dom.classList.toggle("ruled", !!this.node.attrs.ruled);
    this.dom.classList.toggle("editing", this.editMode);
    this.dom.classList.toggle("empty", this.strokes.length === 0);
    this.svg.style.touchAction = this.editMode && this.sawPen === false ? "none" : "auto";

    this.strokeLayer.replaceChildren(...this.strokes.map(renderStroke));
    this.refreshToolbar();
  }

  private setAttrs(attrs: Record<string, unknown>): void {
    const pos = this.getPos();
    if (pos === undefined) return;
    const tr = this.editor.state.tr.setNodeMarkup(pos, undefined, {
      ...this.node.attrs,
      ...attrs,
    });
    this.editor.view.dispatch(tr);
  }

  // ------------------------------------------------------------- toolbar

  private swatches: HTMLElement[] = [];
  private toolButtons: Record<string, HTMLElement> = {};

  private buildToolbar(): void {
    const mk = (label: string, cls: string, onClick: () => void) => {
      const b = document.createElement("button");
      b.type = "button";
      b.className = cls;
      b.title = label;
      b.setAttribute("aria-label", label);
      b.addEventListener("mousedown", (e) => e.preventDefault());
      b.addEventListener("click", onClick);
      return b;
    };

    const draw = mk("Draw", "ink-tool ink-draw-toggle", () => {
      this.editMode = !this.editMode;
      this.sync();
    });
    draw.textContent = "Draw";
    this.toolButtons.draw = draw;

    const group = document.createElement("div");
    group.className = "ink-tools";

    for (const tool of ["pen", "marker", "eraser"] as const) {
      const b = mk(tool[0]!.toUpperCase() + tool.slice(1), "ink-tool", () => {
        tools.tool = tool;
        this.editMode = true;
        this.sync();
      });
      b.textContent = { pen: "Pen", marker: "Marker", eraser: "Eraser" }[tool];
      this.toolButtons[tool] = b;
      group.appendChild(b);
    }

    const colors = document.createElement("div");
    colors.className = "ink-colors";
    for (const c of INK_COLORS) {
      const b = mk(c, "ink-swatch", () => {
        if (tools.tool === "eraser") tools.tool = "pen";
        if (tools.tool === "marker") tools.markerColor = c;
        else tools.color = c;
        this.sync();
      });
      b.style.background = c;
      b.dataset.color = c;
      this.swatches.push(b);
      colors.appendChild(b);
    }

    const sizes = document.createElement("div");
    sizes.className = "ink-sizes";
    for (const s of [2, 4, 7, 12]) {
      const b = mk(`Size ${s}`, "ink-size", () => {
        tools.size = s;
        this.sync();
      });
      const dot = document.createElement("i");
      dot.style.width = dot.style.height = `${Math.min(14, 3 + s)}px`;
      b.appendChild(dot);
      b.dataset.size = String(s);
      sizes.appendChild(b);
    }

    const ruled = mk("Ruled guide", "ink-tool", () => {
      this.setAttrs({ ruled: !this.node.attrs.ruled });
    });
    ruled.textContent = "Guides";
    this.toolButtons.ruled = ruled;

    const clear = mk("Clear drawing", "ink-tool ink-danger", () => {
      if (this.strokes.length) this.setAttrs({ strokes: [] });
    });
    clear.textContent = "Clear";

    this.toolbar.append(draw, group, colors, sizes, ruled, clear);
  }

  private refreshToolbar(): void {
    this.toolButtons.draw?.classList.toggle("active", this.editMode);
    for (const tool of ["pen", "marker", "eraser"]) {
      this.toolButtons[tool]?.classList.toggle("active", this.editMode && tools.tool === tool);
    }
    this.toolButtons.ruled?.classList.toggle("active", !!this.node.attrs.ruled);
    const active = tools.tool === "marker" ? tools.markerColor : tools.color;
    for (const s of this.swatches) s.classList.toggle("active", s.dataset.color === active);
    for (const s of Array.from(this.toolbar.querySelectorAll<HTMLElement>(".ink-size"))) {
      s.classList.toggle("active", Number(s.dataset.size) === tools.size);
    }
  }

  // ------------------------------------------------------------- drawing

  private toCanvas(ev: PointerEvent): Point {
    const rect = this.svg.getBoundingClientRect();
    const x = ((ev.clientX - rect.left) / rect.width) * CANVAS_WIDTH;
    const y = ((ev.clientY - rect.top) / rect.height) * this.height;
    // A mouse reports pressure 0 while down; treat anything unhelpful as mid.
    const pressure = ev.pointerType === "pen" && ev.pressure > 0 ? ev.pressure : 0.5;
    return [x, y, pressure];
  }

  private onPointerDown = (ev: PointerEvent): void => {
    if (!this.editMode || ev.button !== 0) return;
    if (ev.pointerType === "pen") this.sawPen = true;
    // Palm rejection: after a stylus has been used here, fingers scroll.
    if (ev.pointerType === "touch" && this.sawPen) return;
    ev.preventDefault();
    this.activePointer = ev.pointerId;
    this.svg.setPointerCapture(ev.pointerId);
    this.drawing = true;

    if (tools.tool === "eraser") {
      this.erodeAt(ev);
      return;
    }
    this.current = {
      points: [this.toCanvas(ev)],
      color: tools.tool === "marker" ? tools.markerColor : tools.color,
      size: tools.size,
      tool: tools.tool,
    };
    this.livePath = renderStroke(this.current);
    this.liveLayer.appendChild(this.livePath);
  };

  private onPointerMove = (ev: PointerEvent): void => {
    if (!this.drawing || ev.pointerId !== this.activePointer) return;
    ev.preventDefault();
    if (tools.tool === "eraser") {
      this.erodeAt(ev);
      return;
    }
    if (!this.current || !this.livePath) return;
    // Coalesced events recover the full stylus sample rate, which is what
    // makes a fast stroke smooth rather than faceted.
    const events = typeof ev.getCoalescedEvents === "function" ? ev.getCoalescedEvents() : [ev];
    for (const e of events.length ? events : [ev]) this.current.points.push(this.toCanvas(e));
    this.livePath.setAttribute("d", toPath(this.current));
  };

  private onPointerUp = (ev: PointerEvent): void => {
    if (ev.pointerId !== this.activePointer) return;
    this.drawing = false;
    this.activePointer = null;
    if (this.svg.hasPointerCapture(ev.pointerId)) this.svg.releasePointerCapture(ev.pointerId);
    if (this.current && this.current.points.length > 1) {
      this.setAttrs({ strokes: [...this.strokes, this.current] });
    }
    this.current = null;
    this.livePath?.remove();
    this.livePath = null;
  };

  /** Stroke-level eraser: whole strokes go, the way a Samsung eraser does. */
  private erodeAt(ev: PointerEvent): void {
    const [x, y] = this.toCanvas(ev);
    const radius = 6 + tools.size * 2;
    const kept = this.strokes.filter((s) => distanceToStroke(s, x, y) > radius);
    if (kept.length !== this.strokes.length) this.setAttrs({ strokes: kept });
  }

  private onGripDown = (ev: PointerEvent): void => {
    ev.preventDefault();
    const startY = ev.clientY;
    const rect = this.svg.getBoundingClientRect();
    const unitsPerPx = this.height / rect.height;
    const startHeight = this.height;
    const move = (e: PointerEvent) => {
      const next = Math.round(
        Math.min(2000, Math.max(160, startHeight + (e.clientY - startY) * unitsPerPx)),
      );
      this.svg.setAttribute("viewBox", `0 0 ${CANVAS_WIDTH} ${next}`);
      this.svg.style.aspectRatio = `${CANVAS_WIDTH} / ${next}`;
      this.dom.dataset.pendingHeight = String(next);
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      const next = Number(this.dom.dataset.pendingHeight);
      if (next && next !== startHeight) this.setAttrs({ height: next });
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };

  // -------------------------------------------------------- node view API

  update(node: NodeViewRendererProps["node"]): boolean {
    if (node.type.name !== "ink") return false;
    this.node = node;
    this.sync();
    return true;
  }

  selectNode(): void {
    this.dom.classList.add("selected");
  }

  deselectNode(): void {
    this.dom.classList.remove("selected");
    this.editMode = false;
    this.sync();
  }

  /** ProseMirror must not treat drawing gestures as document edits. */
  stopEvent(event: Event): boolean {
    if (event.type.startsWith("pointer") || event.type.startsWith("mouse")) {
      return this.editMode || this.toolbar.contains(event.target as Element);
    }
    return false;
  }

  ignoreMutation(): boolean {
    return true;
  }

  destroy(): void {
    this.svg.removeEventListener("pointerdown", this.onPointerDown);
    this.svg.removeEventListener("pointermove", this.onPointerMove);
    this.svg.removeEventListener("pointerup", this.onPointerUp);
    this.svg.removeEventListener("pointercancel", this.onPointerUp);
    this.svg.removeEventListener("pointerleave", this.onPointerUp);
  }
}
