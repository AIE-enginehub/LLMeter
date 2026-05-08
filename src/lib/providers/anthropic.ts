import { ProtocolAdapter, TokenUsage, cleanRequestHeaders } from "./base";

/**
 * Anthropic Claude 协议适配器
 *
 * 覆盖所有 Anthropic 兼容端点（透传方式）：
 * messages, messages/count_tokens, messages/batches, models 等
 *
 * 认证头：x-api-key 或 Authorization Bearer + anthropic-version
 * 兼容厂商：Anthropic, MiniMax 等支持 Anthropic 协议的厂商
 */
export const anthropicProtocol: ProtocolAdapter = {
  protocol: "anthropic",

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
   * usage.input_tokens / output_tokens
   * usage.cache_read_input_tokens / cache_creation_input_tokens
   */
  extractUsage(responseBody: unknown): TokenUsage | null {
    const body = responseBody as Record<string, unknown>;
    if (!body?.usage) return null;
    const usage = body.usage as Record<string, number>;

    const inputTokens = usage.input_tokens ?? 0;
    const outputTokens = usage.output_tokens ?? 0;
    const cacheRead = usage.cache_read_input_tokens ?? 0;
    const cacheCreate = usage.cache_creation_input_tokens ?? 0;

    return {
      promptTokens: inputTokens,
      completionTokens: outputTokens,
      cachedTokens: cacheRead + cacheCreate,
      totalTokens: inputTokens + outputTokens + cacheRead + cacheCreate,
    };
  },

  /**
   * SSE: event: type\ndata: {...}\n\n
   * message_start → input_tokens + cache tokens
   * message_delta → output_tokens
   */
  extractStreamUsage(chunk: string): TokenUsage | null {
    let result: TokenUsage | null = null;
    const lines = chunk.split("\n");

    for (const line of lines) {
      if (!line.startsWith("data: ")) continue;
      try {
        const data = JSON.parse(line.slice(6));

        if (data.type === "message_start" && data.message?.usage) {
          const u = data.message.usage;
          result = {
            promptTokens: u.input_tokens ?? 0,
            completionTokens: u.output_tokens ?? 0,
            cachedTokens: (u.cache_read_input_tokens ?? 0) + (u.cache_creation_input_tokens ?? 0),
            totalTokens: 0,
          };
          result.totalTokens = result.promptTokens + result.completionTokens + result.cachedTokens;
        }

        if (data.type === "message_delta" && data.usage) {
          const outputTokens = data.usage.output_tokens ?? 0;
          if (result) {
            result.completionTokens = outputTokens;
            result.totalTokens = result.promptTokens + result.completionTokens + result.cachedTokens;
          } else {
            result = {
              promptTokens: 0,
              completionTokens: outputTokens,
              cachedTokens: 0,
              totalTokens: outputTokens,
            };
          }
        }
      } catch {
        // 非 JSON 行跳过
      }
    }
    return result;
  },
};
