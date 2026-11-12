interface Console {
  debug(...values: unknown[]): void;
  info(...values: unknown[]): void;
  log(...values: unknown[]): void;
  warn(...values: unknown[]): void;
  error(...values: unknown[]): void;
}

declare var console: Console;

declare function setTimeout<Args extends unknown[]>(
  callback: (...args: Args) => void,
  delay?: number,
  ...args: Args
): number;
declare function setInterval<Args extends unknown[]>(
  callback: (...args: Args) => void,
  delay?: number,
  ...args: Args
): number;
declare function clearTimeout(id: number | undefined): void;
declare function clearInterval(id: number | undefined): void;

interface Performance {
  now(): number;
}

declare var performance: Performance;

interface AbortEvent {
  readonly type: "abort";
  readonly target: AbortSignal;
}

interface AbortSignal {
  readonly aborted: boolean;
  readonly reason: unknown;
  onabort: ((this: AbortSignal, event: AbortEvent) => void) | null;
  addEventListener(
    type: "abort",
    listener: ((this: AbortSignal, event: AbortEvent) => void) | null,
    options?: boolean | { readonly once?: boolean },
  ): void;
  removeEventListener(
    type: "abort",
    listener: ((this: AbortSignal, event: AbortEvent) => void) | null,
  ): void;
  throwIfAborted(): void;
}

declare var AbortSignal: {
  readonly prototype: AbortSignal;
  abort(reason?: unknown): AbortSignal;
};

interface AbortController {
  readonly signal: AbortSignal;
  abort(reason?: unknown): void;
}

declare var AbortController: {
  readonly prototype: AbortController;
  new (): AbortController;
};

interface TextEncoder {
  readonly encoding: "utf-8";
  encode(input?: string): Uint8Array;
}

declare var TextEncoder: {
  readonly prototype: TextEncoder;
  new (): TextEncoder;
};

interface TextDecoder {
  readonly encoding: "utf-8";
  decode(input?: ArrayBuffer | ArrayBufferView): string;
}

declare var TextDecoder: {
  readonly prototype: TextDecoder;
  new (): TextDecoder;
};
