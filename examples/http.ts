import * as Http from "mira/http";
import * as S from "mira/schema";

const Echo = S.object({
  json: S.object({ hello: S.string() }),
  headers: S.record(S.string()),
});

function* fetchJson(url: string, options?: Http.Options) {
  return yield* Http.json(yield* Http.request(url, options));
}

function* failure(url: string, options?: Http.Options) {
  try {
    yield* fetchJson(url, options);
    return "none";
  } catch (error) {
    if (error instanceof Http.RequestError || error instanceof Http.ResponseError) {
      return `${error._tag} ${error.reason}: ${error.message}`;
    }
    throw error;
  }
}

export default function* () {
  console.log(`ip: ${JSON.stringify(yield* fetchJson("https://httpbin.org/ip"))}`);

  const echo = yield* Echo.parse(
    yield* fetchJson("https://httpbin.org/post", {
      method: "POST",
      headers: { "X-Mira": "test" },
      json: { hello: "world" },
    }),
  );
  console.log(`echoed ${echo.json.hello}, user agent ${echo.headers["User-Agent"]}`);

  console.log(`404: ${yield* failure("https://httpbin.org/status/404")}`);
  console.log(`html: ${yield* failure("https://httpbin.org/html")}`);
  console.log(`slow: ${yield* failure("https://httpbin.org/delay/5", { timeout: "1 second" })}`);
  console.log(`nowhere: ${yield* failure("https://mira.invalid/")}`);

  try {
    yield* fetchJson("https://httpbin.org/status/503");
  } catch (error) {
    if (!(error instanceof Http.ResponseError)) throw error;
    console.log(`recovered from ${error.response.status}`);
  }
}
