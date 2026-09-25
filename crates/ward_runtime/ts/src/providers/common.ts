// What the providers share: the answer's wrapper, costs, and HTTP errors.

import { ModelError, ModelUnavailable, RateLimited } from "../errors.ts";
import type { AiRequest } from "../model.ts";

/** APIs want an object at the top of a structured-output schema, so the answer is
 * wrapped as `{"value": ...}`; `$defs` stay at the top so `$ref`s still resolve. */
export function objectSchema(schema: Record<string, unknown>): Record<string, unknown> {
  const { $defs, ...inner } = structuredClone(schema) as Record<string, unknown>;
  const out: Record<string, unknown> = {
    type: "object",
    properties: { value: inner },
    required: ["value"],
    additionalProperties: false,
  };
  if ($defs) out.$defs = $defs;
  return out;
}

/** Dollars, from prices per million input and output tokens; unknown without prices. */
export function cost(prices: [number, number] | null, input: number, output: number): number | null {
  return prices === null ? null : (input * prices[0] + output * prices[1]) / 1_000_000;
}

export function promptText(request: AiRequest): string {
  let text = request.prompt;
  if (request.errors.length) text += `\n\nYour previous answer was rejected: ${request.errors[request.errors.length - 1]}`;
  return text;
}

/** POSTs JSON; HTTP and network errors become `ModelError`s the runtime can retry. */
export async function post(url: string, headers: Record<string, string>, body: unknown): Promise<Record<string, unknown>> {
  let response: Response;
  try {
    response = await fetch(url, {
      method: "POST",
      headers: { "content-type": "application/json", ...headers },
      body: JSON.stringify(body),
    });
  } catch (e) {
    throw new ModelUnavailable(`${url}: ${e instanceof Error ? e.message : String(e)}`);
  }
  const text = await response.text();
  if (!response.ok) {
    const message = `HTTP ${response.status}: ${text.slice(0, 500)}`;
    if (response.status === 429) throw new RateLimited(message, response.status);
    if (response.status >= 500) throw new ModelUnavailable(message, response.status);
    throw new ModelError(message, response.status);
  }
  return JSON.parse(text) as Record<string, unknown>;
}
