// Trust labels at the host boundary. A value the host passes in is untrusted by
// default, so a parameter that reaches a sink only accepts a value the host vouches
// for by wrapping it: `handle(email, new Trusted(to))`.

import { TrustError } from "./errors.ts";

export class Trusted<T> {
  readonly value: T;
  constructor(value: T) {
    this.value = value;
  }
}

export function trusted<T>(value: T): Trusted<T> {
  return new Trusted(value);
}

export function vouched<T>(value: T | Trusted<T>, param: string, fn: string): T {
  if (value instanceof Trusted) return value.value;
  throw new TrustError(
    `\`${fn}\` sends its parameter \`${param}\` to a sensitive action, so it only accepts ` +
      "a value the caller vouches for: pass `new Trusted(value)`",
  );
}
