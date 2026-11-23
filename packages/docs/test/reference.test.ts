import assert from "node:assert/strict";
import { fileURLToPath } from "node:url";
import { test } from "node:test";

import { reference } from "../src/reference/model.ts";
import { convert } from "../src/reference/project.ts";
import { signatureTokens } from "../src/reference/tokens.ts";

const fixture = (file: string) => fileURLToPath(new URL(`fixture/${file}`, import.meta.url));
const project = await convert([fixture("mira.ts"), fixture("http.ts")], fixture("tsconfig.json"));
const { pages, links } = reference(project, ["mira", "mira/http"]);
const page = (slug: string) => {
  const found = pages.find((candidate) => candidate.slug === slug);
  assert.ok(found, `no page /reference/${slug}`);
  return found;
};

test("each namespace gets a page after its module's page", () => {
  assert.deepEqual(
    pages.map((candidate) => candidate.slug),
    ["", "combat", "world", "http"],
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
    ["find", "cells.list"],
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
