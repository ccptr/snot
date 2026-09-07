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
window.addEventListener("error", (ev) => {
  console.error(ev.error ?? ev.message);
  toast(String(ev.message ?? ev.error), "error");
});
window.addEventListener("unhandledrejection", (ev) => {
  console.error(ev.reason);
  toast(String(ev.reason), "error");
});

// A right-click menu on a desktop app should be the app's, not the webview's.
window.addEventListener("contextmenu", (ev) => {
  if (!(ev.target as HTMLElement)?.closest("input, textarea, [contenteditable=true]")) {
    ev.preventDefault();
  }
});




