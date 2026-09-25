// Claude, through the Anthropic Messages API (with `fetch`, no SDK). The return
// type's schema is a tool the model must call, so answers are structured.

import type { AiRequest, Completion } from "../model.ts";
import { cost, objectSchema, post, promptText } from "./common.ts";

export const DEFAULT_MODEL = "claude-sonnet-5";

export interface AnthropicOptions {
  maxTokens?: number;
  /** Dollars per million input and output tokens, for `cost` budgets. */
  prices?: [number, number] | null;
  /** Defaults to `ANTHROPIC_API_KEY`. */
  apiKey?: string;
  baseUrl?: string;
}

export class Anthropic {
  readonly model: string;
  readonly prices: [number, number] | null;
  private readonly options: AnthropicOptions;

  constructor(model: string = DEFAULT_MODEL, options: AnthropicOptions = {}) {
    this.model = model;
    this.prices = options.prices ?? null;
    this.options = options;
  }

  async complete(request: AiRequest): Promise<Completion> {
    const key = this.options.apiKey ?? process.env.ANTHROPIC_API_KEY ?? "";
    const data = await post(
      `${this.options.baseUrl ?? "https://api.anthropic.com"}/v1/messages`,
      { "x-api-key": key, "anthropic-version": "2023-06-01" },
      {
        model: this.model,
        max_tokens: this.options.maxTokens ?? 4096,
        messages: [{ role: "user", content: promptText(request) }],
        tools: [{ name: "answer", description: `Give the answer of \`${request.function}\`.`, input_schema: objectSchema(request.schema) }],
        tool_choice: { type: "tool", name: "answer" },
      },
    );
    const content = (data.content ?? []) as { type: string; input?: { value?: unknown }; text?: string }[];
    const call = content.find((b) => b.type === "tool_use");
    const usage = (data.usage ?? {}) as { input_tokens?: number; output_tokens?: number };
    const input = usage.input_tokens ?? 0;
    const output = usage.output_tokens ?? 0;
    const text =
      call?.input && "value" in call.input
        ? JSON.stringify(call.input.value)
        : content.map((b) => b.text ?? "").join("");
    return { text, tokens: input + output, cost: cost(this.prices, input, output) };
  }
}
