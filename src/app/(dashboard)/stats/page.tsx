"use client";

import { useEffect, useState } from "react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Table, TableBody, TableCell, TableHead, TableHeader, TableRow,
} from "@/components/ui/table";

interface DailyStat {
  date: string;
  count: number;
  prompt_tokens: number;
  completion_tokens: number;
  total_tokens: number;
}

interface StatsData {
  overview: {
    totalRequests: number;
    totalPromptTokens: number;
    totalCompletionTokens: number;
    totalTokens: number;
    avgDuration: number;
    errorRate: string;
  };
  byProvider: Array<{
    provider: string;
    _count: number;
    _sum: {
      promptTokens: number | null;
      completionTokens: number | null;
      totalTokens: number | null;
    };
  }>;
  byModel: Array<{
    model: string | null;
    _count: number;
    _sum: { totalTokens: number | null };
  }>;
  dailyStats: DailyStat[];
}

function fmt(n: number): string {
  if (n >= 1_000_000) return (n / 1_000_000).toFixed(1) + "M";
  if (n >= 1_000) return (n / 1_000).toFixed(1) + "K";
  return n.toString();
}

/** 排名奖牌样式 */
const RANK_STYLES = [
  "bg-gradient-to-r from-amber-400 to-amber-500 text-white border-0",
  "bg-gradient-to-r from-slate-300 to-slate-400 text-white border-0",
  "bg-gradient-to-r from-orange-300 to-orange-400 text-white border-0",
];

export default function StatsPage() {
  const [stats, setStats] = useState<StatsData | null>(null);
  const [days, setDays] = useState(30);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    setLoading(true);
    fetch(`/api/query/stats?days=${days}`)
      .then((res) => res.json())
      .then((json) => setStats(json.data))
      .catch(console.error)
      .finally(() => setLoading(false));
  }, [days]);

  if (loading) {
    return (
      <div className="flex items-center justify-center h-64 text-muted-foreground text-sm">
        <svg className="h-5 w-5 animate-spin mr-2 text-slate-400" viewBox="0 0 24 24" fill="none">
          <circle cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="3" strokeDasharray="32" strokeLinecap="round" />
        </svg>
        加载中...
      </div>
    );
  }

  if (!stats) return null;

  const maxDailyTokens = Math.max(...stats.dailyStats.map((d) => d.total_tokens), 1);
  const maxDailyCount = Math.max(...stats.dailyStats.map((d) => d.count), 1);

  return (
    <div className="space-y-6">
      {/* 页头 */}
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-lg font-semibold text-slate-900">用量统计</h2>
          <p className="text-xs text-muted-foreground mt-0.5">查看 API 调用量和 Token 消耗趋势</p>
        </div>
        <div className="flex gap-1 bg-slate-100/80 rounded-lg p-0.5">
          {[7, 14, 30, 90].map((d) => (
            <Button
              key={d}
              variant={days === d ? "default" : "ghost"}
              size="xs"
              onClick={() => setDays(d)}
              className={days === d ? "shadow-sm" : "text-slate-500"}
            >
              {d}天
            </Button>
          ))}
        </div>
      </div>

      {/* 每日请求数 */}
      <Card className="shadow-sm ring-0 border border-slate-200/60">
        <CardHeader>
          <CardTitle className="text-slate-800">每日请求数</CardTitle>
        </CardHeader>
        <CardContent>
          {stats.dailyStats.length === 0 ? (
            <p className="text-sm text-muted-foreground py-4 text-center">暂无数据</p>
          ) : (
            <div className="flex items-end gap-[3px] h-44 overflow-x-auto pb-6 relative">
              {stats.dailyStats.map((d, i) => {
                const heightPct = (d.count / maxDailyCount) * 100;
                return (
                  <div
                    key={d.date}
                    className="flex flex-col items-center flex-shrink-0 group"
                    style={{ minWidth: "28px" }}
                  >
                    <div className="relative w-full flex items-end justify-center h-32">
                      <div
                        className="w-5 bg-gradient-to-t from-blue-400 to-blue-300 rounded-t-sm transition-all duration-200 group-hover:from-blue-500 group-hover:to-blue-400 animate-bar"
                        style={{
                          height: `${Math.max(heightPct, 4)}%`,
                          animationDelay: `${i * 30}ms`,
                        }}
                      />
                      <div className="absolute -top-7 bg-slate-800 text-white text-[10px] px-2 py-1 rounded-md shadow-lg opacity-0 group-hover:opacity-100 transition-all duration-200 group-hover:-translate-y-0.5 whitespace-nowrap pointer-events-none">
                        {d.count} 次
                        <div className="absolute top-full left-1/2 -translate-x-1/2 w-0 h-0 border-l-[4px] border-r-[4px] border-t-[4px] border-transparent border-t-slate-800" />
                      </div>
                    </div>
                    <span className="text-[10px] text-slate-400 mt-2 -rotate-45 origin-top-left whitespace-nowrap">
                      {new Date(d.date).toLocaleDateString("zh-CN", { month: "numeric", day: "numeric" })}
                    </span>
                  </div>
                );
              })}
            </div>
          )}
        </CardContent>
      </Card>

      {/* 每日 Token 消耗 */}
      <Card className="shadow-sm ring-0 border border-slate-200/60">
        <CardHeader className="flex flex-row items-center justify-between">
          <CardTitle className="text-slate-800">每日 Token 消耗</CardTitle>
          <div className="flex gap-4 text-xs text-slate-500">
            <span className="flex items-center gap-1.5">
              <span className="w-2.5 h-2.5 bg-gradient-to-r from-emerald-300 to-emerald-400 rounded-sm" /> 输入
            </span>
            <span className="flex items-center gap-1.5">
              <span className="w-2.5 h-2.5 bg-gradient-to-r from-violet-300 to-violet-400 rounded-sm" /> 输出
            </span>
          </div>
        </CardHeader>
        <CardContent>
          {stats.dailyStats.length === 0 ? (
            <p className="text-sm text-muted-foreground py-4 text-center">暂无数据</p>
          ) : (
            <div className="flex items-end gap-[3px] h-44 overflow-x-auto pb-6 relative">
              {stats.dailyStats.map((d, i) => {
                const promptPct = (d.prompt_tokens / maxDailyTokens) * 100;
                const completionPct = (d.completion_tokens / maxDailyTokens) * 100;
                return (
                  <div
                    key={d.date}
                    className="flex flex-col items-center flex-shrink-0 group"
                    style={{ minWidth: "28px" }}
                  >
                    <div className="relative w-full flex items-end justify-center h-32">
                      <div className="flex flex-col w-5 animate-bar" style={{ animationDelay: `${i * 30}ms` }}>
                        <div
                          className="w-full bg-gradient-to-t from-violet-400 to-violet-300 rounded-t-sm transition-colors group-hover:from-violet-500 group-hover:to-violet-400"
                          style={{ height: `${Math.max(completionPct, 0)}px`, maxHeight: "128px" }}
                        />
                        <div
                          className="w-full bg-gradient-to-t from-emerald-400 to-emerald-300 transition-colors group-hover:from-emerald-500 group-hover:to-emerald-400"
                          style={{ height: `${Math.max(promptPct, 0)}px`, maxHeight: "128px" }}
                        />
                      </div>
                      <div className="absolute -top-7 bg-slate-800 text-white text-[10px] px-2 py-1 rounded-md shadow-lg opacity-0 group-hover:opacity-100 transition-all duration-200 group-hover:-translate-y-0.5 whitespace-nowrap pointer-events-none">
                        {fmt(d.total_tokens)} tokens
                        <div className="absolute top-full left-1/2 -translate-x-1/2 w-0 h-0 border-l-[4px] border-r-[4px] border-t-[4px] border-transparent border-t-slate-800" />
                      </div>
                    </div>
                    <span className="text-[10px] text-slate-400 mt-2 -rotate-45 origin-top-left whitespace-nowrap">
                      {new Date(d.date).toLocaleDateString("zh-CN", { month: "numeric", day: "numeric" })}
                    </span>
                  </div>
                );
              })}
            </div>
          )}
        </CardContent>
      </Card>

      {/* 模型使用排行 */}
      <Card className="shadow-sm ring-0 border border-slate-200/60">
        <CardHeader>
          <CardTitle className="text-slate-800">模型使用排行</CardTitle>
        </CardHeader>
        <CardContent>
          {stats.byModel.length === 0 ? (
            <p className="text-sm text-muted-foreground py-4 text-center">暂无数据</p>
          ) : (
            <Table>
              <TableHeader>
                <TableRow className="hover:bg-transparent bg-slate-50/50">
                  <TableHead className="w-12 text-slate-500">排名</TableHead>
                  <TableHead className="text-slate-500">模型</TableHead>
                  <TableHead className="text-right text-slate-500">调用次数</TableHead>
                  <TableHead className="text-right text-slate-500">总 Token</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {stats.byModel.map((m, i) => (
                  <TableRow
                    key={m.model || i}
                    className="hover:bg-slate-50/60 transition-colors animate-in-up"
                    style={{ animationDelay: `${i * 40}ms` }}
                  >
                    <TableCell>
                      <Badge
                        variant={i < 3 ? "default" : "outline"}
                        className={`font-mono text-[10px] ${i < 3 ? RANK_STYLES[i] : ""}`}
                      >
                        {i + 1}
                      </Badge>
                    </TableCell>
                    <TableCell className="font-mono text-sm font-medium text-slate-700">{m.model || "unknown"}</TableCell>
                    <TableCell className="text-right font-mono text-slate-600">{m._count}</TableCell>
                    <TableCell className="text-right font-mono font-medium text-slate-800">{fmt(m._sum.totalTokens ?? 0)}</TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
