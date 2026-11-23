import { type Route, routes } from "./routes.ts";

export type NavItem = Pick<Route, "title" | "href" | "kind">;
export type NavSection = { readonly title: Route["section"]; readonly items: readonly NavItem[] };

export async function navigation(): Promise<readonly NavSection[]> {
  const all = await routes();
  return (["Start", "Reference"] as const).map((title) => ({
    title,
    items: all.filter((route) => route.section === title),
  }));
}
