import { NextRequest, NextResponse } from "next/server";
import { prisma } from "@/lib/db";

export async function GET(request: NextRequest) {
  const searchParams = request.nextUrl.searchParams;
  const page = Math.max(1, parseInt(searchParams.get("page") || "1"));
  const pageSize = Math.min(
    100,
    Math.max(1, parseInt(searchParams.get("pageSize") || "20"))
  );
  const provider = searchParams.get("provider");
  const model = searchParams.get("model");
  const status = searchParams.get("status");
  const search = searchParams.get("search");

  // 构建查询条件
  const where: Record<string, unknown> = {};
  if (provider) where.provider = provider;
  if (model) where.model = { contains: model };
  if (status) where.status = status;
  if (search) {
    where.OR = [
      { model: { contains: search, mode: "insensitive" } },
      { path: { contains: search, mode: "insensitive" } },
    ];
  }

  const [logs, total] = await Promise.all([
    prisma.requestLog.findMany({
      where,
      orderBy: { createdAt: "desc" },
      skip: (page - 1) * pageSize,
      take: pageSize,
      select: {
        id: true,
        provider: true,
        model: true,
        path: true,
        method: true,
        isStream: true,
        responseStatus: true,
        promptTokens: true,
        completionTokens: true,
        cachedTokens: true,
        totalTokens: true,
        cost: true,
        status: true,
        errorMessage: true,
        duration: true,
        createdAt: true,
        completedAt: true,
      },
    }),
    prisma.requestLog.count({ where }),
  ]);

  return NextResponse.json({
    data: logs,
    pagination: {
      page,
      pageSize,
      total,
      totalPages: Math.ceil(total / pageSize),
    },
  });
}
