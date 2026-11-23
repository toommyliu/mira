import { navigate } from "astro:transitions/client";

import type { Entry, Group } from "../pages/search.json.ts";

const LIMIT = 100;
const groups: readonly string[] = ["namespace", "function", "type", "doc"] satisfies Group[];

const dialog = document.querySelector<HTMLDialogElement>("dialog.search");
const input = dialog?.querySelector<HTMLInputElement>("input");
const list = dialog?.querySelector<HTMLUListElement>("[role='listbox']");
const empty = dialog?.querySelector<HTMLElement>(".search-empty");
let entries: Promise<Entry[]> | undefined;
type Result = { readonly entry: Entry; readonly snippet: string | undefined };
const SNIPPET_BEFORE = 40;
const SNIPPET_AFTER = 80;

let results: Result[] = [];
let active = 0;
let filter = "";
const filters = [...(dialog?.querySelectorAll<HTMLButtonElement>("[data-filter]") ?? [])];

function snippet(text: string, at: number): string {
  const start = Math.max(0, at - SNIPPET_BEFORE);
  const end = Math.min(text.length, at + SNIPPET_AFTER);
  return `${start > 0 ? "…" : ""}${text.slice(start, end).trim()}${end < text.length ? "…" : ""}`;
}

function score(entry: Entry, query: string): { rank: number; snippet: string | undefined } {
  const title = entry.title.toLowerCase();
  const last = title.slice(title.lastIndexOf(".") + 1);
  const words = query.split(/\s+/).filter((word) => word !== "");
  const found = (rank: number, at?: number) => ({
    rank,
    snippet: at === undefined ? undefined : snippet(entry.text, at),
  });
  if (title === query || last === query) return found(0);
  if (last.startsWith(query)) return found(1);
  if (title.startsWith(query)) return found(2);
  if (title.includes(query)) return found(3);
  const meta = `${title} ${entry.context} ${entry.kind}`.toLowerCase();
  if (words.every((word) => meta.includes(word))) return found(4);
  const text = entry.text.toLowerCase();
  if (text !== "" && words.every((word) => text.includes(word))) {
    return found(5, text.indexOf(words[0] ?? query));
  }
  return found(Number.POSITIVE_INFINITY);
}

function render() {
  if (!list || !input || !empty) return;
  list.replaceChildren(
    ...results.map(({ entry, snippet }, index) => {
      const option = document.createElement("li");
      option.id = `search-${index}`;
      option.role = "option";
      option.ariaSelected = String(index === active);
      const link = document.createElement("a");
      link.href = entry.href;
      link.tabIndex = -1;
      const title = document.createElement("span");
      title.className = entry.code ? "search-title code" : "search-title";
      title.textContent = entry.title;
      const meta = document.createElement("span");
      meta.className = "search-meta";
      meta.textContent = entry.context === "" ? entry.kind : `${entry.kind} · ${entry.context}`;
      link.append(title, meta);
      if (snippet !== undefined) {
        const excerpt = document.createElement("span");
        excerpt.className = "search-snippet";
        excerpt.textContent = snippet;
        link.append(excerpt);
      }
      option.append(link);
      option.addEventListener("pointermove", () => select(index));
      return option;
    }),
  );
  const query = input.value.trim();
  empty.hidden = (query === "" && filter === "") || results.length > 0;
  input.setAttribute("aria-expanded", String(results.length > 0));
  if (results.length > 0) input.setAttribute("aria-activedescendant", `search-${active}`);
  else input.removeAttribute("aria-activedescendant");
}

function select(index: number) {
  if (index === active || !list) return;
  list.children[active]?.setAttribute("aria-selected", "false");
  active = index;
  const option = list.children[active];
  option?.setAttribute("aria-selected", "true");
  option?.scrollIntoView({ block: "nearest" });
  input?.setAttribute("aria-activedescendant", `search-${active}`);
}

async function search() {
  if (!input) return;
  const query = input.value.trim().toLowerCase();
  entries ??= fetch("/search.json").then((response): Promise<Entry[]> => response.json());
  const all = await entries;
  if (input.value.trim().toLowerCase() !== query) return;
  const pool = filter === "" ? all : all.filter((entry) => entry.group === filter);
  results =
    query === ""
      ? filter === ""
        ? []
        : pool
            .toSorted((a, b) => a.title.localeCompare(b.title))
            .slice(0, LIMIT)
            .map((entry) => ({ entry, snippet: undefined }))
      : pool
          .map((entry) => ({ entry, ...score(entry, query) }))
          .filter((match) => Number.isFinite(match.rank))
          .sort((a, b) => a.rank - b.rank || a.entry.title.length - b.entry.title.length)
          .slice(0, LIMIT)
          .map(({ entry, snippet }) => ({ entry, snippet }));
  active = 0;
  render();
}

function open() {
  if (!dialog || dialog.open) return;
  dialog.showModal();
  input?.select();
  void search();
}

document.addEventListener("click", (event) => {
  if (event.target instanceof Element && event.target.closest("[data-open-search]")) open();
});

document.addEventListener("keydown", (event) => {
  const typing =
    event.target instanceof HTMLElement &&
    (event.target.isContentEditable ||
      ["INPUT", "TEXTAREA", "SELECT"].includes(event.target.tagName));
  if ((event.key === "k" && (event.metaKey || event.ctrlKey)) || (event.key === "/" && !typing)) {
    event.preventDefault();
    open();
  }
});

input?.addEventListener("input", () => void search());

for (const button of filters) {
  button.addEventListener("click", () => {
    const chosen = button.dataset.filter ?? "";
    filter = groups.includes(chosen) ? chosen : "";
    for (const other of filters) other.setAttribute("aria-pressed", String(other === button));
    input?.focus();
    void search();
  });
}

input?.addEventListener("keydown", (event) => {
  if (event.key === "ArrowDown" || event.key === "ArrowUp") {
    event.preventDefault();
    if (results.length === 0) return;
    const step = event.key === "ArrowDown" ? 1 : -1;
    select((active + step + results.length) % results.length);
  } else if (event.key === "Enter") {
    const result = results[active];
    if (result === undefined) return;
    event.preventDefault();
    dialog?.close();
    void navigate(result.entry.href);
  }
});

list?.addEventListener("click", (event) => {
  if (event.target instanceof Element && event.target.closest("a")) dialog?.close();
});

dialog?.addEventListener("click", (event) => {
  if (event.target === dialog) dialog.close();
});
