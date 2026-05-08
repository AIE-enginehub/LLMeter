import { NextRequest, NextResponse } from "next/server";
import { prisma } from "@/lib/db";

/** 获取单条日志详情（含完整请求/响应体） */
export async function GET(
  _request: NextRequest,
  { params }: { params: Promise<{ id: string }> }
) {
  const { id } = await params;
  const log = await prisma.requestLog.findUnique({ where: { id } });

  if (!log) {
    return NextResponse.json({ error: "日志不存在" }, { status: 404 });
  }

  return NextResponse.json({ data: log });
}
