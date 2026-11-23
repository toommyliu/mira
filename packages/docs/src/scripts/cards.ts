const OPEN_DELAY = 600;
const WARM = 400;
const LEAVE_GRACE = 100;
const OFFSET = 4;
const ALIGN = 4;
const MARGIN = 8;

const card = document.createElement("div");
card.className = "hover-card";
card.setAttribute("role", "group");
card.hidden = true;
document.body.append(card);

let anchor: HTMLElement | undefined;
let openTimer: number | undefined;
let leaveTimer: number | undefined;
let closedAt = Number.NEGATIVE_INFINITY;
let dismissed: HTMLElement | undefined;

function wrapping(): boolean {
  try {
    return localStorage.getItem("card-wrap") !== "off";
  } catch {
    return true;
  }
}

function applyWrap(wrap: boolean) {
  card.classList.toggle("wrap", wrap);
  card.querySelector(".card-wrap")?.setAttribute("aria-pressed", String(wrap));
}

function element(target: EventTarget | null): Element | null {
  return target instanceof Element ? target : null;
}

function trigger(target: EventTarget | null): HTMLElement | null {
  const found = element(target)?.closest<HTMLElement>("a[data-card]") ?? null;
  return found?.closest(".hover-card") ? null : found;
}

function open(link: HTMLElement, entrance: boolean) {
  const content = document.getElementById(`card-${link.dataset.card}`);
  if (!(content instanceof HTMLTemplateElement)) return;
  window.clearTimeout(openTimer);
  window.clearTimeout(leaveTimer);
  anchor = link;
  card.replaceChildren(content.content.cloneNode(true));
  card.setAttribute("aria-label", `Definition of ${link.textContent ?? "type"}`);
  applyWrap(wrapping());
  card.hidden = false;
  card.classList.remove("entering");
  const box = link.getBoundingClientRect();
  const size = card.getBoundingClientRect();
  const below = box.bottom + OFFSET + size.height <= window.innerHeight - MARGIN;
  card.dataset.side = below ? "bottom" : "top";
  card.style.top = `${below ? box.bottom + OFFSET : Math.max(MARGIN, box.top - OFFSET - size.height)}px`;
  card.style.left = `${Math.max(MARGIN, Math.min(box.left - ALIGN, window.innerWidth - MARGIN - size.width))}px`;
  if (entrance) {
    void card.offsetWidth;
    card.classList.add("entering");
  }
}

function close() {
  window.clearTimeout(openTimer);
  window.clearTimeout(leaveTimer);
  if (card.hidden) return;
  card.hidden = true;
  anchor = undefined;
  closedAt = performance.now();
}

function warm() {
  return !card.hidden || performance.now() - closedAt < WARM;
}

document.addEventListener("pointerover", (event) => {
  if (event.pointerType === "touch") return;
  if (element(event.target)?.closest(".hover-card")) {
    window.clearTimeout(leaveTimer);
    return;
  }
  const link = trigger(event.target);
  if (link === null) return;
  window.clearTimeout(leaveTimer);
  if (link === anchor) return;
  window.clearTimeout(openTimer);
  if (warm()) open(link, false);
  else openTimer = window.setTimeout(() => open(link, true), OPEN_DELAY);
});

document.addEventListener("pointerout", (event) => {
  const from = element(event.target);
  if (from === null || from.closest("a[data-card], .hover-card") === null) return;
  const to = element(event.relatedTarget);
  if (
    to?.closest(".hover-card") ||
    (anchor !== undefined && to?.closest("a[data-card]") === anchor)
  )
    return;
  window.clearTimeout(openTimer);
  window.clearTimeout(leaveTimer);
  leaveTimer = window.setTimeout(close, LEAVE_GRACE);
});

document.addEventListener("focusin", (event) => {
  const link = trigger(event.target);
  if (link === anchor && !card.hidden) return;
  if (link !== null && link === dismissed) {
    dismissed = undefined;
    return;
  }
  if (link?.matches(":focus-visible")) open(link, false);
  else if (!element(event.target)?.closest(".hover-card")) close();
});

card.addEventListener("click", (event) => {
  if (!element(event.target)?.closest(".card-wrap")) return;
  const wrap = !card.classList.contains("wrap");
  applyWrap(wrap);
  try {
    localStorage.setItem("card-wrap", wrap ? "on" : "off");
  } catch {}
});

document.addEventListener("pointerdown", (event) => {
  if (!element(event.target)?.closest(".hover-card")) close();
});

const FOCUSABLE =
  "a[href], button:not([disabled]), input, select, textarea, summary, [tabindex]:not([tabindex='-1'])";

function controls(): HTMLElement[] {
  return [...card.querySelectorAll<HTMLElement>(FOCUSABLE)];
}

function after(link: HTMLElement): HTMLElement | undefined {
  const all = [...document.querySelectorAll<HTMLElement>(FOCUSABLE)].filter(
    (candidate) => !card.contains(candidate) && candidate.getClientRects().length > 0,
  );
  return all[all.indexOf(link) + 1];
}

document.addEventListener("keydown", (event) => {
  const focused = document.activeElement;
  const inside = focused instanceof HTMLElement && card.contains(focused);
  if (event.key === "Escape") {
    const link = anchor;
    close();
    if (inside && link !== undefined) {
      dismissed = link;
      link.focus();
    }
    return;
  }
  if (event.key !== "Tab" || card.hidden || anchor === undefined) return;
  const items = controls();
  const first = items[0];
  if (focused === anchor && !event.shiftKey && first !== undefined) {
    event.preventDefault();
    first.focus();
  } else if (inside && event.shiftKey && focused === first) {
    event.preventDefault();
    anchor.focus();
  } else if (inside && !event.shiftKey && focused === items.at(-1)) {
    event.preventDefault();
    const next = after(anchor);
    close();
    next?.focus();
  }
});

document.addEventListener(
  "scroll",
  (event) => {
    if (!element(event.target)?.closest(".hover-card")) close();
  },
  { capture: true, passive: true },
);

document.addEventListener("astro:before-swap", close);
document.addEventListener("astro:after-swap", () => document.body.append(card));
