import assert from "node:assert/strict";
import { test } from "node:test";

import { parseBridge, renderAs3, renderRust } from "./gen-bridge.ts";

const world = (body: string) => ({
  path: "as3/src/mira/game/World.as",
  text: `package mira.game
{
  [BridgeNamespace("world")]
  public class World
  {
${body}
  }
}`,
});

test("exports register under their namespace, sorted by name", () => {
  const source = world(`
    [BridgeExport]
    public static function reload():void {}

    [BridgeExport("isReady")]
    public static function isLoaded():Boolean { return true; }

    [BridgeIgnore]
    public static function helper():void {}
`);
  assert.equal(
    renderAs3(parseBridge([source])),
    `// AUTO-GENERATED FILE. DO NOT EDIT.
package mira.generated
{
  import mira.Externalizer;
  import mira.game.World;

  public class BridgeRegistryGenerated
  {
    public static function register(external:Externalizer):void
    {
      external.externalize("world.isReady", World.isLoaded);
      external.externalize("world.reload", World.reload);
    }
  }
}
`,
  );
});

test("a static method without bridge metadata is rejected with its location", () => {
  const source = world(`
    public static function forgotten():void {}
`);
  assert.throws(() => parseBridge([source]), /forgotten.*World\.as:7/);
});

test("two exports with the same name are rejected", () => {
  const source = world(`
    [BridgeExport("reload")]
    public static function a():void {}

    [BridgeExport("reload")]
    public static function b():void {}
`);
  assert.throws(() => parseBridge([source]), /Duplicate bridge export "world\.reload"/);
});

test("a default the generator can't reproduce is rejected", () => {
  const source = world(`
    [BridgeExport]
    public static function at(point:Object = new Object()):void {}
`);
  assert.throws(() => parseBridge([source]), /Unsupported default value/);
});

test("rust bindings pass declared defaults when an argument is omitted", () => {
  const source = world(`
    [BridgeExport]
    public static function jump(cell:String, pad:String = "Spawn"):void {}
`);
  assert.match(renderRust(parseBridge([source])), /pad\.unwrap_or\("Spawn"\)/);
});

test("a parameter can take a Rust type that converts into a loader value", () => {
  const source = world(`
    [BridgeExport]
    [BridgeRustType("gameAction: GameAction")]
    public static function isActionAvailable(gameAction:String):Boolean { return true; }
`);
  assert.match(
    renderRust(parseBridge([source])),
    /game_action: crate::bridge::GameAction[\s\S]*Value::from\(game_action\)/,
  );
  const unknown = world(`
    [BridgeExport]
    [BridgeRustType("action: GameAction")]
    public static function isActionAvailable(gameAction:String):Boolean { return true; }
`);
  assert.throws(() => parseBridge([unknown]), /unknown parameter action/);
});
