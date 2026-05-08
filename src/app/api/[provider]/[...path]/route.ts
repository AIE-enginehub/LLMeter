import { NextRequest } from "next/server";
import { proxyHandler } from "@/lib/proxy/handler";

/**
 * 统一代理转发路由
 *
 * /api/{provider}/{...path} → 根据 provider 名称查找配置，转发到对应 base URL
 *
 * 支持所有 HTTP 方法和所有路径，完全透传。
 * provider 名称通过环境变量 {PROVIDER}_BASE_URL 注册。
 */

async function handle(
  request: NextRequest,
  { params }: { params: Promise<{ provider: string; path: string[] }> }
) {
  const { provider, path } = await params;
  return proxyHandler(request, provider, path.join("/"));
}

export const GET = handle;
export const POST = handle;
export const PUT = handle;
export const DELETE = handle;
export const PATCH = handle;

export const runtime = "nodejs";
export const dynamic = "force-dynamic";
