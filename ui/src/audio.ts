import { Node as TiptapNode, mergeAttributes } from "@tiptap/core";
import type { Editor } from "@tiptap/core";
import { convertFileSrc } from "@tauri-apps/api/core";

import { api } from "./api";
import { button, el, icon } from "./dom";
import { toast } from "./modal";

/**
 * A voice recording made inside a note.
 *
 * The clip is an ordinary attachment — content-addressed like a pasted image —
 * and the document only carries a reference to it. `attachmentId` is the
 * durable half of that reference: `src` is a webview URL built from a path on
 * the machine that recorded it, so it goes stale if the library moves, and the
 * node view re-resolves from the id when it does.
 */
export interface AudioAttrs {
  src: string;
  name: string;
  attachmentId: string | null;
  duration: number | null;
}

declare module "@tiptap/core" {
  interface Commands<ReturnType> {
    audioNote: {
      /** Inserts a recording at the caret. */
      setAudio: (attrs: AudioAttrs) => ReturnType;
    };
  }
}

/** Minutes and seconds, the way a player labels a clip. */
export function clock(seconds: number): string {
  const total = Math.max(0, Math.round(seconds));
  return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, "0")}`;
}

// ------------------------------------------------------------- the document

export const AudioNote = TiptapNode.create({
  name: "audio",
  group: "block",
  atom: true,
  draggable: true,
  selectable: true,

  addAttributes() {
    return {
      src: { default: "" },
      name: { default: "" },
      attachmentId: {
        default: null,
        parseHTML: (e) => e.getAttribute("data-attachment-id"),
        renderHTML: (a) =>
          a.attachmentId ? { "data-attachment-id": String(a.attachmentId) } : {},
      },
      duration: {
        default: null,
        parseHTML: (e) => Number(e.getAttribute("data-duration")) || null,
        renderHTML: (a) => (a.duration ? { "data-duration": String(a.duration) } : {}),
      },
    };
  },

  parseHTML() {
    return [{ tag: "audio[src]" }];
  },

  renderHTML({ HTMLAttributes }) {
    // Plain markup, so a note copied out of the app pastes as something a
    // browser can still play.
    return ["audio", mergeAttributes(HTMLAttributes, { controls: "controls", preload: "metadata" })];
  },

  addCommands() {
    return {
      setAudio:
        (attrs: AudioAttrs) =>
        ({ commands }) =>
          commands.insertContent({ type: this.name, attrs }),
    };
  },

  addNodeView() {
    return ({ node }) => {
      const attrs = node.attrs as AudioAttrs;
      const label = attrs.name || "Voice recording";
      const caption = el("span", {
        class: "audio-name",
        text: attrs.duration ? `${label} · ${clock(attrs.duration)}` : label,
      });
      const player = el("audio", {
        class: "audio-player",
        attrs: { controls: "controls", preload: "metadata" },
      });

      let objectUrl: string | null = null;
      void (async () => {
        // The attachment id is asked first, and the stored `src` is only the
        // fallback: the URL was built from a path on whichever machine made
        // the recording, so it is the half of the reference that goes stale.
        let url = attrs.src;
        if (attrs.attachmentId) {
          try {
            url = convertFileSrc(await api.attachmentPath(attrs.attachmentId));
          } catch {
            caption.textContent = `${label} — missing from the library`;
            return;
          }
        }
        try {
          // Chromium reads media in bounded byte ranges, and Tauri's asset
          // protocol on Android answers those wrongly — a range from the
          // middle of a file comes back a byte long or not at all, which
          // stalls the player with no error to show for it. Fetching the clip
          // whole and playing it from a blob asks for no ranges at all, and a
          // voice note is small enough to hold in memory.
          const response = await fetch(url);
          if (!response.ok) throw new Error(String(response.status));
          objectUrl = URL.createObjectURL(await response.blob());
          player.src = objectUrl;
        } catch {
          // Whatever went wrong reading it that way, the player can still be
          // pointed at the file itself.
          player.src = url;
        }
      })();

      const dom = el("div", { class: "audio-note", attrs: { "data-drag-handle": "" } }, player, caption);

      return {
        dom,
        // The player's own controls are the point of the node; ProseMirror
        // must not swallow the clicks and drags that work them.
        stopEvent: (event) => player.contains(event.target as Node),
        ignoreMutation: () => true,
        destroy: () => {
          if (objectUrl) URL.revokeObjectURL(objectUrl);
        },
      };
    };
  },
});

// ------------------------------------------------------------- the recorder

/**
 * Containers worth asking for, best first: Opus in WebM where Chromium is the
 * webview, and MPEG-4/AAC where WebKit is. An empty string means the webview
 * will not say what it supports, so it gets to pick.
 */
const CONTAINERS = [
  "audio/webm;codecs=opus",
  "audio/webm",
  "audio/ogg;codecs=opus",
  "audio/mp4",
  "audio/aac",
  "audio/mpeg",
];

function preferredContainer(): string | null {
  if (typeof MediaRecorder === "undefined") return null;
  if (typeof MediaRecorder.isTypeSupported !== "function") return "";
  return CONTAINERS.find((type) => MediaRecorder.isTypeSupported(type)) ?? null;
}

export type RecorderSupport = { ok: true } | { ok: false; why: string };

/**
 * Whether this webview can record at all, asked before a button is drawn
 * rather than discovered when one is pressed.
 *
 * Not every webview Snot ships in has a recorder: WebKitGTK, which is what
 * Linux desktop runs, ships `MediaRecorder` only partially and can refuse the
 * microphone outright. A missing feature is a disabled button that explains
 * itself, not an exception out of a click handler.
 */
export function recorderSupport(): RecorderSupport {
  if (typeof MediaRecorder === "undefined") {
    return { ok: false, why: "This platform's webview has no audio recorder, so Snot cannot record here." };
  }
  if (!navigator.mediaDevices?.getUserMedia) {
    return { ok: false, why: "This platform's webview will not open a microphone, so Snot cannot record here." };
  }
  if (preferredContainer() === null) {
    return { ok: false, why: "This platform's webview cannot encode audio in any format Snot can store." };
  }
  return { ok: true };
}

export interface Recording {
  blob: Blob;
  /** Measured while recording: a WebM stream from `MediaRecorder` usually
   *  carries no duration of its own. */
  seconds: number;
}

/** Holds the microphone for as long as a recording is running, and no longer. */
export class VoiceRecorder {
  private recorder: MediaRecorder | null = null;
  private stream: MediaStream | null = null;
  private chunks: Blob[] = [];
  private startedAt = 0;
  private ticker: ReturnType<typeof setInterval> | null = null;

  get active(): boolean {
    return this.recorder !== null;
  }

  /** Opens the microphone and starts recording; `onTick` gets elapsed seconds. */
  async start(onTick: (seconds: number) => void): Promise<void> {
    if (this.recorder) return;
    const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
    const container = preferredContainer();
    const recorder = new MediaRecorder(stream, container ? { mimeType: container } : undefined);
    this.stream = stream;
    this.recorder = recorder;
    this.chunks = [];
    recorder.addEventListener("dataavailable", (ev) => {
      if (ev.data.size > 0) this.chunks.push(ev.data);
    });
    recorder.start();
    this.startedAt = Date.now();
    onTick(0);
    this.ticker = setInterval(() => onTick(this.elapsed()), 500);
  }

  /** Stops, and hands back everything that was captured. */
  async stop(): Promise<Recording | null> {
    const recorder = this.recorder;
    if (!recorder) return null;
    const seconds = this.elapsed();
    const blob = await new Promise<Blob>((resolve) => {
      recorder.addEventListener(
        "stop",
        () => resolve(new Blob(this.chunks, { type: this.chunks[0]?.type || recorder.mimeType })),
        { once: true },
      );
      recorder.stop();
    });
    this.release();
    return { blob, seconds };
  }

  /** Drops the recording and the microphone with it. */
  cancel(): void {
    if (this.recorder && this.recorder.state !== "inactive") this.recorder.stop();
    this.release();
  }

  private elapsed(): number {
    return (Date.now() - this.startedAt) / 1000;
  }

  private release(): void {
    if (this.ticker !== null) clearInterval(this.ticker);
    this.ticker = null;
    this.recorder = null;
    this.chunks = [];
    for (const track of this.stream?.getTracks() ?? []) track.stop();
    this.stream = null;
  }
}

/** What to say about a microphone the browser would not hand over. */
function explain(err: unknown): string {
  const name = (err as { name?: string })?.name ?? "";
  if (name === "NotAllowedError" || name === "SecurityError") {
    return "Snot was not allowed to use the microphone. Grant it in the system settings and try again.";
  }
  if (name === "NotFoundError" || name === "OverconstrainedError") {
    return "No microphone was found on this device.";
  }
  if (name === "NotReadableError") {
    return "The microphone is busy — another app is holding it.";
  }
  return `Could not start recording: ${err}`;
}

const EXTENSIONS: [string, string][] = [
  ["webm", "webm"],
  ["ogg", "ogg"],
  ["mp4", "m4a"],
  ["aac", "aac"],
  ["mpeg", "mp3"],
  ["wav", "wav"],
];

function fileName(mime: string, at: Date): string {
  const ext = EXTENSIONS.find(([needle]) => mime.includes(needle))?.[1] ?? "webm";
  const pad = (n: number) => String(n).padStart(2, "0");
  const stamp =
    `${at.getFullYear()}-${pad(at.getMonth() + 1)}-${pad(at.getDate())}` +
    `-${pad(at.getHours())}${pad(at.getMinutes())}${pad(at.getSeconds())}`;
  return `recording-${stamp}.${ext}`;
}

/**
 * The toolbar's microphone: one button that starts a recording, shows it
 * running, and on the second press stores the clip and drops it into the note.
 */
export function recordButton(editor: Editor, noteId: () => string | null): HTMLButtonElement {
  const support = recorderSupport();
  const elapsed = el("span", { class: "rec-time", text: "0:00" });

  const b = button({
    label: "Record audio",
    icon: "mic",
    class: "tool",
    showLabel: false,
    onClick: () => void press(),
  });
  b.dataset.tool = "record";
  b.addEventListener("mousedown", (e) => e.preventDefault());

  if (!support.ok) {
    b.disabled = true;
    b.title = support.why;
    b.setAttribute("aria-label", `Record audio — ${support.why}`);
    return b;
  }

  const recorder = new VoiceRecorder();
  editor.on("destroy", () => recorder.cancel());

  const setRunning = (on: boolean) => {
    b.classList.toggle("recording", on);
    b.title = on ? "Stop recording" : "Record audio";
    b.setAttribute("aria-label", b.title);
    b.replaceChildren(icon(on ? "stop" : "mic"), elapsed);
    elapsed.hidden = !on;
  };
  setRunning(false);

  const press = async () => {
    if (recorder.active) {
      b.disabled = true;
      try {
        const clip = await recorder.stop();
        if (clip) await insert(editor, noteId(), clip);
      } catch (err) {
        toast(`Could not save that recording: ${err}`, "error");
      } finally {
        setRunning(false);
        b.disabled = false;
      }
      return;
    }
    try {
      await recorder.start((seconds) => {
        elapsed.textContent = clock(seconds);
      });
      setRunning(true);
    } catch (err) {
      recorder.cancel();
      setRunning(false);
      toast(explain(err), "error");
    }
  };

  return b;
}

/** Stores a finished clip as an attachment and puts it in the document. */
async function insert(editor: Editor, noteId: string | null, clip: Recording): Promise<void> {
  const mime = clip.blob.type || "audio/webm";
  const name = fileName(mime, new Date());
  const bytes = new Uint8Array(await clip.blob.arrayBuffer());
  const stored = await api.putAttachment(noteId, name, mime, bytes);
  editor
    .chain()
    .focus()
    .setAudio({
      src: convertFileSrc(stored.path),
      name,
      attachmentId: stored.id,
      duration: Math.round(clip.seconds * 10) / 10,
    })
    .run();
}
