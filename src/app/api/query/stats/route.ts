import { NextRequest, NextResponse } from "next/server";
import { prisma } from "@/lib/db";

export async function GET(request: NextRequest) {
  const searchParams = request.nextUrl.searchParams;
  const days = Math.min(
    365,
    Math.max(1, parseInt(searchParams.get("days") || "7"))
  );
  const since = new Date();
  since.setDate(since.getDate() - days);

  const [overview, byProvider, byModel, dailyStats] = await Promise.all([
    prisma.requestLog.aggregate({
      where: { createdAt: { gte: since } },
      _count: true,
      _sum: {
        promptTokens: true,
        completionTokens: true,
        cachedTokens: true,
        totalTokens: true,
      },
      _avg: { duration: true },
    }),

    prisma.requestLog.groupBy({
      by: ["provider"],
      where: { createdAt: { gte: since } },
      _count: true,
      _sum: {
        promptTokens: true,
        completionTokens: true,
        cachedTokens: true,
        totalTokens: true,
      },
    }),

    prisma.requestLog.groupBy({
      by: ["model"],
      where: { createdAt: { gte: since }, model: { not: null } },
      _count: true,
      _sum: {
        promptTokens: true,
        completionTokens: true,
        cachedTokens: true,
        totalTokens: true,
      },
      orderBy: { _count: { model: "desc" } },
      take: 10,
    }),

    prisma.$queryRaw`
      SELECT
        DATE(created_at) as date,
        COUNT(*)::int as count,
        COALESCE(SUM(prompt_tokens), 0)::int as prompt_tokens,
        COALESCE(SUM(completion_tokens), 0)::int as completion_tokens,
        COALESCE(SUM(cached_tokens), 0)::int as cached_tokens,
        COALESCE(SUM(total_tokens), 0)::int as total_tokens
      FROM request_logs
      WHERE created_at >= ${since}
      GROUP BY DATE(created_at)
      ORDER BY date ASC
    `,
  ]);

  const errorCount = await prisma.requestLog.count({
    where: { createdAt: { gte: since }, status: "error" },
  });

  return NextResponse.json({
    data: {
      overview: {
        totalRequests: overview._count,
        totalPromptTokens: overview._sum.promptTokens ?? 0,
        totalCompletionTokens: overview._sum.completionTokens ?? 0,
        totalCachedTokens: overview._sum.cachedTokens ?? 0,
        totalTokens: overview._sum.totalTokens ?? 0,
        avgDuration: Math.round(overview._avg.duration ?? 0),
        errorRate:
          overview._count > 0
            ? ((errorCount / overview._count) * 100).toFixed(2)
            : "0",
      },
      byProvider,
      byModel,
      dailyStats,
    },
  });
}
