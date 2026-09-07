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

// A right-click menu on a desktop app should be the app's, not the webview's.
window.addEventListener("contextmenu", (ev) => {
  if (!(ev.target as HTMLElement)?.closest("input, textarea, [contenteditable=true]")) {
    ev.preventDefault();
  }
});
