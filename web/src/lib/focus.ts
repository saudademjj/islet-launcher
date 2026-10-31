/** Keyboard focus around dialogs: it stays inside while one is open and goes
    back to where it was when it closes. */

const FOCUSABLE = 'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

// The last thing focused in the app behind the dialogs. A dialog closing
// returns here; WebKit doesn't focus a clicked button, so this is mostly
// what the keyboard reached.
let lastFocus: HTMLElement | null = null;
if (typeof document !== "undefined") {
  document.addEventListener("focusin", (event) => {
    const target = event.target as HTMLElement | null;
    if (target?.closest?.(".shell")) lastFocus = target;
  });
}

/** Puts focus back behind a closing dialog, unless it already moved on:
    to what had it before, else the dialog still open underneath, else the
    app. */
export function restoreFocus(dialog: HTMLElement | null, before: Element | null) {
  const active = document.activeElement;
  const stranded = !active || active === document.body || (dialog?.contains(active) ?? false);
  if (!stranded) return;
  const usable = (el: Element | null | undefined): el is HTMLElement => el instanceof HTMLElement && el.isConnected && !dialog?.contains(el) && !el.closest("[inert]");
  const under = [...document.querySelectorAll<HTMLElement>(".modal")].filter((el) => el !== dialog).pop();
  const target = [before, under, lastFocus].find(usable);
  target?.focus({ preventScroll: true });
}

/** Tab from the last control goes to the first, and back. */
export function trapTab(event: React.KeyboardEvent, dialog: HTMLElement | null) {
  if (event.key !== "Tab" || !dialog) return;
  const items = [...dialog.querySelectorAll<HTMLElement>(FOCUSABLE)].filter((el) => el.getClientRects().length > 0);
  if (items.length === 0) return;
  const first = items[0];
  const last = items[items.length - 1];
  const active = document.activeElement;
  if (event.shiftKey && (active === first || active === dialog)) {
    event.preventDefault();
    last.focus();
  } else if (!event.shiftKey && active === last) {
    event.preventDefault();
    first.focus();
  }
}
