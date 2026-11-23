export type AbortEvent = { readonly type: "abort"; readonly target: AbortSignal };
export type AbortListener = (this: AbortSignal, event: AbortEvent) => void;

let fire: (signal: AbortSignal, reason: unknown) => void;

const abortError = (): Error =>
  Object.assign(new Error("This operation was aborted"), { name: "AbortError" });

export class AbortSignal {
  onabort: AbortListener | null = null;
  #aborted = false;
  #reason: unknown = undefined;
  #listeners = new Map<AbortListener, boolean>();

  static {
    fire = (signal, reason) => signal.#fire(reason);
  }

  get aborted(): boolean {
    return this.#aborted;
  }

  get reason(): unknown {
    return this.#reason;
  }

  static abort(reason?: unknown): AbortSignal {
    const controller = new AbortController();
    controller.abort(reason);
    return controller.signal;
  }

  addEventListener(
    type: string,
    listener: AbortListener | null,
    options?: boolean | { readonly once?: boolean },
  ): void {
    if (type !== "abort" || listener === null || this.#listeners.has(listener)) return;
    this.#listeners.set(listener, typeof options === "object" && options.once === true);
  }

  removeEventListener(type: string, listener: AbortListener | null): void {
    if (type === "abort" && listener !== null) this.#listeners.delete(listener);
  }

  throwIfAborted(): void {
    if (this.#aborted) throw this.#reason;
  }

  #fire(reason: unknown): void {
    if (this.#aborted) return;
    this.#aborted = true;
    this.#reason = reason === undefined ? abortError() : reason;
    const event: AbortEvent = { type: "abort", target: this };
    const call = (listener: AbortListener) => {
      try {
        listener.call(this, event);
      } catch (error) {
        report(error);
      }
    };
    const listeners = [...this.#listeners.keys()];
    if (this.onabort !== null) call(this.onabort);
    for (const listener of listeners) {
      const once = this.#listeners.get(listener);
      if (once === undefined) continue;
      if (once) this.#listeners.delete(listener);
      call(listener);
    }
  }
}

let report: (error: unknown) => void = () => {};

export const reportListenerErrors = (to: (error: unknown) => void): void => {
  report = to;
};

export class AbortController {
  readonly signal = new AbortSignal();

  abort(reason?: unknown): void {
    fire(this.signal, reason);
  }
}
