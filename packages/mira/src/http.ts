import * as Data from "effect/Data";
import * as Duration from "effect/Duration";
import * as Effect from "effect/Effect";

import { call } from "./call.ts";

export type Method = "GET" | "HEAD" | "POST" | "PUT" | "PATCH" | "DELETE" | "OPTIONS";

export type Options = {
  /** @defaultValue "GET" */
  readonly method?: Method;
  readonly headers?: Readonly<Record<string, string>>;
  /**
   * Total time through redirects and the response download.
   * @defaultValue 20 seconds
   */
  readonly timeout?: Duration.Input;
} & (
  | { readonly body?: string; readonly json?: never }
  | {
      /** Sent as JSON, with `content-type: application/json` unless `headers` sets one. */
      readonly json: unknown;
      readonly body?: never;
    }
);

export type Response = {
  readonly status: number;
  /** True for status codes 200 through 299. */
  readonly ok: boolean;
  /** The URL after redirects. */
  readonly url: string;
  /** Names are lowercase. Repeated headers are joined with ", ". */
  readonly headers: Readonly<Record<string, string>>;
  /** The body decoded as UTF-8. Bodies over 8 MiB fail with `tooLarge`. */
  readonly text: string;
};

/** The request got no response. */
export class RequestError extends Data.TaggedError("RequestError")<{
  readonly reason: "invalidRequest" | "network" | "timeout" | "tooLarge";
  readonly url: string;
  readonly message: string;
}> {}

/** The response wasn't what `json` expected. */
export class ResponseError extends Data.TaggedError("ResponseError")<{
  readonly reason: "status" | "invalidJson";
  readonly response: Response;
  readonly message: string;
  readonly cause?: unknown;
}> {}

type Reply =
  | ({ readonly ok: true } & Omit<Response, "ok">)
  | {
      readonly ok: false;
      readonly reason: RequestError["reason"];
      readonly message: string;
    };

const timeoutMs = (timeout: Duration.Input | undefined): number | undefined => {
  if (timeout === undefined) return undefined;
  const millis = Duration.toMillis(timeout);
  if (millis < 0) throw new Error("the timeout is negative");
  return Math.min(Math.ceil(millis), Number.MAX_SAFE_INTEGER);
};

const send = (url: string, options: Options): object => {
  const { method, timeout } = options;
  const shared = { domain: "http", url, method, timeoutMs: timeoutMs(timeout) };
  if (options.json === undefined)
    return { ...shared, headers: options.headers, body: options.body };
  const typed = Object.keys(options.headers ?? {}).some(
    (name) => name.toLowerCase() === "content-type",
  );
  const headers = typed
    ? options.headers
    : { ...options.headers, "content-type": "application/json" };
  return { ...shared, headers, body: JSON.stringify(options.json) };
};

/**
 * Sends an HTTP or HTTPS request and downloads the whole response. Any status counts as a response;
 * check `ok`, or use `json` to require a 2xx. Stopping the script or interrupting the effect cancels
 * the request.
 *
 * @example
 * ```ts
 * import * as Http from "mira/http";
 *
 * export default function* () {
 *   yield* Http.request("https://discord.com/api/webhooks/...", {
 *     method: "POST",
 *     json: { content: "Finished farming." },
 *   });
 * }
 * ```
 */
export const request = (
  url: string,
  options: Options = {},
): Effect.Effect<Response, RequestError> =>
  Effect.try({
    try: () => send(url, options),
    catch: (cause) =>
      new RequestError({
        reason: "invalidRequest",
        url,
        message: `couldn't send the request: ${cause instanceof Error ? cause.message : String(cause)}`,
      }),
  }).pipe(
    Effect.flatMap((wire) => call<Reply>(wire)),
    Effect.flatMap((reply) => {
      if (!reply.ok) return Effect.fail(new RequestError({ ...reply, url }));
      const { ok: _, ...response } = reply;
      return Effect.succeed({ ...response, ok: response.status >= 200 && response.status < 300 });
    }),
  );

/**
 * Parses a 2xx response as JSON. Decode the result yourself, for example with `Schema.decodeUnknownEffect`.
 *
 * @example
 * ```ts
 * import * as Effect from "effect/Effect";
 * import * as Http from "mira/http";
 *
 * export default function* () {
 *   const release = yield* Http.request("https://api.github.com/repos/o/r/releases/latest").pipe(
 *     Effect.flatMap(Http.json),
 *   );
 * }
 * ```
 */
export const json = (response: Response): Effect.Effect<unknown, ResponseError> =>
  response.ok
    ? Effect.try({
        try: (): unknown => JSON.parse(response.text),
        catch: (cause) =>
          new ResponseError({
            reason: "invalidJson",
            response,
            message: `${response.url} didn't respond with JSON`,
            cause,
          }),
      })
    : Effect.fail(
        new ResponseError({
          reason: "status",
          response,
          message: `${response.url} responded with status ${response.status}`,
        }),
      );
