export type Native = {
  readonly call: (id: number, request: string) => void;
  readonly cancel: (id: number) => void;
  readonly ask: (id: number, request: string) => void;
  readonly dismiss: (id: number) => void;
};

export type Settled = { readonly ok: unknown } | { readonly error: string };

export type Channel = "call" | "ask";

const MAX_OPEN_DIALOGS = 100;

let native: Native | undefined;
let lastId = 0;
const pending = new Map<number, (settled: Settled) => void>();
const dialogs = new Set<number>();

export const connect = (host: Native): void => {
  native = host;
};

export const deliver = (id: number, reply: string): void => {
  const settle = pending.get(id);
  if (settle === undefined) return;
  pending.delete(id);
  dialogs.delete(id);
  const settled: Settled = JSON.parse(reply);
  settle(settled);
};

export const send = (
  channel: Channel,
  request: object,
  settle: (settled: Settled) => void,
): (() => void) => {
  const host = native;
  if (host === undefined) throw new Error("Mira's runtime isn't connected");
  if (channel === "ask" && dialogs.size >= MAX_OPEN_DIALOGS)
    throw new Error(`A script can have at most ${MAX_OPEN_DIALOGS} dialogs open at once`);
  const id = ++lastId;
  host[channel](id, JSON.stringify(request));
  pending.set(id, settle);
  if (channel === "ask") dialogs.add(id);
  return () => {
    if (!pending.delete(id)) return;
    dialogs.delete(id);
    if (channel === "call") host.cancel(id);
    else host.dismiss(id);
  };
};
