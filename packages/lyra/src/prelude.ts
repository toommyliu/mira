import { AbortController, AbortSignal, reportListenerErrors } from "./abort.ts";
import { connect, deliver, type Native } from "./native.ts";

type Level = "debug" | "info" | "warn" | "error";

type Host = Native & {
  readonly now: () => number;
  readonly log: (level: Level, message: string) => void;
};

type Timer = { readonly at: number; readonly run: () => void };

const describe = (value: unknown): string => {
  if (typeof value === "string") return value;
  if (value instanceof Error) {
    const header = `${value.name}: ${value.message}`;
    return value.stack ? `${header}\n${value.stack}` : header;
  }
  try {
    return JSON.stringify(value) ?? String(value);
  } catch {
    return String(value);
  }
};

const encodeUtf8 = (text: string): Uint8Array => {
  const bytes: number[] = [];
  for (const character of text) {
    const code = character.codePointAt(0) ?? 0;
    if (code < 0x80) bytes.push(code);
    else if (code < 0x800) bytes.push(0xc0 | (code >> 6), 0x80 | (code & 63));
    else if (code < 0x10000)
      bytes.push(0xe0 | (code >> 12), 0x80 | ((code >> 6) & 63), 0x80 | (code & 63));
    else
      bytes.push(
        0xf0 | (code >> 18),
        0x80 | ((code >> 12) & 63),
        0x80 | ((code >> 6) & 63),
        0x80 | (code & 63),
      );
  }
  return new Uint8Array(bytes);
};

const decodeUtf8 = (bytes: Uint8Array): string => {
  let text = "";
  let index = 0;
  const next = () => (bytes[index++] ?? 0) & 63;
  while (index < bytes.length) {
    let code = bytes[index++] ?? 0;
    if (code >= 0xf0) code = ((code & 7) << 18) | (next() << 12) | (next() << 6) | next();
    else if (code >= 0xe0) code = ((code & 15) << 12) | (next() << 6) | next();
    else if (code >= 0xc0) code = ((code & 31) << 6) | next();
    text += String.fromCodePoint(code);
  }
  return text;
};

export function install(host: Host) {
  connect(host);
  const timers = new Map<number, Timer>();
  let lastId = 0;
  const schedule = (id: number, delay: unknown, run: () => void) => {
    timers.set(id, { at: host.now() + Math.max(0, Number(delay) || 0), run });
  };
  const clear = (id: unknown) => {
    timers.delete(Number(id));
  };
  const console = Object.fromEntries(
    (["debug", "info", "warn", "error"] as const).map((level) => [
      level,
      (...values: unknown[]) => host.log(level, values.map(describe).join(" ")),
    ]),
  );
  reportListenerErrors((error) => host.log("error", describe(error)));
  Object.assign(globalThis, {
    AbortController,
    AbortSignal,
    setTimeout: (callback: (...args: unknown[]) => void, delay?: unknown, ...args: unknown[]) => {
      const id = ++lastId;
      schedule(id, delay, () => callback(...args));
      return id;
    },
    setInterval: (callback: (...args: unknown[]) => void, delay?: unknown, ...args: unknown[]) => {
      const id = ++lastId;
      const tick = () => {
        schedule(id, Math.max(1, Number(delay) || 0), tick);
        callback(...args);
      };
      schedule(id, Math.max(1, Number(delay) || 0), tick);
      return id;
    },
    clearTimeout: clear,
    clearInterval: clear,
    console: { ...console, log: console["info"] },
    performance: { now: () => host.now() },
    TextEncoder: class {
      get encoding() {
        return "utf-8";
      }
      encode(text = "") {
        return encodeUtf8(String(text));
      }
    },
    TextDecoder: class {
      get encoding() {
        return "utf-8";
      }
      decode(input?: ArrayBuffer | ArrayBufferView) {
        if (input === undefined) return "";
        const bytes = ArrayBuffer.isView(input)
          ? new Uint8Array(input.buffer, input.byteOffset, input.byteLength)
          : new Uint8Array(input);
        return decodeUtf8(bytes);
      }
    },
  });
  return {
    deliver,
    nextTimer: (): number => {
      let earliest = Infinity;
      for (const timer of timers.values()) earliest = Math.min(earliest, timer.at);
      return earliest === Infinity ? -1 : Math.max(0, earliest - host.now());
    },
    runTimers: (): void => {
      const now = host.now();
      for (const [id, timer] of Array.from(timers)) {
        if (timer.at <= now && timers.get(id) === timer) {
          timers.delete(id);
          timer.run();
        }
      }
    },
  };
}
