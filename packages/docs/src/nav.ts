import { type Route, routes } from "./routes.ts";

export type NavItem = Pick<Route, "title" | "href" | "kind">;
export type NavGroup = {
  readonly kind: "group";
  readonly title: string;
  readonly items: readonly NavItem[];
};
export type NavSection = {
  readonly title: Route["section"];
  readonly items: readonly (NavItem | NavGroup)[];
};

function grouped(routes: readonly Route[]): readonly (NavItem | NavGroup)[] {
  const entries: (NavItem | NavGroup)[] = [];
  const groups = new Map<string, NavItem[]>();
  for (const route of routes) {
    if (route.group === null) {
      entries.push(route);
      continue;
    }
    let items = groups.get(route.group);
    if (items === undefined) {
      items = [];
      groups.set(route.group, items);
      entries.push({ kind: "group", title: route.group, items });
    }
    items.push(route);
  }
  return entries;
}

export type Area = "docs" | "reference";

const AREAS = {
  docs: ["App", "Scripting"],
  reference: ["Reference"],
} as const satisfies Record<Area, readonly Route["section"][]>;

export function area(path: string): Area | null {
  if (path.startsWith("/docs/")) return "docs";
  if (path.startsWith("/reference/")) return "reference";
  return null;
}

export async function home(area: Area): Promise<string> {
  const [first] = await navigation(area);
  const [item] = first?.items ?? [];
  if (item === undefined) throw new Error(`the ${area} area has no pages`);
  return item.kind === "group" ? (item.items[0]?.href ?? "/") : item.href;
}

export async function navigation(area: Area): Promise<readonly NavSection[]> {
  const all = await routes();
  return AREAS[area].map((title) => ({
    title,
    items: grouped(all.filter((route) => route.section === title)),
  }));
}
