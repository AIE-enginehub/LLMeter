import { ProtocolAdapter, TokenUsage, cleanRequestHeaders } from "./base";

/**
 * Google Gemini 协议适配器
 *
 * 覆盖所有 Gemini 端点（透传方式）：
 * generateContent, streamGenerateContent, embedContent, countTokens,
 * files, models, cachedContents, corpora, tunedModels 等
 *
 * 认证：query param key=xxx 或 OAuth Bearer
 */
export const geminiProtocol: ProtocolAdapter = {
  protocol: "gemini",

  transformRequestHeaders(headers: Headers): Headers {
    return cleanRequestHeaders(headers);
  },

  /** 从 URL 路径中提取模型名称，如 models/gemini-2.5-flash */
  extractModel(_body: unknown, path: string): string | null {
    const match = path.match(/models\/([^/:]+)/);
    return match ? match[1] : null;
  },

  /** 流式通过 alt=sse query param 判断 */
  isStreamRequest(_body: unknown, searchParams: URLSearchParams): boolean {
    return searchParams.get("alt") === "sse";
  },

  /**
   * usageMetadata.promptTokenCount / candidatesTokenCount / cachedContentTokenCount / totalTokenCount
   */
  extractUsage(responseBody: unknown): TokenUsage | null {
    const body = responseBody as Record<string, unknown>;
    const meta = body?.usageMetadata as Record<string, number> | undefined;
    if (!meta) return null;

    return {
      promptTokens: meta.promptTokenCount ?? 0,
      completionTokens: meta.candidatesTokenCount ?? 0,
      cachedTokens: meta.cachedContentTokenCount ?? 0,
      totalTokens: meta.totalTokenCount ?? 0,
    };
  },

  /** SSE: data: {...}\n\n，含 usageMetadata */
  extractStreamUsage(chunk: string): TokenUsage | null {
    let result: TokenUsage | null = null;
    const lines = chunk.split("\n");

    for (const line of lines) {
      if (!line.startsWith("data: ")) continue;
      try {
        const data = JSON.parse(line.slice(6));
        if (data.usageMetadata) {
          const meta = data.usageMetadata;
          result = {
            promptTokens: meta.promptTokenCount ?? 0,
            completionTokens: meta.candidatesTokenCount ?? 0,
            cachedTokens: meta.cachedContentTokenCount ?? 0,
            totalTokens: meta.totalTokenCount ?? 0,
          };
        }
      } catch {
        // 非 JSON 行跳过
      }
    }
    return result;
  },
};
