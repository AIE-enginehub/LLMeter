import { NextRequest } from "next/server";
import { prisma } from "@/lib/db";
import {
  getProtocolByPrefix,
  resolveModelRoute,
  getDefaultBaseUrl,
  TokenUsage,
} from "@/lib/providers";

/** 请求头脱敏 */
function sanitizeHeaders(headers: Headers): Record<string, string> {
  const result: Record<string, string> = {};
  headers.forEach((value, key) => {
    const lower = key.toLowerCase();
    if (lower === "authorization" || lower === "x-api-key" || lower === "api-key") {
      result[key] = value.length > 20
        ? value.slice(0, 10) + "***" + value.slice(-4)
        : "***";
    } else {
      result[key] = value;
    }
  });
  return result;
}

function isJsonContent(headers: Headers): boolean {
  return (headers.get("content-type") || "").includes("application/json");
}

/**
 * 代理转发处理器
 * @param request 原始请求
 * @param prefix  URL 前缀（v1 / v1beta / anthropic）
 * @param subPath 前缀之后的路径（如 chat/completions）
 */
export async function proxyHandler(
  request: NextRequest,
  prefix: string,
  subPath: string
): Promise<Response> {
  const protocol = getProtocolByPrefix(prefix);
  if (!protocol) {
    return new Response(
      JSON.stringify({ error: `Unknown protocol prefix: ${prefix}` }),
      { status: 404, headers: { "Content-Type": "application/json" } }
    );
  }

  const startTime = Date.now();
  const url = new URL(request.url);
  const query = url.searchParams.toString();

  let requestBodyForLog: unknown = null;
  let rawBodyBuffer: ArrayBuffer | null = null;
  const hasBody = ["POST", "PUT", "PATCH"].includes(request.method);

  if (hasBody) {
    rawBodyBuffer = await request.arrayBuffer();
    if (isJsonContent(request.headers) && rawBodyBuffer.byteLength > 0) {
      try {
        requestBodyForLog = JSON.parse(new TextDecoder().decode(rawBodyBuffer));
      } catch {
        requestBodyForLog = "[non-json body]";
      }
    } else {
      requestBodyForLog = { _type: "binary", _size: rawBodyBuffer.byteLength };
    }
  }

  const model = protocol.extractModel(requestBodyForLog, subPath);
  const isStream = protocol.isStreamRequest(requestBodyForLog, url.searchParams);

  let providerName = "unknown";
  let baseUrl: string | null = null;

  if (model) {
    const route = resolveModelRoute(model);
    if (route) {
      providerName = route.name;
      baseUrl = route.baseUrl;
    }
  }

  if (!baseUrl) {
    baseUrl = getDefaultBaseUrl(prefix);
  }

  if (!baseUrl) {
    return new Response(
      JSON.stringify({
        error: "No route matched",
        hint: model
          ? `模型 "${model}" 未匹配到任何路由，请检查 ROUTE_*_MODELS 配置`
          : `前缀 /${prefix} 无默认路由，请配置 DEFAULT_${prefix.toUpperCase()}_BASE_URL`,
      }),
      { status: 404, headers: { "Content-Type": "application/json" } }
    );
  }

  const fullPath = `${prefix}/${subPath}`;
  const targetUrl = `${baseUrl}/${fullPath}${query ? `?${query}` : ""}`;

  const transformedHeaders = protocol.transformRequestHeaders(request.headers);

  let logId: string | null = null;
  try {
    const log = await prisma.requestLog.create({
      data: {
        provider: providerName,
        model,
        path: fullPath,
        method: request.method,
        isStream,
        requestHeaders: sanitizeHeaders(request.headers),
        requestBody: requestBodyForLog as object,
        status: "pending",
      },
    });
    logId = log.id;
  } catch (e) {
    console.error("Failed to create request log:", e);
  }

  try {
    const fetchOptions: RequestInit = {
      method: request.method,
      headers: transformedHeaders,
    };
    if (rawBodyBuffer !== null && hasBody) {
      fetchOptions.body = rawBodyBuffer;
    }

    const response = await fetch(targetUrl, fetchOptions);

    if (isStream && response.body) {
      return handleStreamResponse(response, protocol, logId, startTime);
    } else {
      return handleNormalResponse(response, protocol, logId, startTime);
    }
  } catch (error) {
    const duration = Date.now() - startTime;
    const errorMessage = error instanceof Error ? error.message : String(error);

    if (logId) {
      prisma.requestLog
        .update({
          where: { id: logId },
          data: { status: "error", errorMessage, duration, completedAt: new Date() },
        })
        .catch(console.error);
    }

    return new Response(
      JSON.stringify({ error: "Proxy request failed", message: errorMessage }),
      { status: 502, headers: { "Content-Type": "application/json" } }
    );
  }
}

/** 处理非流式响应 */
async function handleNormalResponse(
  response: Response,
  protocol: { extractUsage: (body: unknown) => TokenUsage | null },
  logId: string | null,
  startTime: number
): Promise<Response> {
  const responseBuffer = await response.arrayBuffer();
  const duration = Date.now() - startTime;

  let responseBodyForLog: unknown = null;
  const contentType = response.headers.get("content-type") || "";
  if (contentType.includes("application/json") || contentType.includes("text/")) {
    const text = new TextDecoder().decode(responseBuffer);
    try {
      responseBodyForLog = JSON.parse(text);
    } catch {
      responseBodyForLog = text.length > 10000 ? text.slice(0, 10000) + "...[truncated]" : text;
    }
  } else {
    responseBodyForLog = { _type: "binary", _size: responseBuffer.byteLength };
  }

  const usage = protocol.extractUsage(responseBodyForLog);

  if (logId) {
    prisma.requestLog
      .update({
        where: { id: logId },
        data: {
          responseStatus: response.status,
          responseBody: responseBodyForLog as object,
          promptTokens: usage?.promptTokens,
          completionTokens: usage?.completionTokens,
          cachedTokens: usage?.cachedTokens,
          totalTokens: usage?.totalTokens,
          status: response.ok ? "success" : "error",
          errorMessage: response.ok ? null : String(responseBodyForLog).slice(0, 2000),
          duration,
          completedAt: new Date(),
        },
      })
      .catch(console.error);
  }

  return new Response(responseBuffer, {
    status: response.status,
    headers: passHeaders(response.headers),
  });
}

/** 处理流式响应 */
function handleStreamResponse(
  response: Response,
  protocol: { extractStreamUsage: (chunk: string) => TokenUsage | null },
  logId: string | null,
  startTime: number
): Response {
  let usage: TokenUsage | null = null;
  let fullResponse = "";

  if (logId) {
    prisma.requestLog
      .update({
        where: { id: logId },
        data: { status: "streaming", responseStatus: response.status },
      })
      .catch(console.error);
  }

  const decoder = new TextDecoder();

  const transformStream = new TransformStream({
    transform(chunk, controller) {
      controller.enqueue(chunk);

      const text = decoder.decode(chunk, { stream: true });
      fullResponse += text;

      const chunkUsage = protocol.extractStreamUsage(text);
      if (chunkUsage) {
        if (!usage) {
          usage = { ...chunkUsage };
        } else {
          if (chunkUsage.promptTokens > 0) usage.promptTokens = chunkUsage.promptTokens;
          if (chunkUsage.completionTokens > 0) usage.completionTokens = chunkUsage.completionTokens;
          if (chunkUsage.cachedTokens > 0) usage.cachedTokens = chunkUsage.cachedTokens;
          usage.totalTokens = usage.promptTokens + usage.completionTokens + usage.cachedTokens;
        }
      }
    },
    flush() {
      const duration = Date.now() - startTime;
      if (logId) {
        prisma.requestLog
          .update({
            where: { id: logId },
            data: {
              responseBody: fullResponse as unknown as object,
              promptTokens: usage?.promptTokens,
              completionTokens: usage?.completionTokens,
              cachedTokens: usage?.cachedTokens,
              totalTokens: usage?.totalTokens,
              status: "success",
              duration,
              completedAt: new Date(),
            },
          })
          .catch(console.error);
      }
    },
  });

  return new Response(response.body!.pipeThrough(transformStream), {
    status: response.status,
    headers: passHeaders(response.headers),
  });
}

/** 透传响应头 */
function passHeaders(responseHeaders: Headers): Headers {
  const headers = new Headers();
  responseHeaders.forEach((value, key) => {
    const lower = key.toLowerCase();
    if (lower !== "transfer-encoding" && lower !== "content-encoding") {
      headers.set(key, value);
    }
  });
  return headers;
}
