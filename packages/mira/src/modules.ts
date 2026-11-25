export const modules = [
  { specifier: "mira", source: "mira", out: "mira/index" },
  { specifier: "mira/http", source: "http", out: "mira/http" },
  { specifier: "mira/schema", source: "schema", out: "mira/schema" },
] as const;
