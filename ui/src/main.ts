import "./styles.css";
import { App } from "./app";
import { toast } from "./modal";

const mount = document.getElementById("app");
if (!mount) throw new Error("missing #app");

const app = new App(mount);
app.start().catch((err) => {
  console.error(err);
  toast(`Could not open your library: ${err}`, "error");
});

// Nothing should fail silently: a dropped promise or a thrown handler is a
// bug the user has just hit, and they deserve to be told rather than left
// looking at a page that quietly did nothing.
/**
 * A benign browser warning, not a fault: it means a resize handler ran twice
 * in a frame. Showing it as a failure trains people to ignore real ones.
 */
const isNoise = (message: string) => message.includes("ResizeObserver loop");

window.addEventListener("error", (ev) => {
  const message = String(ev.message ?? ev.error);
  if (isNoise(message)) return;
  console.error(ev.error ?? ev.message);
  toast(message, "error");
});
window.addEventListener("unhandledrejection", (ev) => {
  const message = String(ev.reason);
  if (isNoise(message)) return;
  console.error(ev.reason);
  toast(message, "error");
});

// A right-click menu on a desktop app should be the app's, not the webview's.
window.addEventListener("contextmenu", (ev) => {
  if (!(ev.target as HTMLElement)?.closest("input, textarea, [contenteditable=true]")) {
    ev.preventDefault();
  }
});




