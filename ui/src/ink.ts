import { getStroke } from "perfect-freehand";

const NS = "http://www.w3.org/2000/svg";
/**
 * Ink lives in a coordinate space 1000 units wide, with y in the same units.
 * A page drawn on a phone therefore reopens on a desktop at the same
 * proportions, and every stroke is vectors — sharp at any zoom, never a
 * bitmap pinned to one screen density.
 */
const CANVAS_WIDTH = 1000;

export type Point = [number, number, number];

export interface Stroke {
  points: Point[];
  color: string;
  size: number;
  tool: "pen" | "marker";
}

export type Tool = "pen" | "marker" | "eraser";

/**
 * "ink" is not a colour but a token: it renders as the page's default ink,
 * dark on a light theme and light on a dark one. Named colours are stored
 * literally, since they read on either background — only the default has to
 * follow the page, or a note written at night is blank in the morning.
 */
export const DEFAULT_INK = "ink";
export const INK_COLORS = [
  DEFAULT_INK, "#dc2626", "#ea580c", "#ca8a04",
  "#16a34a", "#0891b2", "#2563eb", "#7c3aed",
];
export const MARKER_COLORS = ["#facc15", "#4ade80", "#60a5fa", "#f472b6"];
export const INK_SIZES = [2, 4, 7, 12];

export interface ToolState {
  tool: Tool;
  color: string;
  markerColor: string;
  size: number;
}

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
  if (outline.length < 4) return "";
  const avg = (a: number, b: number) => (a + b) / 2;
  let d = `M${outline[0]![0]!.toFixed(2)},${outline[0]![1]!.toFixed(2)}`;
  for (let i = 0; i < outline.length; i++) {
    const a = outline[i]!;
    const b = outline[(i + 1) % outline.length]!;
    d += ` L${avg(a[0]!, b[0]!).toFixed(2)},${avg(a[1]!, b[1]!).toFixed(2)}`;
  }
  return `${d} Z`;
}

function renderStroke(stroke: Stroke): SVGPathElement {
  const path = document.createElementNS(NS, "path");
  path.setAttribute("d", toPath(stroke));
  // Set through style, not the attribute: presentation attributes do not
  // resolve custom properties.
  path.style.fill = stroke.color === DEFAULT_INK ? "var(--ink-default)" : stroke.color;
  if (stroke.tool === "marker") {
    path.setAttribute("opacity", "0.35");
    path.setAttribute("style", "mix-blend-mode: multiply");
  }
  return path;
}

function nearStroke(stroke: Stroke, x: number, y: number, radius: number): boolean {
  for (const [px, py] of stroke.points) {
    if (Math.hypot(px - x, py - y) <= radius) return true;
  }
  return false;
}

/**
 * The handwriting layer: a transparent sheet the size of the whole note that
 * you can draw across, over and between the text — the way a pen works on
 * paper. It ignores the pointer entirely until pen mode is switched on, so
 * typing and selecting are untouched the rest of the time.
 */
export class InkLayer {
  readonly el: SVGSVGElement;
  private committed = document.createElementNS(NS, "g");
  private live = document.createElementNS(NS, "g");
  private strokes: Stroke[] = [];
  private current: Stroke | null = null;
  private livePath: SVGPathElement | null = null;
  private activePointer: number | null = null;
  /** Once a stylus has written on this note, fingers scroll instead of draw. */
  private sawPen = false;
  private enabled = false;
  private pageHeight = 1;

  constructor(
    private readonly page: HTMLElement,
    private readonly tools: ToolState,
    private readonly onChange: () => void,
  ) {
    this.el = document.createElementNS(NS, "svg");
    this.el.setAttribute("class", "ink-layer");
    this.el.setAttribute("preserveAspectRatio", "none");
    this.el.append(this.committed, this.live);

    this.el.addEventListener("pointerdown", this.onDown);
    this.el.addEventListener("pointermove", this.onMove);
    this.el.addEventListener("pointerup", this.onUp);
    this.el.addEventListener("pointercancel", this.onUp);
  }

  /** Replaces the whole page's ink, e.g. when a different note is opened. */
  setStrokes(strokes: unknown): void {
    this.strokes = Array.isArray(strokes) ? (strokes as Stroke[]) : [];
    this.sawPen = false;
    this.render();
  }

  getStrokes(): Stroke[] {
    return this.strokes;
  }

  setEnabled(on: boolean): void {
    this.enabled = on;
    this.el.classList.toggle("drawing", on);
  }

  isEnabled(): boolean {
    return this.enabled;
  }

  undo(): void {
    if (this.strokes.length === 0) return;
    this.strokes = this.strokes.slice(0, -1);
    this.render();
    this.onChange();
  }

  clear(): void {
    if (this.strokes.length === 0) return;
    this.strokes = [];
    this.render();
    this.onChange();
  }

  /**
   * Matches the layer to the page it covers. The width sets the scale, so a
   * stroke keeps its position relative to the page at any window size.
   */
  resize(): void {
    const width = this.page.clientWidth || 1;
    const height = this.page.clientHeight || 1;
    this.pageHeight = (height / width) * CANVAS_WIDTH;
    this.el.setAttribute("viewBox", `0 0 ${CANVAS_WIDTH} ${this.pageHeight.toFixed(2)}`);
  }

  /** How far down the page the lowest stroke reaches, in page pixels. */
  inkDepth(): number {
    let max = 0;
    for (const stroke of this.strokes) {
      for (const [, y] of stroke.points) if (y > max) max = y;
    }
    const width = this.page.clientWidth || 1;
    return (max / CANVAS_WIDTH) * width;
  }

  private render(): void {
    this.committed.replaceChildren(...this.strokes.map(renderStroke));
  }

  private toCanvas(ev: PointerEvent): Point {
    const rect = this.el.getBoundingClientRect();
    const x = ((ev.clientX - rect.left) / (rect.width || 1)) * CANVAS_WIDTH;
    const y = ((ev.clientY - rect.top) / (rect.height || 1)) * this.pageHeight;
    // A mouse reports pressure 0 while held down; only trust a real stylus.
    const pressure = ev.pointerType === "pen" && ev.pressure > 0 ? ev.pressure : 0.5;
    return [x, y, pressure];
  }

  /**
   * Is this sample somewhere the pointer could actually be? WebKitGTK mixes
   * pointermove events carrying a fixed nonsense position into a real stroke,
   * both directly and inside `getCoalescedEvents()`, and one of them drags a
   * line across the whole page. Anything outside the sheet is not a point on
   * it, so it is dropped rather than drawn.
   */
  private onSheet(point: Point): boolean {
    const margin = 40;
    return (
      point[0] >= -margin &&
      point[0] <= CANVAS_WIDTH + margin &&
      point[1] >= -margin &&
      point[1] <= this.pageHeight + margin
    );
  }

  /**
   * Adds a sample unless it repeats the previous one. Duplicate points give
   * perfect-freehand a zero-length segment to take a normal from, and the
   * resulting outline explodes across the page.
   */
  private pushPoint(point: Point): boolean {
    const points = this.current!.points;
    const last = points[points.length - 1];
    if (last && Math.hypot(point[0] - last[0], point[1] - last[1]) < 0.35) return false;
    points.push(point);
    return true;
  }

  private onDown = (ev: PointerEvent): void => {
    if (!this.enabled || ev.button !== 0) return;
    if (ev.pointerType === "pen") this.sawPen = true;
    if (ev.pointerType === "touch" && this.sawPen) return;
    ev.preventDefault();
    this.activePointer = ev.pointerId;
    this.el.setPointerCapture(ev.pointerId);

    if (this.tools.tool === "eraser") {
      this.eraseAt(ev);
      return;
    }
    const start = this.toCanvas(ev);
    if (!this.onSheet(start)) return;
    this.current = {
      points: [],
      color: this.tools.tool === "marker" ? this.tools.markerColor : this.tools.color,
      size: this.tools.size,
      tool: this.tools.tool,
    };
    this.pushPoint(start);
    this.livePath = renderStroke(this.current);
    this.live.appendChild(this.livePath);
  };

  private onMove = (ev: PointerEvent): void => {
    if (ev.pointerId !== this.activePointer) return;
    ev.preventDefault();
    if (this.tools.tool === "eraser") {
      this.eraseAt(ev);
      return;
    }
    if (!this.current || !this.livePath) return;

    // Coalesced events recover the full stylus sample rate, which is what
    // keeps a fast stroke smooth rather than faceted.
    const samples =
      typeof ev.getCoalescedEvents === "function"
        ? ev.getCoalescedEvents().map((e) => this.toCanvas(e))
        : [];
    samples.push(this.toCanvas(ev));

    let added = false;
    for (const point of samples) {
      if (this.onSheet(point) && this.pushPoint(point)) added = true;
    }
    if (added) this.livePath.setAttribute("d", toPath(this.current));
  };

  private onUp = (ev: PointerEvent): void => {
    if (ev.pointerId !== this.activePointer) return;
    this.activePointer = null;
    if (this.el.hasPointerCapture(ev.pointerId)) this.el.releasePointerCapture(ev.pointerId);
    const stroke = this.current;
    this.current = null;
    this.livePath?.remove();
    this.livePath = null;
    // A single tap is a dot, not a stroke; two points are the minimum that
    // perfect-freehand can give an outline.
    if (stroke && stroke.points.length >= 2) {
      this.strokes = [...this.strokes, stroke];
      this.render();
      this.onChange();
    }
  };

  /** Whole strokes go, the way a Samsung eraser does — not pixels. */
  private eraseAt(ev: PointerEvent): void {
    const [x, y] = this.toCanvas(ev);
    const radius = 6 + this.tools.size * 2;
    const kept = this.strokes.filter((s) => !nearStroke(s, x, y, radius));
    if (kept.length !== this.strokes.length) {
      this.strokes = kept;
      this.render();
      this.onChange();
    }
  }
}
