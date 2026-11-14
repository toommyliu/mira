import * as Effect from "effect/Effect";
import * as Schema from "effect/Schema";
import * as Http from "mira/http";

const Echo = Schema.Struct({
  json: Schema.Struct({ hello: Schema.String }),
  headers: Schema.Record(Schema.String, Schema.String),
});

export default function* () {
  const ip = yield* Http.request("https://httpbin.org/ip").pipe(Effect.flatMap(Http.json));
  console.log(`ip: ${JSON.stringify(ip)}`);

  const echo = yield* Http.request("https://httpbin.org/post", {
    method: "POST",
    headers: { "X-Mira": "test" },
    json: { hello: "world" },
  }).pipe(Effect.flatMap(Http.json), Effect.flatMap(Schema.decodeUnknownEffect(Echo)));
  console.log(`echoed ${echo.json.hello}, user agent ${echo.headers["User-Agent"]}`);

  const missing = yield* Http.request("https://httpbin.org/status/404").pipe(
    Effect.flatMap(Http.json),
    Effect.flip,
  );
  console.log(`404: ${missing._tag} ${missing.reason}`);

  const notJson = yield* Http.request("https://httpbin.org/html").pipe(
    Effect.flatMap(Http.json),
    Effect.flip,
  );
  console.log(`html: ${notJson._tag} ${notJson.reason}`);

  const slow = yield* Http.request("https://httpbin.org/delay/5", { timeout: "1 second" }).pipe(
    Effect.flip,
  );
  console.log(`slow: ${slow._tag} ${slow.reason}: ${slow.message}`);

  const nowhere = yield* Http.request("https://mira.invalid/").pipe(Effect.flip);
  console.log(`nowhere: ${nowhere._tag} ${nowhere.reason}: ${nowhere.message}`);

  const typed = yield* Http.request("https://httpbin.org/status/503").pipe(
    Effect.flatMap(Http.json),
    Effect.catchTag("ResponseError", (error) =>
      Effect.succeed(`recovered from ${error.response.status}`),
    ),
  );
  console.log(`catchTag: ${String(typed)}`);
}
