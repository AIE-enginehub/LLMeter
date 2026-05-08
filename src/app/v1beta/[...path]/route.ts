import { NextRequest } from "next/server";
import { proxyHandler } from "@/lib/proxy/handler";

function handle(request: NextRequest, { params }: { params: Promise<{ path: string[] }> }) {
  return params.then(({ path }) => proxyHandler(request, "v1beta", path.join("/")));
}

export const GET = handle;
export const POST = handle;
export const PUT = handle;
export const PATCH = handle;
export const DELETE = handle;
