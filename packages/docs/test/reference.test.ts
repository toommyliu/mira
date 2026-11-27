import assert from "node:assert/strict";
import { fileURLToPath } from "node:url";
import { test } from "node:test";

import { commentText } from "../src/reference/comment.ts";
import { reference } from "../src/reference/model.ts";
import { convert } from "../src/reference/project.ts";
import { signatureTokens } from "../src/reference/tokens.ts";

const fixture = (file: string) => fileURLToPath(new URL(`fixture/${file}`, import.meta.url));
const modules = [
  {
    specifier: "mira",
    slug: "",
    origin: "runtime",
    description: undefined,
    path: fixture("mira.ts"),
  },
  {
    specifier: "mira/http",
    slug: "http",
    origin: "runtime",
    description: undefined,
    path: fixture("http.ts"),
  },
  {
    specifier: "@mira/dailies",
    slug: "packages/dailies",
    origin: "vendored",
    description: "Daily and weekly quests.",
    path: fixture("dailies.ts"),
  },
] as const;
const project = await convert(modules, fixture("tsconfig.json"));
const { pages, links } = reference(project, modules);
const page = (slug: string) => {
  const found = pages.find((candidate) => candidate.slug === slug);
  assert.ok(found, `no page /reference/${slug}`);
  return found;
};

test("each namespace gets a page after its module's page", () => {
  assert.deepEqual(
    pages.map((candidate) => candidate.slug),
    ["", "combat", "world", "http", "packages/dailies"],
  );
});

test("a type lives with the one namespace that uses it, directly or through another type", () => {
  assert.deepEqual(
    page("combat").types.map((type) => type.name),
    ["Aura", "Target", "KillOptions", "KillError"],
  );
  assert.ok([...links.values()].includes("/reference/combat/#Aura"));
});

test("a type used by several namespaces lives on the module page", () => {
  assert.deepEqual(
    page("").types.map((type) => type.name),
    ["Query"],
  );
});

test("a @group tag puts a shared type on that namespace's page", () => {
  assert.deepEqual(
    page("world").types.map((type) => type.name),
    ["Signal", "SignalType"],
  );
  assert.ok([...links.values()].includes("/reference/world/#Signal"));
});

test("a union tagged by a literal field lists each variant with its fields", () => {
  const signal = page("world").types.find((type) => type.name === "Signal");
  assert.ok(signal?.body.kind === "variants");
  assert.deepEqual(
    signal.body.variants.map((variant) => ({
      anchor: variant.anchor,
      summary: commentText(variant.comment),
      fields: variant.properties.map((field) => [field.path, commentText(field.comment)]),
    })),
    [
      { anchor: "Signal.open", summary: "A door opened.", fields: [["door", "Which door."]] },
      { anchor: "Signal.close", summary: "", fields: [] },
    ],
  );
});

test("a type parameter's constraint keeps the alias it was written with", () => {
  const on = page("world").members.find((member) => member.path === "on");
  assert.ok(on?.body.kind === "call");
  const [signature] = on.body.signatures;
  assert.ok(signature);
  assert.match(
    signatureTokens("World.on", signature, links)
      .map((token) => token.text)
      .join(""),
    /^yield\* World\.on<const T extends SignalType>\(/,
  );
});

test("a member shows what yield* gives back and how it fails", () => {
  const kill = page("combat").members.find((member) => member.path === "kill");
  assert.ok(kill?.body.kind === "call");
  const [signature] = kill.body.signatures;
  assert.ok(signature);
  assert.equal(
    signatureTokens("Combat.kill", signature, links)
      .map((token) => token.text)
      .join(""),
    "yield* Combat.kill(query: Query, options?: KillOptions): boolean",
  );
  assert.ok(signature.result.kind === "yields");
  assert.equal(
    signature.result.error?.type === "reference" && signature.result.error.name,
    "KillError",
  );
});

test("members of a nested object read as paths", () => {
  assert.deepEqual(
    page("world").members.map((member) => member.path),
    ["find", "cells.list", "on"],
  );
});

test("a module without namespaces is one page imported as a namespace", () => {
  const http = page("http");
  assert.deepEqual(http.binding, { kind: "namespace", name: "Http" });
  assert.deepEqual(
    http.members.map((member) => member.path),
    ["request"],
  );
  assert.deepEqual(
    http.types.map((type) => type.name),
    ["Response", "RequestError"],
  );
});

test("a vendored package's generator shows what yield* gives back", () => {
  const dailies = page("packages/dailies");
  assert.equal(dailies.origin, "vendored");
  assert.equal(commentText(dailies.comment), "Daily and weekly quests.");
  const wheel = dailies.members.find((member) => member.path === "wheelOfDoom");
  assert.ok(wheel?.body.kind === "call");
  const [signature] = wheel.body.signatures;
  assert.ok(signature);
  assert.equal(
    signatureTokens("Dailies.wheelOfDoom", signature, links)
      .map((token) => token.text)
      .join(""),
    "yield* Dailies.wheelOfDoom(): Spin",
  );
  assert.ok([...links.values()].includes("/reference/packages/dailies/#Spin"));
});
