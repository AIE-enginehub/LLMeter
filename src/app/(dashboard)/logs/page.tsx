"use client";

import { useEffect, useState, useCallback } from "react";
import { Card, CardContent } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Table, TableBody, TableCell, TableHead, TableHeader, TableRow,
} from "@/components/ui/table";
import {
  Select, SelectContent, SelectItem, SelectTrigger, SelectValue,
} from "@/components/ui/select";
import {
  Dialog, DialogContent, DialogHeader, DialogTitle,
} from "@/components/ui/dialog";

interface LogItem {
  id: string;
  provider: string;
  model: string | null;
  path: string;
  method: string;
  isStream: boolean;
  responseStatus: number | null;
  promptTokens: number | null;
  completionTokens: number | null;
  cachedTokens: number | null;
  totalTokens: number | null;
  status: string;
  errorMessage: string | null;
  duration: number | null;
  createdAt: string;
}

interface Pagination {
  page: number;
  pageSize: number;
  total: number;
  totalPages: number;
}

function formatBody(body: unknown): string {
  if (!body) return "无";
  if (typeof body === "string") return body;
  try {
    return JSON.stringify(body, null, 2);
  } catch {
    return String(body);
  }
}

function statusConfig(status: string): {
  variant: "default" | "secondary" | "destructive" | "outline";
  dot: string;
} {
  const map: Record<string, { variant: "default" | "secondary" | "destructive" | "outline"; dot: string }> = {
    success: { variant: "secondary", dot: "bg-emerald-500" },
    error: { variant: "destructive", dot: "bg-red-500" },
    streaming: { variant: "outline", dot: "bg-blue-500 animate-pulse" },
    pending: { variant: "outline", dot: "bg-amber-400 animate-pulse" },
  };
  return map[status] || { variant: "outline", dot: "bg-slate-400" };
}

export default function LogsPage() {
  const [logs, setLogs] = useState<LogItem[]>([]);
  const [pagination, setPagination] = useState<Pagination | null>(null);
  const [loading, setLoading] = useState(true);
  const [selectedLog, setSelectedLog] = useState<Record<string, unknown> | null>(null);
  const [detailLoading, setDetailLoading] = useState(false);

  const [providers, setProviders] = useState<string[]>([]);
  const [provider, setProvider] = useState("__all__");
  const [status, setStatus] = useState("__all__");
  const [page, setPage] = useState(1);

  useEffect(() => {
    fetch("/api/query/providers")
      .then((res) => res.json())
      .then((json) => setProviders(json.data || []))
      .catch(console.error);
  }, []);

  const fetchLogs = useCallback(async () => {
    setLoading(true);
    const params = new URLSearchParams();
    params.set("page", page.toString());
    params.set("pageSize", "20");
    if (provider && provider !== "__all__") params.set("provider", provider);
    if (status && status !== "__all__") params.set("status", status);

    try {
      const res = await fetch(`/api/query/logs?${params}`);
      const json = await res.json();
      setLogs(json.data);
      setPagination(json.pagination);
    } catch (e) {
      console.error(e);
    } finally {
      setLoading(false);
    }
  }, [page, provider, status]);

  useEffect(() => {
    fetchLogs();
  }, [fetchLogs]);

  async function viewDetail(id: string) {
    setDetailLoading(true);
    setSelectedLog({});
    try {
      const res = await fetch(`/api/query/logs/${id}`);
      const json = await res.json();
      setSelectedLog(json.data);
    } catch (e) {
      console.error(e);
    } finally {
      setDetailLoading(false);
    }
  }

  return (
    <div className="space-y-4">
      {/* 页头 */}
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-lg font-semibold text-slate-900">调用日志</h2>
          <p className="text-xs text-muted-foreground mt-0.5">查看所有 API 请求的详细记录</p>
        </div>
        <Button
          variant="outline"
          size="xs"
          onClick={fetchLogs}
          className="text-slate-500 hover:text-slate-700"
        >
          <svg className="h-3 w-3 mr-1" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <path d="M23 4v6h-6M1 20v-6h6" />
            <path d="M3.51 9a9 9 0 0114.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0020.49 15" />
          </svg>
          刷新
        </Button>
      </div>

      {/* 筛选栏 */}
      <div className="flex gap-3">
        <Select
          value={provider}
          onValueChange={(v) => { setProvider(v ?? "__all__"); setPage(1); }}
        >
          <SelectTrigger className="w-[160px] h-8 text-xs bg-white">
            <SelectValue placeholder="全部服务商" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="__all__">全部服务商</SelectItem>
            {providers.map((p) => (
              <SelectItem key={p} value={p}>{p}</SelectItem>
            ))}
          </SelectContent>
        </Select>

        <Select
          value={status}
          onValueChange={(v) => { setStatus(v ?? "__all__"); setPage(1); }}
        >
          <SelectTrigger className="w-[140px] h-8 text-xs bg-white">
            <SelectValue placeholder="全部状态" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="__all__">全部状态</SelectItem>
            <SelectItem value="success">成功</SelectItem>
            <SelectItem value="error">失败</SelectItem>
            <SelectItem value="streaming">流式中</SelectItem>
            <SelectItem value="pending">等待中</SelectItem>
          </SelectContent>
        </Select>
      </div>

      {/* 日志表格 */}
      <Card className="shadow-sm ring-0 border border-slate-200/60">
        <CardContent className="p-0">
          {loading ? (
            <div className="flex items-center justify-center h-48 text-sm text-muted-foreground">
              <svg className="h-4 w-4 animate-spin mr-2 text-slate-400" viewBox="0 0 24 24" fill="none">
                <circle cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="3" strokeDasharray="32" strokeLinecap="round" />
              </svg>
              加载中...
            </div>
          ) : logs.length === 0 ? (
            <div className="flex flex-col items-center justify-center h-48 text-sm text-muted-foreground">
              <svg className="h-8 w-8 text-slate-300 mb-2" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
                <path d="M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8z" />
                <polyline points="14 2 14 8 20 8" />
              </svg>
              暂无日志数据
            </div>
          ) : (
            <Table>
              <TableHeader>
                <TableRow className="hover:bg-transparent bg-slate-50/50">
                  <TableHead className="text-slate-500 text-xs">时间</TableHead>
                  <TableHead className="text-slate-500 text-xs">服务商</TableHead>
                  <TableHead className="text-slate-500 text-xs">模型</TableHead>
                  <TableHead className="text-center text-slate-500 text-xs">流式</TableHead>
                  <TableHead className="text-center text-slate-500 text-xs">状态</TableHead>
                  <TableHead className="text-right text-slate-500 text-xs">输入/输出/缓存</TableHead>
                  <TableHead className="text-right text-slate-500 text-xs">耗时</TableHead>
                  <TableHead className="text-center text-slate-500 text-xs">操作</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {logs.map((log, i) => {
                  const sc = statusConfig(log.status);
                  return (
                    <TableRow
                      key={log.id}
                      className="hover:bg-blue-50/40 transition-colors duration-150 cursor-pointer animate-in-up"
                      style={{ animationDelay: `${i * 20}ms` }}
                      onClick={() => viewDetail(log.id)}
                    >
                      <TableCell className="text-muted-foreground text-xs whitespace-nowrap">
                        {new Date(log.createdAt).toLocaleString("zh-CN")}
                      </TableCell>
                      <TableCell className="font-medium capitalize text-slate-700 text-xs">{log.provider}</TableCell>
                      <TableCell className="font-mono text-xs max-w-[180px] truncate text-slate-600">
                        {log.model || "-"}
                      </TableCell>
                      <TableCell className="text-center">
                        {log.isStream ? (
                          <Badge variant="outline" className="text-[10px] bg-blue-50 text-blue-600 border-blue-200/60">SSE</Badge>
                        ) : (
                          <span className="text-muted-foreground text-xs">-</span>
                        )}
                      </TableCell>
                      <TableCell className="text-center">
                        <Badge variant={sc.variant} className="text-[10px] gap-1">
                          <span className={`inline-block h-1.5 w-1.5 rounded-full ${sc.dot}`} />
                          {log.status}
                        </Badge>
                      </TableCell>
                      <TableCell className="text-right text-xs font-mono text-slate-500">
                        {log.promptTokens != null || log.completionTokens != null
                          ? `${log.promptTokens ?? 0} / ${log.completionTokens ?? 0} / ${log.cachedTokens ?? 0}`
                          : "-"}
                      </TableCell>
                      <TableCell className="text-right text-muted-foreground text-xs font-mono">
                        {log.duration ? `${log.duration}ms` : "-"}
                      </TableCell>
                      <TableCell className="text-center">
                        <Button
                          variant="ghost"
                          size="xs"
                          className="text-blue-600 hover:text-blue-700 hover:bg-blue-50"
                          onClick={(e) => { e.stopPropagation(); viewDetail(log.id); }}
                        >
                          详情
                        </Button>
                      </TableCell>
                    </TableRow>
                  );
                })}
              </TableBody>
            </Table>
          )}

          {/* 分页 */}
          {pagination && pagination.totalPages > 1 && (
            <div className="flex items-center justify-between px-4 py-3 border-t border-slate-100">
              <span className="text-xs text-slate-400">
                共 {pagination.total} 条记录
              </span>
              <div className="flex items-center gap-1">
                <Button
                  variant="outline"
                  size="xs"
                  onClick={() => setPage(Math.max(1, page - 1))}
                  disabled={page === 1}
                  className="text-xs"
                >
                  上一页
                </Button>
                <span className="px-3 text-xs text-slate-500 font-mono">
                  {page} / {pagination.totalPages}
                </span>
                <Button
                  variant="outline"
                  size="xs"
                  onClick={() => setPage(Math.min(pagination.totalPages, page + 1))}
                  disabled={page === pagination.totalPages}
                  className="text-xs"
                >
                  下一页
                </Button>
              </div>
            </div>
          )}
        </CardContent>
      </Card>

      {/* 详情弹窗 */}
      <Dialog
        open={selectedLog !== null}
        onOpenChange={(open) => { if (!open) setSelectedLog(null); }}
      >
        <DialogContent className="sm:max-w-2xl max-h-[80vh] overflow-y-auto">
          <DialogHeader>
            <DialogTitle className="text-slate-800">请求详情</DialogTitle>
          </DialogHeader>

          {detailLoading ? (
            <div className="flex items-center justify-center text-muted-foreground py-12 text-sm">
              <svg className="h-4 w-4 animate-spin mr-2 text-slate-400" viewBox="0 0 24 24" fill="none">
                <circle cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="3" strokeDasharray="32" strokeLinecap="round" />
              </svg>
              加载中...
            </div>
          ) : selectedLog && Object.keys(selectedLog).length > 0 ? (
            <div className="space-y-5 animate-in-fade">
              {/* 基本信息 */}
              <div className="grid grid-cols-2 sm:grid-cols-3 gap-2">
                {([
                    ["服务商", String(selectedLog.provider || "-")],
                    ["模型", String(selectedLog.model || "-")],
                    ["状态", String(selectedLog.status || "-")],
                    ["耗时", selectedLog.duration ? `${String(selectedLog.duration)}ms` : "-"],
                    ["输入 Token", String(selectedLog.promptTokens ?? "-")],
                    ["输出 Token", String(selectedLog.completionTokens ?? "-")],
                    ["缓存 Token", String(selectedLog.cachedTokens ?? "-")],
                    ["总 Token", String(selectedLog.totalTokens ?? "-")],
                    ["流式", selectedLog.isStream ? "是" : "否"],
                  ] satisfies [string, string][]).map(([label, value]) => (
                  <div key={label} className="bg-slate-50 rounded-lg p-2.5 border border-slate-100">
                    <p className="text-[11px] text-slate-400 font-medium">{label}</p>
                    <p className="text-sm font-medium mt-0.5 text-slate-700 font-mono">{value}</p>
                  </div>
                ))}
              </div>

              {/* 请求体 */}
              <div>
                <p className="text-xs font-medium text-slate-500 mb-1.5 flex items-center gap-1">
                  <svg className="h-3 w-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                    <path d="M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8z" />
                    <polyline points="14 2 14 8 20 8" />
                  </svg>
                  请求体
                </p>
                <pre className="bg-slate-900 text-slate-200 rounded-lg p-3 text-xs overflow-auto max-h-48 font-mono leading-relaxed">
                  {formatBody(selectedLog.requestBody)}
                </pre>
              </div>

              {/* 响应体 */}
              <div>
                <p className="text-xs font-medium text-slate-500 mb-1.5 flex items-center gap-1">
                  <svg className="h-3 w-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                    <path d="M21 15a2 2 0 01-2 2H7l-4 4V5a2 2 0 012-2h14a2 2 0 012 2z" />
                  </svg>
                  响应体
                </p>
                <pre className="bg-slate-900 text-slate-200 rounded-lg p-3 text-xs overflow-auto max-h-48 font-mono leading-relaxed">
                  {formatBody(selectedLog.responseBody)}
                </pre>
              </div>

              {/* 错误信息 */}
              {selectedLog.errorMessage ? (
                <div>
                  <p className="text-xs font-medium text-red-500 mb-1.5">错误信息</p>
                  <pre className="bg-red-50 border border-red-100 rounded-lg p-3 text-xs overflow-auto text-red-600 font-mono">
                    {String(selectedLog.errorMessage)}
                  </pre>
                </div>
              ) : null}
            </div>
          ) : null}
        </DialogContent>
      </Dialog>
    </div>
  );
}
