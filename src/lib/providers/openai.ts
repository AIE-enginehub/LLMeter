import { ProtocolAdapter, TokenUsage, cleanRequestHeaders } from "./base";

/**
 * OpenAI 协议适配器
 *
 * 覆盖所有 OpenAI 兼容端点（透传方式）：
 * chat/completions, responses, embeddings, files, models,
 * images, audio, moderations, assistants, threads, vector_stores, batches 等
 *
 * 兼容厂商：OpenAI, DeepSeek, 及所有 OpenAI 兼容 API
 */
export const openaiProtocol: ProtocolAdapter = {
  protocol: "openai",

  transformRequestHeaders(headers: Headers): Headers {
    return cleanRequestHeaders(headers);
  },

  extractModel(body: unknown): string | null {
    if (body && typeof body === "object" && "model" in body) {
      return (body as Record<string, unknown>).model as string;
    }
    return null;
  },

  isStreamRequest(body: unknown): boolean {
    if (body && typeof body === "object" && "stream" in body) {
      return (body as Record<string, unknown>).stream === true;
    }
    return false;
  },

  /**
   * ChatCompletion: usage.prompt_tokens / completion_tokens, prompt_tokens_details.cached_tokens
   * Responses API: usage.input_tokens / output_tokens, input_tokens_details.cached_tokens
   */
  extractUsage(responseBody: unknown): TokenUsage | null {
    const body = responseBody as Record<string, unknown>;
    if (!body?.usage) return null;
    const usage = body.usage as Record<string, unknown>;

    if ("prompt_tokens" in usage) {
      const details = usage.prompt_tokens_details as Record<string, number> | undefined;
      return {
        promptTokens: (usage.prompt_tokens as number) ?? 0,
        completionTokens: (usage.completion_tokens as number) ?? 0,
        cachedTokens: details?.cached_tokens ?? 0,
        totalTokens: (usage.total_tokens as number) ?? 0,
      };
    }

    if ("input_tokens" in usage) {
      const details = usage.input_tokens_details as Record<string, number> | undefined;
      return {
        promptTokens: (usage.input_tokens as number) ?? 0,
        completionTokens: (usage.output_tokens as number) ?? 0,
        cachedTokens: details?.cached_tokens ?? 0,
        totalTokens: (usage.total_tokens as number) ?? 0,
      };
    }

    return null;
  },

  /**
   * ChatCompletion stream: data: {...}\n\n + data: [DONE]
   * Responses stream: event: response.completed\ndata: {...}
   */
  extractStreamUsage(chunk: string): TokenUsage | null {
    let result: TokenUsage | null = null;
    const lines = chunk.split("\n");

    for (const line of lines) {
      if (!line.startsWith("data: ") || line.includes("[DONE]")) continue;
      try {
        const data = JSON.parse(line.slice(6));

        if (data.usage) {
          const usage = data.usage;
          if ("prompt_tokens" in usage) {
            const details = usage.prompt_tokens_details as Record<string, number> | undefined;
            result = {
              promptTokens: usage.prompt_tokens ?? 0,
              completionTokens: usage.completion_tokens ?? 0,
              cachedTokens: details?.cached_tokens ?? 0,
              totalTokens: usage.total_tokens ?? 0,
            };
          } else if ("input_tokens" in usage) {
            const details = usage.input_tokens_details as Record<string, number> | undefined;
            result = {
              promptTokens: usage.input_tokens ?? 0,
              completionTokens: usage.output_tokens ?? 0,
              cachedTokens: details?.cached_tokens ?? 0,
              totalTokens: usage.total_tokens ?? 0,
            };
          }
        }

        if (data.type === "response.completed" && data.response?.usage) {
          const usage = data.response.usage;
          const details = usage.input_tokens_details as Record<string, number> | undefined;
          result = {
            promptTokens: usage.input_tokens ?? 0,
            completionTokens: usage.output_tokens ?? 0,
            cachedTokens: details?.cached_tokens ?? 0,
            totalTokens: usage.total_tokens ?? 0,
          };
        }
      } catch {
        // 非 JSON 行跳过
      }
    }
    return result;
  },
};
