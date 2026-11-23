// @ts-check
const SAVE_DELAY = 150;

function setup() {
  const main = /** @type {HTMLElement | null} */ (document.querySelector(".frame"));
  if (main === null) return;
  const controller = new AbortController();
  const { signal } = controller;
  document.addEventListener("astro:before-swap", () => controller.abort(), { once: true });
  const own = () => getComputedStyle(main).overflowY === "auto";
  const scroller = () => (own() ? main : document.documentElement);
  const viewTop = () => (own() ? main.getBoundingClientRect().top : 0);

  const restore = () => {
    const scroll = history.state?.scroll;
    const target = location.hash
      ? document.getElementById(decodeURIComponent(location.hash.slice(1)))
      : null;
    if (typeof scroll === "number") scroller().scrollTop = scroll;
    else if (target !== null) target.scrollIntoView({ block: "start" });
    else scroller().scrollTop = 0;
  };
  restore();
  if (own() && document.activeElement === document.body) main.focus({ preventScroll: true });

  const path = (/** @type {string} */ pathname) =>
    pathname.endsWith("/") ? pathname : `${pathname}/`;
  for (const link of /** @type {NodeListOf<HTMLAnchorElement>} */ (
    document.querySelectorAll(".sidebar .nav a")
  )) {
    if (path(link.pathname) === path(location.pathname)) link.setAttribute("aria-current", "page");
    else link.removeAttribute("aria-current");
  }

  let saving = 0;
  const save = () => history.replaceState({ ...history.state, scroll: scroller().scrollTop }, "");
  const saveSoon = () => {
    window.clearTimeout(saving);
    saving = window.setTimeout(save, SAVE_DELAY);
  };
  main.addEventListener("scroll", saveSoon, { passive: true, signal });
  window.addEventListener("scroll", saveSoon, { passive: true, signal });
  window.addEventListener("pagehide", save, { signal });
  const page = location.pathname + location.search;
  window.addEventListener(
    "popstate",
    () => {
      if (location.pathname + location.search !== page) return;
      window.clearTimeout(saving);
      restore();
    },
    { signal },
  );
  signal.addEventListener("abort", () => window.clearTimeout(saving));

  const aside = /** @type {HTMLElement | null} */ (document.querySelector(".contents"));
  const column = /** @type {HTMLElement | null} */ (
    aside?.querySelector(".contents-scroll") ?? null
  );
  const jump = /** @type {HTMLButtonElement | null} */ (
    aside?.querySelector(".contents-jump") ?? null
  );
  const jumpLabel = jump?.querySelector("[data-jump-label]") ?? null;
  const menu = /** @type {HTMLDetailsElement | null} */ (document.querySelector(".page-contents"));
  const label = menu?.querySelector("[data-current]") ?? null;
  const lists = [...document.querySelectorAll(".contents-list")].map((list) => [
    .../** @type {NodeListOf<HTMLAnchorElement>} */ (list.querySelectorAll("a[href^='#']")),
  ]);
  const links = lists[0] ?? [];
  const rows = [...(column?.querySelectorAll("li") ?? [])];
  const sections = links.map((link) =>
    document.getElementById(decodeURIComponent(link.hash.slice(1))),
  );
  const motion = window.matchMedia("(prefers-reduced-motion: reduce)");
  let current = -1;
  let frame = 0;
  const linked = () =>
    typeof history.state?.scroll === "number"
      ? -1
      : links.findIndex((link) => link.hash === location.hash);
  let chosen = linked();
  /** @type {number | null} */
  let landed = null;

  function locate() {
    const view = scroller();
    if (chosen !== -1) {
      landed ??= view.scrollTop;
      if (Math.abs(view.scrollTop - landed) <= 2) return chosen;
      chosen = -1;
    }
    const max = view.scrollHeight - view.clientHeight;
    const inset = Number.parseFloat(getComputedStyle(view).scrollPaddingTop) + 8;
    const line = viewTop() + inset;
    const marks = sections.flatMap((section, index) =>
      section === null
        ? []
        : [{ index, at: view.scrollTop + section.getBoundingClientRect().top - line }],
    );
    const last = marks.at(-1);
    if (last !== undefined && last.at > max) {
      const from = Math.max(0, max - (view.clientHeight - inset));
      const scale = (max - from) / (last.at - from);
      for (const mark of marks) if (mark.at > from) mark.at = from + (mark.at - from) * scale;
    }
    let found = -1;
    for (const mark of marks) {
      if (mark.at > view.scrollTop + 1) break;
      found = mark.index;
    }
    return found;
  }

  /** @param {HTMLElement} row */
  function centered(row) {
    if (column === null) return 0;
    const item = row.getBoundingClientRect();
    const view = column.getBoundingClientRect();
    return column.scrollTop + item.top - view.top - (view.height - item.height) / 2;
  }

  function cue() {
    if (aside === null || column === null || jump === null) return;
    aside.toggleAttribute("data-up", column.scrollTop > 0);
    aside.toggleAttribute(
      "data-down",
      column.scrollTop + column.clientHeight < column.scrollHeight - 1,
    );
    const row = rows[current];
    if (row === undefined) {
      jump.hidden = true;
      return;
    }
    const item = row.getBoundingClientRect();
    const view = column.getBoundingClientRect();
    const side = item.bottom <= view.top ? "top" : item.top >= view.bottom ? "bottom" : undefined;
    jump.hidden = side === undefined;
    if (side === undefined) return;
    jump.dataset.side = side;
    const text = row.textContent?.trim() ?? "";
    if (jumpLabel) jumpLabel.textContent = text;
    jump.setAttribute("aria-label", `Show ${text} in the contents`);
    const stacked = rows
      .filter(
        (other, index) =>
          other.classList.contains("depth-2") &&
          (side === "top" ? index <= current : index > current),
      )
      .map((other) => other.getBoundingClientRect())
      .filter((box) => box.bottom > view.top && box.top < view.bottom);
    if (side === "top") {
      jump.style.top = `${Math.max(view.top, ...stacked.map((box) => box.bottom)) - view.top + 4}px`;
      jump.style.bottom = "";
    } else {
      jump.style.bottom = `${view.bottom - Math.min(view.bottom, ...stacked.map((box) => box.top)) + 4}px`;
      jump.style.top = "";
    }
  }

  function update() {
    frame = 0;
    const found = locate();
    if (found === current) return;
    current = found;
    for (const list of lists) {
      for (const [index, link] of list.entries()) {
        if (index === current) link.setAttribute("aria-current", "location");
        else link.removeAttribute("aria-current");
      }
    }
    if (label) label.textContent = links[current]?.textContent ?? "";
    const row = rows[current];
    if (column !== null && row !== undefined && column.offsetParent !== null) {
      const item = row.getBoundingClientRect();
      const view = column.getBoundingClientRect();
      if (item.top < view.top + 48 || item.bottom > view.bottom - 48)
        column.scrollTop = centered(row);
    }
    cue();
  }

  if (links.length > 0) {
    const onScroll = () => {
      frame ||= window.requestAnimationFrame(update);
    };
    main.addEventListener("scroll", onScroll, { passive: true, signal });
    window.addEventListener("scroll", onScroll, { passive: true, signal });
    window.addEventListener(
      "popstate",
      () => {
        chosen = linked();
        landed = null;
        onScroll();
      },
      { signal },
    );
    for (const list of lists) {
      for (const [index, link] of list.entries()) {
        link.addEventListener(
          "click",
          () => {
            chosen = index;
            landed = null;
            onScroll();
          },
          { signal },
        );
      }
    }
    update();
    cue();
  }

  if (column !== null) {
    column.addEventListener("scroll", cue, { passive: true, signal });
    const resize = new ResizeObserver(cue);
    resize.observe(column);
    signal.addEventListener("abort", () => resize.disconnect());
    jump?.addEventListener("click", () => {
      const row = rows[current];
      if (row === undefined) return;
      column.scrollTo({ top: centered(row), behavior: motion.matches ? "auto" : "smooth" });
    });
  }

  if (menu !== null) {
    menu.addEventListener("click", (event) => {
      if (event.target instanceof Element && event.target.closest("a")) menu.open = false;
    });
    menu.addEventListener("keydown", (event) => {
      if (event.key !== "Escape" || !menu.open) return;
      menu.open = false;
      menu.querySelector("summary")?.focus();
    });
    document.addEventListener(
      "pointerdown",
      (event) => {
        if (menu.open && !(event.target instanceof Node && menu.contains(event.target)))
          menu.open = false;
      },
      { signal },
    );
  }
}

setup();
document.addEventListener("astro:after-swap", setup);
