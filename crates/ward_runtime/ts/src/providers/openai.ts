// Models through the OpenAI Chat Completions API (with `fetch`), with the return
// type's schema as the response format.

import type { AiRequest, Completion } from "../model.ts";
import { cost, objectSchema, post, promptText } from "./common.ts";

export interface OpenAIOptions {
  prices?: [number, number] | null;
  /** Defaults to `OPENAI_API_KEY`. */
  apiKey?: string;
  baseUrl?: string;
}

export class OpenAI {
  readonly model: string;
  readonly prices: [number, number] | null;
  private readonly options: OpenAIOptions;

  constructor(model: string, options: OpenAIOptions = {}) {
    this.model = model;
    this.prices = options.prices ?? null;
    this.options = options;
  }

  async complete(request: AiRequest): Promise<Completion> {
    const key = this.options.apiKey ?? process.env.OPENAI_API_KEY ?? "";
    const data = await post(
      `${this.options.baseUrl ?? "https://api.openai.com"}/v1/chat/completions`,
      { authorization: `Bearer ${key}` },
      {
        model: this.model,
        messages: [{ role: "user", content: promptText(request) }],
        response_format: { type: "json_schema", json_schema: { name: "answer", schema: objectSchema(request.schema) } },
      },
    );
    const choices = (data.choices ?? []) as { message?: { content?: string } }[];
    const content = choices[0]?.message?.content ?? "";
    let text = content;
    try {
      const parsed = JSON.parse(content) as { value?: unknown };
      if (parsed && typeof parsed === "object" && "value" in parsed) text = JSON.stringify(parsed.value);
    } catch {
      // Not JSON: decoded (and rejected) as it is.
    }
    const usage = data.usage as { prompt_tokens?: number; completion_tokens?: number } | undefined;
    const input = usage?.prompt_tokens ?? 0;
    const output = usage?.completion_tokens ?? 0;
    return { text, tokens: usage ? input + output : null, cost: cost(this.prices, input, output) };
  }
}
