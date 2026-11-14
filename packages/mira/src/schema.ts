import * as Data from "effect/Data";
import * as Effect from "effect/Effect";
import * as Native from "effect/Schema";
import type * as SchemaAST from "effect/SchemaAST";
import * as SchemaIssue from "effect/SchemaIssue";

export type Path = readonly (string | number)[];

export type Issue = { readonly path: Path; readonly message: string };

/** The value didn't match the schema. `message` lists every problem, one per line. */
export class ParseError extends Data.TaggedError("ParseError")<{
  readonly issues: readonly Issue[];
  readonly message: string;
}> {}

/** A schema that can only be an object field, made by `optional()` or `default()`. */
export interface Field<N extends Native.Top> {
  /** The Effect schema underneath, for use with `effect/Schema`. */
  readonly schema: N;
  /**
   * Checks `input` and returns it typed, with defaults filled in and unknown object keys removed.
   *
   * @example
   * ```ts
   * const config = yield* Config.parse(JSON.parse(text));
   * ```
   */
  readonly parse: (input: unknown) => Effect.Effect<N["Type"], ParseError, N["DecodingServices"]>;
  readonly is: (input: unknown) => input is N["Type"];
}

interface Methods<N extends Native.Top, Self> extends Field<N> {
  /** Allows `undefined`, and lets an object leave the key out. */
  optional(): Field<Native.optional<N>>;
  nullable(): Schema<Native.NullOr<N>>;
  /** Uses `value` when the input is `undefined` or an object leaves the key out. */
  default(value: N["Type"]): Field<Native.withDecodingDefaultType<N>>;
  refine(predicate: (value: N["Type"]) => boolean, message?: string): Self;
}

export interface Schema<N extends Native.Top> extends Methods<N, Schema<N["Rebuild"]>> {}

export interface StringSchema extends Methods<Native.String, StringSchema> {
  min(length: number, message?: string): StringSchema;
  max(length: number, message?: string): StringSchema;
  regex(pattern: RegExp, message?: string): StringSchema;
}

export interface NumberSchema extends Methods<Native.Number, NumberSchema> {
  int(message?: string): NumberSchema;
  min(value: number, message?: string): NumberSchema;
  max(value: number, message?: string): NumberSchema;
}

export interface ArraySchema<E extends Native.Top> extends Methods<
  Native.$Array<E>,
  ArraySchema<E>
> {
  min(length: number, message?: string): ArraySchema<E>;
  max(length: number, message?: string): ArraySchema<E>;
}

type AnySchema = { readonly schema: Native.Top };

type Value = {
  readonly schema: Native.Top & {
    readonly "~type.optionality": "required";
    readonly "~encoded.optionality": "required";
  };
};

type Fields<F extends Readonly<Record<string, AnySchema>>> = {
  readonly [K in keyof F]: F[K]["schema"];
};

type Members<M extends readonly Value[]> = { readonly [K in keyof M]: M[K]["schema"] };

/**
 * The type a schema parses to.
 *
 * @example
 * ```ts
 * type Config = S.Infer<typeof Config>;
 * ```
 */
export type Infer<T extends AnySchema> = T["schema"]["Type"];

const formatter = SchemaIssue.makeFormatterStandardSchemaV1();

const maxLine = 200;

const maxLines = 20;

const identifier = /^[A-Za-z_$][\w$]*$/;

const formatPath = (path: Path): string =>
  path
    .map((key, index) => {
      if (typeof key === "number") return `[${key}]`;
      if (!identifier.test(key)) return `[${JSON.stringify(key)}]`;
      return index === 0 ? key : `.${key}`;
    })
    .join("");

const toParseError = (issue: SchemaIssue.Issue): ParseError => {
  const issues = formatter(issue).issues.map(({ path = [], message }): Issue => ({
    path: path.map((segment) => {
      const key = typeof segment === "object" ? segment.key : segment;
      return typeof key === "number" ? key : String(key);
    }),
    message,
  }));
  const lines = issues.slice(0, maxLines).map(({ path, message }) => {
    const line = path.length === 0 ? message : `${formatPath(path)}: ${message}`;
    return line.length > maxLine ? `${line.slice(0, maxLine - 1)}…` : line;
  });
  if (issues.length > maxLines) lines.push(`and ${issues.length - maxLines} more`);
  return new ParseError({ issues, message: lines.join("\n") });
};

const annotations = (message: string | undefined) =>
  message === undefined ? undefined : { message };

const field = <N extends Native.Top>(schema: N): Field<N> => {
  const decode = Native.decodeUnknownEffect(schema, { errors: "all", reportInput: true });
  return {
    schema,
    parse: (input) => decode(input).pipe(Effect.mapError((error) => toParseError(error.issue))),
    is: Native.is(schema),
  };
};

const make = <N extends Native.Top, Self>(
  schema: N,
  rebuild: (schema: N["Rebuild"]) => Self,
): Methods<N, Self> => ({
  ...field(schema),
  optional: () => field(Native.optional(schema)),
  nullable: () => plain(Native.NullOr(schema)),
  default: (value) => field(schema.pipe(Native.withDecodingDefaultType(Effect.succeed(value)))),
  refine: (predicate, message) =>
    rebuild(schema.check(Native.makeFilter<N["Type"]>(predicate, annotations(message)))),
});

const plain = <N extends Native.Top>(schema: N): Schema<N> => make(schema, plain);

const stringFrom = (schema: Native.String): StringSchema => ({
  ...make(schema, stringFrom),
  min: (length, message) =>
    stringFrom(schema.check(Native.isMinLength(length, annotations(message)))),
  max: (length, message) =>
    stringFrom(schema.check(Native.isMaxLength(length, annotations(message)))),
  regex: (pattern, message) =>
    stringFrom(schema.check(Native.isPattern(pattern, annotations(message)))),
});

const numberFrom = (schema: Native.Number): NumberSchema => ({
  ...make(schema, numberFrom),
  int: (message) => numberFrom(schema.check(Native.isInt(annotations(message)))),
  min: (value, message) =>
    numberFrom(schema.check(Native.isGreaterThanOrEqualTo(value, annotations(message)))),
  max: (value, message) =>
    numberFrom(schema.check(Native.isLessThanOrEqualTo(value, annotations(message)))),
});

const arrayFrom = <E extends Native.Top>(schema: Native.$Array<E>): ArraySchema<E> => ({
  ...make(schema, arrayFrom),
  min: (length, message) =>
    arrayFrom(schema.check(Native.isMinLength(length, annotations(message)))),
  max: (length, message) =>
    arrayFrom(schema.check(Native.isMaxLength(length, annotations(message)))),
});

/** Any string, including `""`. */
export const string = (): StringSchema => stringFrom(Native.String);

/** Any number except `NaN` and infinity. */
export const number = (): NumberSchema => numberFrom(Native.Finite);

export const boolean = (): Schema<Native.Boolean> => plain(Native.Boolean);

/** Accepts anything. The parsed type is `unknown`. */
export const unknown = (): Schema<Native.Unknown> => plain(Native.Unknown);

/**
 * Exactly one of the given values.
 *
 * @example
 * ```ts
 * const mode = S.literal("farm", "quest");
 * ```
 */
export const literal = <
  const L extends readonly [SchemaAST.LiteralValue, ...SchemaAST.LiteralValue[]],
>(
  ...values: L
): Schema<Native.Literals<L>> => plain(Native.Literals(values));

/**
 * An object with these keys. Keys that aren't listed are removed when parsing.
 *
 * @example
 * ```ts
 * const Config = S.object({
 *   name: S.string().min(1),
 *   kills: S.number().int().min(1).default(10),
 *   note: S.string().optional(),
 * });
 * ```
 */
export const object = <const F extends Readonly<Record<string, AnySchema>>>(
  fields: F,
): Schema<Native.Struct<Fields<F>>> =>
  plain(
    Native.Struct(
      Object.fromEntries(
        Object.entries(fields).map(([key, field]) => [key, field.schema]),
      ) as Fields<F>,
    ),
  );

export const array = <E extends Value>(element: E): ArraySchema<E["schema"]> =>
  arrayFrom(Native.Array(element.schema));

/** An object whose keys can be any string. */
export const record = <V extends Value>(
  value: V,
): Schema<Native.$Record<Native.String, V["schema"]>> =>
  plain(Native.Record(Native.String, value.schema));

/** Matches the first of `members` that accepts the value. */
export const union = <const M extends readonly [Value, Value, ...Value[]]>(
  ...members: M
): Schema<Native.Union<Members<M>>> =>
  plain(Native.Union(members.map((member) => member.schema) as unknown as Members<M>));
