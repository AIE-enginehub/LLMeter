import { NextRequest, NextResponse } from "next/server";
import { verifyToken } from "@/lib/auth";

/** 需要页面认证的路径 */
const PROTECTED_PAGE_PATHS = ["/", "/logs", "/stats"];

export async function middleware(request: NextRequest) {
  const { pathname } = request.nextUrl;

  const isProtectedPage = PROTECTED_PAGE_PATHS.includes(pathname);
  const isProtectedApi = pathname.startsWith("/api/query");

  if (!isProtectedPage && !isProtectedApi) return NextResponse.next();

  const token = request.cookies.get("gc_token")?.value;
  if (!token) {
    if (isProtectedApi) {
      return NextResponse.json({ error: "未登录" }, { status: 401 });
    }
    return NextResponse.redirect(new URL("/login", request.url));
  }

  const user = await verifyToken(token);
  if (!user) {
    if (isProtectedApi) {
      return NextResponse.json({ error: "登录已过期" }, { status: 401 });
    }
    return NextResponse.redirect(new URL("/login", request.url));
  }

  return NextResponse.next();
}

export const config = {
  matcher: ["/", "/logs", "/stats", "/api/query/:path*"],
};
