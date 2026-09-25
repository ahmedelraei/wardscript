// The interface between `ai fn`s and language models.

export interface AiRequest {
  /** The `ai fn` being called. */
  function: string;
  /** Its prompt, with the arguments filled in. */
  prompt: string;
  /** JSON Schema the answer must match. */
  schema: Record<string, unknown>;
  /** 0 for the first try of a model; retries count up. */
  attempt: number;
  /** Why each earlier answer was rejected. */
  errors: string[];
}

/** The prompt plus the output contract, for models without native JSON-schema support. */
export function instructions(request: AiRequest): string {
  let text = `${request.prompt}\n\nAnswer with only a JSON value matching this JSON Schema:\n${JSON.stringify(request.schema)}`;
  if (request.errors.length) text += `\n\nYour previous answer was rejected: ${request.errors[request.errors.length - 1]}`;
  return text;
}

/** A model's answer with what it cost. `cost` is `null` when unknown. */
export interface Completion {
  text: string;
  tokens?: number | null;
  cost?: number | null;
}

export interface Model {
  /** JSON text, or a `Completion` with its usage. */
  complete(request: AiRequest): string | Completion | Promise<string | Completion>;
  /** `null` when the model has no prices, so a `cost` budget can't count it. */
  prices?: [number, number] | null;
}

export function estimateTokens(text: string): number {
  return Math.floor(text.length / 4) + 1;
}
