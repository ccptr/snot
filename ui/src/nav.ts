/**
 * The screens a back gesture peels off before it leaves the app.
 *
 * Android answers its back button out of the webview's history, and Snot is
 * one page. With nothing in that history the first press closes the app from
 * whatever pane, drawer or dialog happens to be covering the screen, so
 * anything that covers the screen registers here.
 *
 * What is kept is one guard entry behind the app for as long as anything is
 * registered — one entry, not one per screen. A stack of entries cannot be
 * kept in step with the app: `history.back()` lands a task later, so a screen
 * that closes and immediately opens another one (a menu item that opens a
 * dialog) pushes its entry before the entry it replaced has come off, and the
 * late arrival then closes the wrong thing. One guard is put back
 * synchronously as each screen comes off, and never gets out of order.
 */

interface Screen {
  /**
   * Takes the screen down, by the same path the app itself would: it calls
   * whatever the app gave the function `pushScreen` handed back, so a screen
   * closed by a back press and one closed by a button leave the same way.
   */
  dismiss: () => void;
}

const GUARD = { snot: "screen" };

const screens: Screen[] = [];

/** Whether the guard entry is currently in the history. */
let guarded = false;

/** Set while a guard entry nothing needs any more is queued to come off. */
let dropping = false;

/** Set while a `history.back()` we asked for ourselves is on its way. */
let spending = false;

/**
 * Registers a screen the back gesture should close before it leaves the app.
 *
 * Returns the function to call when the screen is closed some other way — a
 * Back button, a Cancel, a tap on the scrim — so the history comes back with
 * it. Calling it twice, or after the back gesture has already closed the
 * screen, does nothing.
 */
export function pushScreen(dismiss: () => void): () => void {
  const screen: Screen = { dismiss };
  screens.push(screen);
  syncGuard();
  return () => {
    const at = screens.indexOf(screen);
    if (at === -1) return;
    screens.splice(at, 1);
    syncGuard();
  };
}

/** Keeps exactly one history entry behind the app while any screen is open. */
function syncGuard(): void {
  if (screens.length > 0) {
    // A guard that was on its way out is kept instead.
    dropping = false;
    // Nothing is pushed while a back of our own is in flight: it would land
    // after the entry it replaced comes off, leaving one entry too many and a
    // press that appears to do nothing. The press itself puts the guard back.
    if (guarded || spending) return;
    try {
      history.pushState(GUARD, "");
      guarded = true;
    } catch {
      // A webview that will not take a history entry leaves the back gesture
      // where it already was — it closes the app. Nothing else changes.
    }
    return;
  }

  if (!guarded || dropping) return;
  // Giving the entry up waits a task, because a screen that closes only to
  // open another one — a menu item that opens a dialog — should hand the guard
  // on rather than spend it.
  dropping = true;
  setTimeout(() => {
    if (!dropping) return;
    dropping = false;
    guarded = false;
    spending = true;
    history.back();
  }, 0);
}

window.addEventListener("popstate", () => {
  const asked = spending;
  spending = false;
  // Whatever the press was for, the guard is behind us now.
  dropping = false;
  guarded = false;
  // A press nobody asked for is somebody going back, and the topmost screen
  // closes. The one we asked for has already done its closing.
  if (!asked) screens.pop()?.dismiss();
  syncGuard();
});

// Escape is the same gesture with a keyboard, so it takes the same path.
window.addEventListener("keydown", (ev) => {
  if (ev.key !== "Escape" || screens.length === 0) return;
  ev.preventDefault();
  if (guarded) history.back();
  else screens.pop()?.dismiss();
});
