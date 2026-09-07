import * as pdfjs from "pdfjs-dist";
import type { PDFDocumentLoadingTask, PDFDocumentProxy } from "pdfjs-dist";
// Vite emits the worker as a same-origin asset, which is what the app's
// content-security-policy allows; pdf.js will not start without one.
import workerUrl from "pdfjs-dist/build/pdf.worker.min.mjs?url";

pdfjs.GlobalWorkerOptions.workerSrc = workerUrl;

/** Rendered width in device pixels. Enough to stay sharp when zoomed in. */
const RENDER_WIDTH = 1700;
const GAP = 12;

/**
 * An imported document shown behind a note, so it can be written and drawn on
 * the way you would annotate a printout.
 *
 * Pages are laid out at their true aspect ratio immediately and rasterised
 * only as they come into view — a hundred-page PDF must not cost a hundred
 * canvases, and the ink layer above needs the final geometry from the start
 * or every stroke would land in the wrong place.
 */
export class PdfBackground {
  readonly el = document.createElement("div");
  private doc: PDFDocumentProxy | null = null;
  private task: PDFDocumentLoadingTask | null = null;
  private observer: IntersectionObserver | null = null;
  private rendered = new Set<number>();

  constructor(private readonly onLayout: () => void) {
    this.el.className = "page-bg";
  }

  async load(url: string): Promise<void> {
    this.destroy();
    const task = pdfjs.getDocument({ url });
    this.task = task;
    const doc = await task.promise;
    this.doc = doc;

    // One placeholder per page, sized from the page's own dimensions.
    const slots: HTMLElement[] = [];
    for (let n = 1; n <= doc.numPages; n++) {
      const page = await doc.getPage(n);
      const view = page.getViewport({ scale: 1 });
      const slot = document.createElement("div");
      slot.className = "pdf-page";
      slot.style.aspectRatio = `${view.width} / ${view.height}`;
      slot.style.marginBottom = `${GAP}px`;
      slot.dataset.page = String(n);
      slots.push(slot);
      page.cleanup();
    }
    this.el.replaceChildren(...slots);
    this.onLayout();

    this.observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          if (entry.isIntersecting) void this.renderPage(entry.target as HTMLElement);
        }
      },
      // Start a page before it is reached so scrolling rarely shows a blank.
      { root: null, rootMargin: "400px 0px" },
    );
    for (const slot of slots) this.observer.observe(slot);
  }

  private async renderPage(slot: HTMLElement): Promise<void> {
    const number = Number(slot.dataset.page);
    if (!this.doc || this.rendered.has(number)) return;
    this.rendered.add(number);

    const page = await this.doc.getPage(number);
    const base = page.getViewport({ scale: 1 });
    const viewport = page.getViewport({ scale: RENDER_WIDTH / base.width });
    const canvas = document.createElement("canvas");
    canvas.width = Math.round(viewport.width);
    canvas.height = Math.round(viewport.height);
    const context = canvas.getContext("2d");
    if (!context) return;
    await page.render({ canvas, canvasContext: context, viewport }).promise;
    page.cleanup();
    slot.replaceChildren(canvas);
    slot.classList.add("ready");
  }

  destroy(): void {
    this.observer?.disconnect();
    this.observer = null;
    this.rendered.clear();
    this.el.replaceChildren();
    void this.task?.destroy();
    this.task = null;
    this.doc = null;
  }
}
