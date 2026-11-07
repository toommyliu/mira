export type Native = {
  readonly call: (id: number, request: string) => void;
  readonly cancel: (id: number) => void;
};

export type Settled = { readonly ok: unknown } | { readonly error: string };

let native: Native | undefined;
let lastId = 0;
const pending = new Map<number, (settled: Settled) => void>();

export const connect = (host: Native): void => {
  native = host;
};

export const deliver = (id: number, reply: string): void => {
  const settle = pending.get(id);
  if (settle === undefined) return;
  pending.delete(id);
  const settled: Settled = JSON.parse(reply);
  settle(settled);
};

export const send = (request: object, settle: (settled: Settled) => void): (() => void) => {
  const host = native;
  if (host === undefined) throw new Error("Lyra's runtime isn't connected");
  const id = ++lastId;
  host.call(id, JSON.stringify(request));
  pending.set(id, settle);
  return () => {
    if (pending.delete(id)) host.cancel(id);
  };
};
