import { ProtocolAdapter } from "./base";
import { openaiProtocol } from "./openai";
import { anthropicProtocol } from "./anthropic";
import { geminiProtocol } from "./gemini";

/** 三种协议适配器 */
const protocols: Record<string, ProtocolAdapter> = {
  openai: openaiProtocol,
  anthropic: anthropicProtocol,
  gemini: geminiProtocol,
};

/** URL 路径前缀 → 协议映射 */
const PATH_PROTOCOL_MAP: Record<string, string> = {
  v1: "openai",
  v1beta: "gemini",
  anthropic: "anthropic",
};

/** 模型路由配置 */
interface ModelRoute {
  name: string;
  patterns: string[];
  baseUrl: string;
}

/**
 * 从环境变量加载模型路由
 * ROUTE_{NAME}_MODELS="gpt-*,o1-*"
 * ROUTE_{NAME}_BASE_URL="https://api.openai.com"
 */
function loadModelRoutes(): ModelRoute[] {
  const routes: ModelRoute[] = [];
  const seen = new Set<string>();

  for (const key of Object.keys(process.env)) {
    const match = key.match(/^ROUTE_(.+)_MODELS$/);
    if (!match) continue;
    const name = match[1].toLowerCase();
    if (seen.has(name)) continue;
    seen.add(name);

    const models = process.env[key];
    const baseUrl = process.env[`ROUTE_${match[1]}_BASE_URL`];
    if (!models || !baseUrl) continue;

    routes.push({
      name,
      patterns: models.split(",").map((p) => p.trim()).filter(Boolean),
      baseUrl: baseUrl.replace(/\/+$/, ""),
    });
  }

  return routes;
}

/** glob 风格匹配（支持 * 通配符） */
function matchPattern(model: string, pattern: string): boolean {
  const regex = new RegExp(
    "^" + pattern.replace(/[.+^${}()|[\]\\]/g, "\\$&").replace(/\*/g, ".*") + "$"
  );
  return regex.test(model);
}

let modelRoutes: ModelRoute[] | null = null;

function getRoutes(): ModelRoute[] {
  if (!modelRoutes) {
    modelRoutes = loadModelRoutes();
  }
  return modelRoutes;
}

/** 根据模型名匹配路由，返回 { name, baseUrl } */
export function resolveModelRoute(model: string): { name: string; baseUrl: string } | null {
  for (const route of getRoutes()) {
    for (const pattern of route.patterns) {
      if (matchPattern(model, pattern)) {
        return { name: route.name, baseUrl: route.baseUrl };
      }
    }
  }
  return null;
}

/** 根据 URL 前缀获取协议适配器 */
export function getProtocolByPrefix(prefix: string): ProtocolAdapter | null {
  const protocolName = PATH_PROTOCOL_MAP[prefix];
  return protocolName ? protocols[protocolName] || null : null;
}

/**
 * 获取指定前缀的默认 base URL（用于无 model 的请求如 GET /v1/models）
 * 优先读取 DEFAULT_{PREFIX}_BASE_URL 环境变量，否则取该前缀下第一条路由
 */
export function getDefaultBaseUrl(prefix: string): string | null {
  const envKey = `DEFAULT_${prefix.toUpperCase()}_BASE_URL`;
  const envVal = process.env[envKey];
  if (envVal) return envVal.replace(/\/+$/, "");

  const protocolName = PATH_PROTOCOL_MAP[prefix];
  if (!protocolName) return null;

  for (const route of getRoutes()) {
    try {
      const url = new URL(route.baseUrl);
      const path = url.pathname;
      if (prefix === "v1" && (path.endsWith("/v1") || path.includes("/v1/"))) return route.baseUrl;
      if (prefix === "v1beta" && path.includes("/v1beta")) return route.baseUrl;
      if (prefix === "anthropic" && path.includes("/anthropic")) return route.baseUrl;
    } catch {
      continue;
    }
  }

  return null;
}

/** 获取所有已注册的路由名列表 */
export function getAllRouteNames(): string[] {
  return getRoutes().map((r) => r.name);
}

export { type ProtocolAdapter, type TokenUsage } from "./base";
