import { el } from "./dom";
import { pushScreen } from "./nav";

/**
 * The app draws its own dialogs: `window.prompt`/`confirm` are unavailable or
 * inconsistent across the webviews Tauri wraps, and these can be themed.
 */
function shell(
  title: string,
  body: HTMLElement,
  actions: HTMLElement,
  dismiss: () => void,
): () => void {
  const card = el("div", { class: "modal-card", attrs: { role: "dialog", "aria-modal": "true" } },
    el("h2", { class: "modal-title", text: title }), body, actions);
  const backdrop = el("div", { class: "modal-backdrop" }, card);
  document.body.appendChild(backdrop);
  requestAnimationFrame(() => backdrop.classList.add("in"));
  // A dialog covers the app, so the back gesture and Escape answer it rather
  // than reaching past it to the note or the app itself.
  const drop = pushScreen(dismiss);
  return () => {
    backdrop.remove();
    drop();
  };
}

export function askText(
  title: string,
  opts: { initial?: string; placeholder?: string; confirmLabel?: string } = {},
): Promise<string | null> {
  return new Promise((resolve) => {
    const input = el("input", {
      class: "modal-input",
      attrs: { type: "text", value: opts.initial ?? "", placeholder: opts.placeholder ?? "" },
    });
    let close = () => {};
    const done = (value: string | null) => {
      close();
      resolve(value);
    };
    const actions = el("div", { class: "modal-actions" },
      el("button", { class: "btn ghost", text: "Cancel", attrs: { type: "button" },
        on: { click: () => done(null) } }),
      el("button", { class: "btn primary", text: opts.confirmLabel ?? "Save",
        attrs: { type: "button" },
        on: { click: () => done(input.value.trim() || null) } }),
    );
    input.addEventListener("keydown", (ev) => {
      if (ev.key === "Enter") done(input.value.trim() || null);
    });
    close = shell(title, el("div", { class: "modal-body" }, input), actions, () => done(null));
    input.focus();
    input.select();
  });
}

export function confirmAction(
  title: string,
  message: string,
  opts: { confirmLabel?: string; danger?: boolean } = {},
): Promise<boolean> {
  return new Promise((resolve) => {
    let close = () => {};
    const done = (ok: boolean) => {
      close();
      resolve(ok);
    };
    const actions = el("div", { class: "modal-actions" },
      el("button", { class: "btn ghost", text: "Cancel", attrs: { type: "button" },
        on: { click: () => done(false) } }),
      el("button", {
        class: `btn ${opts.danger ? "danger" : "primary"}`,
        text: opts.confirmLabel ?? "Confirm",
        attrs: { type: "button" },
        on: { click: () => done(true) },
      }),
    );
    close = shell(title, el("p", { class: "modal-body", text: message }), actions, () => done(false));
  });
}

/**
 * How much of the window the soft keyboard is covering, published as a CSS
 * variable.
 *
 * Android does not take the covered strip away: the layout viewport keeps its
 * full height and the keys are drawn over the bottom of it, so a toast pinned
 * to that bottom is painted underneath them and never seen. The visual
 * viewport is the part actually on screen, and `--keyboard` is the difference.
 * Where the window really is resized instead, the two agree and it stays zero.
 */
function trackKeyboard(): void {
  const view = window.visualViewport;
  if (!view) return;
  const place = () => {
    const covered = Math.max(0, window.innerHeight - view.height - view.offsetTop);
    document.documentElement.style.setProperty("--keyboard", `${Math.round(covered)}px`);
  };
  view.addEventListener("resize", place);
  view.addEventListener("scroll", place);
  place();
}
trackKeyboard();

/** A short-lived message; errors stay up longer because they need reading. */
export function toast(message: string, kind: "info" | "error" = "info"): void {
  let host = document.querySelector<HTMLElement>(".toasts");
  if (!host) {
    host = el("div", { class: "toasts" });
    document.body.appendChild(host);
  }
  const item = el("div", { class: `toast ${kind}`, text: message });
  host.appendChild(item);
  requestAnimationFrame(() => item.classList.add("in"));
  setTimeout(() => {
    item.classList.remove("in");
    setTimeout(() => item.remove(), 250);
  }, kind === "error" ? 5200 : 2400);
}

/** Closes whichever popup menu is open, if any. */
let closeMenu: () => void = () => {};

/** A small popup menu anchored under a button. */
export function menu(
  anchor: HTMLElement,
  items: ({ label: string; danger?: boolean; onSelect: () => void } | "sep")[],
): void {
  closeMenu();
  const list = el("div", { class: "popmenu", attrs: { role: "menu" } });

  const outside = (ev: MouseEvent) => {
    if (!list.contains(ev.target as Node)) closeMenu();
  };
  let drop = () => {};
  closeMenu = () => {
    closeMenu = () => {};
    list.remove();
    document.removeEventListener("mousedown", outside);
    drop();
  };
  for (const item of items) {
    if (item === "sep") {
      list.appendChild(el("div", { class: "popmenu-sep" }));
      continue;
    }
    list.appendChild(el("button", {
      class: `popmenu-item${item.danger ? " danger" : ""}`,
      text: item.label,
      attrs: { type: "button", role: "menuitem" },
      on: {
        click: () => {
          closeMenu();
          item.onSelect();
        },
      },
    }));
  }
  document.body.appendChild(list);

  const rect = anchor.getBoundingClientRect();
  const width = list.offsetWidth;
  list.style.top = `${Math.min(rect.bottom + 6, window.innerHeight - list.offsetHeight - 8)}px`;
  list.style.left = `${Math.max(8, Math.min(rect.left, window.innerWidth - width - 8))}px`;

  drop = pushScreen(() => closeMenu());
  setTimeout(() => document.addEventListener("mousedown", outside), 0);
}
