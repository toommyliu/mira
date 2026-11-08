type AbortEvent = { readonly type: "abort"; readonly target: AbortSignal };
type Listener = (this: AbortSignal, event: AbortEvent) => void;

const fire = Symbol("fire");

const abortError = (): Error =>
  Object.assign(new Error("This operation was aborted"), { name: "AbortError" });

export class AbortSignal {
  aborted = false;
  reason: unknown = undefined;
  onabort: Listener | null = null;
  #listeners = new Map<Listener, boolean>();

  static abort(reason?: unknown): AbortSignal {
    const controller = new AbortController();
    controller.abort(reason);
    return controller.signal;
  }

  addEventListener(
    type: string,
    listener: Listener | null,
    options?: boolean | { readonly once?: boolean },
  ): void {
    if (type !== "abort" || listener === null || this.#listeners.has(listener)) return;
    this.#listeners.set(listener, typeof options === "object" && options.once === true);
  }

  removeEventListener(type: string, listener: Listener | null): void {
    if (type === "abort" && listener !== null) this.#listeners.delete(listener);
  }

  throwIfAborted(): void {
    if (this.aborted) throw this.reason;
  }

  [fire](reason: unknown, report: (error: unknown) => void): void {
    if (this.aborted) return;
    this.aborted = true;
    this.reason = reason === undefined ? abortError() : reason;
    const event: AbortEvent = { type: "abort", target: this };
    const call = (listener: Listener) => {
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
    this.signal[fire](reason, report);
  }
}
