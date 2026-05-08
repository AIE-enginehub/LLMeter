import { NextResponse } from "next/server";
import { prisma } from "@/lib/db";
import { getAllRouteNames } from "@/lib/providers";

/** 返回已注册的路由名列表 + 实际有日志的服务商列表 */
export async function GET() {
  const [registered, logged] = await Promise.all([
    Promise.resolve(getAllRouteNames()),
    prisma.requestLog
      .groupBy({ by: ["provider"], _count: true })
      .then((rows) => rows.map((r) => r.provider)),
  ]);

  const all = [...new Set([...registered, ...logged])].sort();

  return NextResponse.json({ data: all });
}
