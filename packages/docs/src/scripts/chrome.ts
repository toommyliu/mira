const root = document.documentElement;
const modes = ["system", "light", "dark"] as const;
type Mode = (typeof modes)[number];
const MENU_OFFSET = 4;
const COPIED_FOR = 1500;

function isMode(value: string | undefined): value is Mode {
  return modes.some((mode) => mode === value);
}

function mode(): Mode {
  const theme = root.dataset.theme;
  return theme === "light" || theme === "dark" ? theme : "system";
}

function remember(key: string, value: string | undefined) {
  try {
    if (value === undefined) localStorage.removeItem(key);
    else localStorage.setItem(key, value);
  } catch {}
}

function sync() {
  const current = mode();
  const name = current.charAt(0).toUpperCase() + current.slice(1);
  document.querySelector("[data-theme-trigger]")?.setAttribute("aria-label", `Theme: ${name}`);
  for (const choice of document.querySelectorAll<HTMLElement>("[data-theme-choice]")) {
    choice.setAttribute("aria-checked", String(choice.dataset.themeChoice === current));
  }
  const wrap = String(root.hasAttribute("data-wrap"));
  for (const button of document.querySelectorAll("[data-code-action='wrap']")) {
    button.setAttribute("aria-pressed", wrap);
  }
}

function choices(menu: Element): HTMLElement[] {
  return [...menu.querySelectorAll<HTMLElement>("[role='menuitemradio']")];
}

document.addEventListener("click", (event) => {
  const target = event.target instanceof Element ? event.target : null;
  if (target === null) return;
  const choice = target.closest<HTMLElement>("[data-theme-choice]");
  if (choice !== null) {
    const chosen = choice.dataset.themeChoice;
    if (!isMode(chosen)) return;
    if (chosen === "system") delete root.dataset.theme;
    else root.dataset.theme = chosen;
    remember("theme", chosen === "system" ? undefined : chosen);
    sync();
    choice.closest<HTMLElement>("[popover]")?.hidePopover();
  } else if (target.closest("[data-code-action='copy']")) {
    const button = target.closest<HTMLElement>("[data-code-action='copy']");
    const code = button?.closest(".code-block")?.querySelector("pre")?.textContent;
    if (button === null || code === undefined || code === null) return;
    void navigator.clipboard.writeText(code).then(() => {
      button.toggleAttribute("data-copied", true);
      button.setAttribute("aria-label", "Copied");
      window.setTimeout(() => {
        button.removeAttribute("data-copied");
        button.setAttribute("aria-label", "Copy code");
      }, COPIED_FOR);
    });
  } else if (target.closest("[data-code-action='wrap']")) {
    const wrap = !root.hasAttribute("data-wrap");
    root.toggleAttribute("data-wrap", wrap);
    remember("code-wrap", wrap ? "on" : "off");
    sync();
  } else if (target.closest("[data-open-nav]")) {
    document.querySelector<HTMLDialogElement>("dialog.drawer")?.showModal();
  } else if (target instanceof HTMLDialogElement && target.matches("dialog.drawer")) {
    target.close();
  }
});

document.addEventListener(
  "beforetoggle",
  (event) => {
    const menu = event.target;
    if (
      !(menu instanceof HTMLElement) ||
      !menu.matches(".menu") ||
      !(event instanceof ToggleEvent)
    ) {
      return;
    }
    if (event.newState !== "open") return;
    const trigger = document.querySelector(`[popovertarget="${menu.id}"]`);
    if (trigger === null) return;
    const box = trigger.getBoundingClientRect();
    menu.style.top = `${box.bottom + MENU_OFFSET}px`;
    menu.style.right = `${document.documentElement.clientWidth - box.right}px`;
  },
  true,
);

document.addEventListener(
  "toggle",
  (event) => {
    const menu = event.target;
    if (
      !(menu instanceof HTMLElement) ||
      !menu.matches(".menu") ||
      !(event instanceof ToggleEvent)
    ) {
      return;
    }
    if (event.newState !== "open") return;
    const items = choices(menu);
    (items.find((item) => item.getAttribute("aria-checked") === "true") ?? items[0])?.focus();
  },
  true,
);

document.addEventListener("keydown", (event) => {
  const target = event.target instanceof HTMLElement ? event.target : null;
  const menu = target?.closest(".menu");
  if (target === null || menu === null || menu === undefined) return;
  const items = choices(menu);
  const index = items.indexOf(target);
  const next =
    event.key === "ArrowDown"
      ? items[(index + 1) % items.length]
      : event.key === "ArrowUp"
        ? items[(index - 1 + items.length) % items.length]
        : event.key === "Home"
          ? items[0]
          : event.key === "End"
            ? items.at(-1)
            : undefined;
  if (next === undefined) return;
  event.preventDefault();
  next.focus();
});

document.addEventListener("astro:page-load", sync);
