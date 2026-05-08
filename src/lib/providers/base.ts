/** Token 用量（统一格式，覆盖三种协议的字段差异） */
export interface TokenUsage {
  promptTokens: number;
  completionTokens: number;
  cachedTokens: number;
  totalTokens: number;
}

/** 协议适配器接口（纯协议层，不绑定具体服务商） */
export interface ProtocolAdapter {
  /** 协议标识 */
  protocol: string;

  /** 处理请求头 */
  transformRequestHeaders(headers: Headers): Headers;

  /** 从请求体或路径中提取模型名称 */
  extractModel(body: unknown, path: string): string | null;

  /** 判断是否流式请求 */
  isStreamRequest(body: unknown, searchParams: URLSearchParams): boolean;

  /** 从非流式响应中提取 Token 用量 */
  extractUsage(responseBody: unknown): TokenUsage | null;

  /** 从流式 chunk 文本中提取 Token 用量 */
  extractStreamUsage(chunk: string): TokenUsage | null;
}

/**
 * 服务商配置
 * 将服务商名称、协议类型、base URL 关联起来
 */
export interface ProviderConfig {
  /** 服务商名称（如 openai, deepseek, minimax） */
  name: string;
  /** 协议类型（openai / anthropic / gemini） */
  protocol: string;
  /** 转发目标 base URL */
  baseUrl: string;
}

/** 不应转发的请求头 */
const HOP_BY_HOP_HEADERS = new Set([
  "host",
  "connection",
  "keep-alive",
  "transfer-encoding",
  "te",
  "trailer",
  "upgrade",
  "proxy-authorization",
  "proxy-authenticate",
  "content-length",
  "expect",
]);

/** 基础请求头清理：去除 hop-by-hop 头 */
export function cleanRequestHeaders(headers: Headers): Headers {
  const cleaned = new Headers();
  headers.forEach((value, key) => {
    if (!HOP_BY_HOP_HEADERS.has(key.toLowerCase())) {
      cleaned.set(key, value);
    }
  });
  return cleaned;
}
