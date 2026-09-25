// A deterministic model for tests: scripted answers per `ai fn`.

import { WardError } from "./errors.ts";
import type { AiRequest, Completion } from "./model.ts";
import { encode } from "./schema.ts";

export class MockError extends WardError {}

/** An answer given as JSON text as-is, e.g. to test invalid output. */
export class Raw {
  readonly text: string;
  constructor(text: string) {
    this.text = text;
  }
}

/** An answer with what it cost. */
export class Usage {
  readonly answer: unknown;
  readonly tokens: number | null;
  readonly cost: number | null;
  constructor(answer: unknown, tokens: number | null = null, cost: number | null = 0) {
    this.answer = answer;
    this.tokens = tokens;
    this.cost = cost;
  }
}

/** Answers in turn: the first call gets the first. */
export class Seq {
  readonly answers: unknown[];
  constructor(...answers: unknown[]) {
    this.answers = answers;
  }
}

/** Answers each `ai fn` from `answers`, keyed by function name: a value (encoded as
 * JSON), a `Raw` text, a `Seq`, a `Usage`, an `Error` to throw, or a function of the
 * request returning one of those. */
export class MockModel {
  readonly answers: Record<string, unknown>;
  readonly calls: AiRequest[] = [];
  private readonly next = new Map<string, number>();

  constructor(answers: Record<string, unknown> = {}) {
    this.answers = answers;
  }

  static fromJson(text: string): MockModel {
    return new MockModel(JSON.parse(text) as Record<string, unknown>);
  }

  complete(request: AiRequest): string | Completion {
    this.calls.push(request);
    if (!(request.function in this.answers)) {
      throw new MockError(`the mock model has no answer for \`${request.function}\``);
    }
    const answer = this.text(request, this.answers[request.function]);
    return typeof answer === "string" ? { text: answer, tokens: null, cost: 0 } : answer;
  }

  private text(request: AiRequest, answer: unknown): string | Completion {
    if (answer instanceof Error) throw answer;
    if (answer instanceof Seq) {
      const n = this.next.get(request.function) ?? 0;
      if (n >= answer.answers.length) {
        throw new MockError(`the mock model ran out of answers for \`${request.function}\` after ${answer.answers.length}`);
      }
      this.next.set(request.function, n + 1);
      return this.text(request, answer.answers[n]);
    }
    if (answer instanceof Usage) {
      const inner = this.text(request, answer.answer);
      return { text: typeof inner === "string" ? inner : inner.text, tokens: answer.tokens, cost: answer.cost };
    }
    if (answer instanceof Raw) return answer.text;
    if (typeof answer === "function") return this.text(request, (answer as (r: AiRequest) => unknown)(request));
    return JSON.stringify(encode(answer));
  }
}
