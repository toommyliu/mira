import assert from "node:assert/strict";
import { test } from "node:test";

import { parsePlatformTrait, referencedCrates, renderPlatform } from "./update-gpui.ts";

const platform = (body: string) => `use std::rc::Rc;

pub trait Platform: 'static {
${body}
}

pub trait Other {
    fn ignored(&self);
}
`;

test("every method is forwarded under its own signature, including ones with default bodies", () => {
  const methods = parsePlatformTrait(
    platform(`    fn text_system(&self) -> Arc<dyn PlatformTextSystem>;

    /// Opens a window. This fn mention is prose.
    fn open_window(
        &self,
        handle: AnyWindowHandle,
        options: WindowParams,
    ) -> anyhow::Result<Box<dyn PlatformWindow>>;

    fn set_activation_policy(&self, _policy: ActivationPolicy) {}

    fn on_quit(&self, callback: Box<dyn FnMut() -> bool>, _reason: &str) {
        let _ = callback;
    }

    fn prompt_for_paths(&self, options: PathPromptOptions) -> oneshot::Receiver<Result<Vec<PathBuf>>>;`),
  );
  assert.deepEqual(methods, [
    { name: "text_system", params: [], returns: "-> Arc<dyn PlatformTextSystem>" },
    {
      name: "open_window",
      params: [
        { name: "handle", type: "AnyWindowHandle" },
        { name: "options", type: "WindowParams" },
      ],
      returns: "-> anyhow::Result<Box<dyn PlatformWindow>>",
    },
    {
      name: "set_activation_policy",
      params: [{ name: "policy", type: "ActivationPolicy" }],
      returns: "",
    },
    {
      name: "on_quit",
      params: [
        { name: "callback", type: "Box<dyn FnMut() -> bool>" },
        { name: "reason", type: "&str" },
      ],
      returns: "",
    },
    {
      name: "prompt_for_paths",
      params: [{ name: "options", type: "PathPromptOptions" }],
      returns: "-> oneshot::Receiver<Result<Vec<PathBuf>>>",
    },
  ]);

  const rendered = renderPlatform(methods);
  assert.match(
    rendered,
    /fn text_system\(&self\) -> Arc<dyn PlatformTextSystem> \{ self\.text_system\.clone\(\) \}/,
  );
  assert.match(rendered, /\{ self\.mac\.set_activation_policy\(policy\) \}/);
  assert.match(rendered, /\{ self\.mac\.on_quit\(callback, reason\) \}/);
  assert.match(rendered, /^use futures::channel::oneshot;$/m);
  assert.match(rendered, /^use anyhow::Result;$/m);
  assert.match(
    rendered,
    /^use gpui::\{Platform, PlatformTextSystem, PlatformWindow, AnyWindowHandle, WindowParams, ActivationPolicy, PathPromptOptions\};$/m,
  );
});

test("only methods that exist on macOS are forwarded", () => {
  const methods = parsePlatformTrait(
    platform(`    fn text_system(&self) -> Arc<dyn PlatformTextSystem>;
    #[cfg(any(target_os = "linux", target_os = "freebsd"))]
    fn read_from_primary(&self) -> Option<ClipboardItem>;
    #[cfg(target_os = "macos")]
    fn read_from_find_pasteboard(&self) -> Option<ClipboardItem>;
    fn quit(&self);`),
  );
  assert.deepEqual(
    methods.map((method) => method.name),
    ["text_system", "read_from_find_pasteboard", "quit"],
  );
});

test("a declaration the generator can't forward stops it instead of being skipped", () => {
  for (const body of [
    `    #[cfg(feature = "screen-capture")]\n    fn sources(&self);`,
    `    #[cfg(not(target_os = "macos"))]\n    fn elsewhere(&self);`,
    "    unsafe fn raw(&self);",
    "    fn generic<T>(&self, value: T);",
    "    fn consume(self);",
  ]) {
    assert.throws(() => parsePlatformTrait(platform(body)), Error, body);
  }
  assert.throws(() => renderPlatform([]), /no text_system/);
});

test("crates named in forwarded signatures are reported so the platform crate can depend on them", () => {
  const methods = parsePlatformTrait(
    platform(`    fn text_system(&self) -> Arc<dyn PlatformTextSystem>;
    fn debug_state(&self) -> serde_json::Value {}
    fn prompt(&self) -> oneshot::Receiver<anyhow::Result<std::path::PathBuf>>;`),
  );
  assert.deepEqual(referencedCrates(methods), ["serde_json", "futures", "anyhow"]);
});
