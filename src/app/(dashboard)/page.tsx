"use client";

import { useEffect, useState } from "react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Table, TableBody, TableCell, TableHead, TableHeader, TableRow,
} from "@/components/ui/table";

interface OverviewData {
  totalRequests: number;
  totalPromptTokens: number;
  totalCompletionTokens: number;
  totalCachedTokens: number;
  totalTokens: number;
  avgDuration: number;
  errorRate: string;
}

interface ProviderStat {
  provider: string;
  _count: number;
  _sum: {
    promptTokens: number | null;
    completionTokens: number | null;
    cachedTokens: number | null;
    totalTokens: number | null;
  };
}

interface StatsData {
  overview: OverviewData;
  byProvider: ProviderStat[];
  byModel: Array<{
    model: string | null;
    _count: number;
    _sum: { totalTokens: number | null };
  }>;
}

function fmt(n: number): string {
  if (n >= 1_000_000) return (n / 1_000_000).toFixed(1) + "M";
  if (n >= 1_000) return (n / 1_000).toFixed(1) + "K";
  return n.toString();
}

/** 概览卡片配色方案 */
const CARD_THEMES = [
  { bg: "from-blue-50 to-blue-50/50", accent: "text-blue-600", icon: "M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-6l-2-2H5a2 2 0 00-2 2z" },
  { bg: "from-violet-50 to-violet-50/50", accent: "text-violet-600", icon: "M7 21a4 4 0 01-4-4V5a2 2 0 012-2h4a2 2 0 012 2v12a4 4 0 01-4 4zm0 0h12a2 2 0 002-2v-4a2 2 0 00-2-2h-2.343M11 7.343l1.657-1.657a2 2 0 012.828 0l2.829 2.829a2 2 0 010 2.828l-8.486 8.485M7 17h.01" },
  { bg: "from-emerald-50 to-emerald-50/50", accent: "text-emerald-600", icon: "M7 16V4m0 0L3 8m4-4l4 4m6 0v12m0 0l4-4m-4 4l-4-4" },
  { bg: "from-amber-50 to-amber-50/50", accent: "text-amber-600", icon: "M12 8v4l3 3m6-3a9 9 0 11-18 0 9 9 0 0118 0z" },
  { bg: "from-sky-50 to-sky-50/50", accent: "text-sky-600", icon: "M4 7v10c0 2 1 3 3 3h10c2 0 3-1 3-3V7c0-2-1-3-3-3H7c-2 0-3 1-3 3z" },
  { bg: "from-rose-50 to-rose-50/50", accent: "text-rose-500", icon: "M12 8v4m0 4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" },
  { bg: "from-teal-50 to-teal-50/50", accent: "text-teal-600", icon: "M9 19v-6a2 2 0 00-2-2H5a2 2 0 00-2 2v6a2 2 0 002 2h2a2 2 0 002-2zm0 0V9a2 2 0 012-2h2a2 2 0 012 2v10m-6 0a2 2 0 002 2h2a2 2 0 002-2m0 0V5a2 2 0 012-2h2a2 2 0 012 2v14a2 2 0 01-2 2h-2a2 2 0 01-2-2z" },
];

/** 进度条渐变色 */
const BAR_COLORS = [
  "from-blue-400 to-blue-500",
  "from-violet-400 to-violet-500",
  "from-emerald-400 to-emerald-500",
  "from-amber-400 to-amber-500",
  "from-sky-400 to-sky-500",
  "from-rose-400 to-rose-500",
  "from-teal-400 to-teal-500",
  "from-indigo-400 to-indigo-500",
  "from-pink-400 to-pink-500",
  "from-cyan-400 to-cyan-500",
];

export default function DashboardPage() {
  const [stats, setStats] = useState<StatsData | null>(null);
  const [days, setDays] = useState(7);
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

  const overviewCards = [
    { label: "总请求数", value: fmt(stats.overview.totalRequests) },
    { label: "总 Token", value: fmt(stats.overview.totalTokens) },
    { label: "输入 Token", value: fmt(stats.overview.totalPromptTokens) },
    { label: "输出 Token", value: fmt(stats.overview.totalCompletionTokens) },
    { label: "缓存 Token", value: fmt(stats.overview.totalCachedTokens) },
    { label: "平均耗时", value: `${stats.overview.avgDuration}ms` },
    { label: "错误率", value: `${stats.overview.errorRate}%` },
  ];

  return (
    <div className="space-y-6">
      {/* 页头 */}
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-lg font-semibold text-slate-900">数据概览</h2>
          <p className="text-xs text-muted-foreground mt-0.5">查看近期 API 调用和 Token 消耗统计</p>
        </div>
        <div className="flex gap-1 bg-slate-100/80 rounded-lg p-0.5">
          {[7, 14, 30].map((d) => (
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

      {/* 概览卡片 - 交错入场 */}
      <div className="grid grid-cols-2 sm:grid-cols-4 lg:grid-cols-7 gap-3">
        {overviewCards.map((card, i) => {
          const theme = CARD_THEMES[i % CARD_THEMES.length];
          return (
            <Card
              key={card.label}
              size="sm"
              className={`bg-gradient-to-br ${theme.bg} ring-0 border-0 shadow-sm hover:shadow-md transition-shadow duration-300 animate-in-up`}
              style={{ animationDelay: `${i * 50}ms` }}
            >
              <CardContent>
                <div className="flex items-center gap-1.5 mb-1.5">
                  <svg className={`h-3 w-3 ${theme.accent} opacity-70`} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                    <path d={theme.icon} />
                  </svg>
                  <p className="text-[11px] text-slate-500 font-medium">{card.label}</p>
                </div>
                <p className={`text-xl font-bold font-mono ${theme.accent}`}>{card.value}</p>
              </CardContent>
            </Card>
          );
        })}
      </div>

      {/* 按服务商统计 */}
      <Card className="shadow-sm ring-0 border border-slate-200/60">
        <CardHeader>
          <CardTitle className="text-slate-800">按服务商统计</CardTitle>
        </CardHeader>
        <CardContent>
          {stats.byProvider.length === 0 ? (
            <p className="text-sm text-muted-foreground py-4 text-center">暂无数据</p>
          ) : (
            <Table>
              <TableHeader>
                <TableRow className="hover:bg-transparent">
                  <TableHead className="text-slate-500">服务商</TableHead>
                  <TableHead className="text-right text-slate-500">请求数</TableHead>
                  <TableHead className="text-right text-slate-500">输入</TableHead>
                  <TableHead className="text-right text-slate-500">输出</TableHead>
                  <TableHead className="text-right text-slate-500">缓存</TableHead>
                  <TableHead className="text-right text-slate-500">总计</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {stats.byProvider.map((p) => (
                  <TableRow key={p.provider} className="hover:bg-slate-50/60 transition-colors">
                    <TableCell className="font-medium capitalize text-slate-700">{p.provider}</TableCell>
                    <TableCell className="text-right font-mono text-slate-600">{p._count}</TableCell>
                    <TableCell className="text-right font-mono text-blue-600/80">{fmt(p._sum.promptTokens ?? 0)}</TableCell>
                    <TableCell className="text-right font-mono text-emerald-600/80">{fmt(p._sum.completionTokens ?? 0)}</TableCell>
                    <TableCell className="text-right font-mono text-amber-600/80">{fmt(p._sum.cachedTokens ?? 0)}</TableCell>
                    <TableCell className="text-right font-mono font-semibold text-slate-800">{fmt(p._sum.totalTokens ?? 0)}</TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          )}
        </CardContent>
      </Card>

      {/* 热门模型 Top 10 */}
      <Card className="shadow-sm ring-0 border border-slate-200/60">
        <CardHeader>
          <CardTitle className="text-slate-800">热门模型 Top 10</CardTitle>
        </CardHeader>
        <CardContent>
          {stats.byModel.length === 0 ? (
            <p className="text-sm text-muted-foreground py-4 text-center">暂无数据</p>
          ) : (
            <div className="space-y-3.5">
              {stats.byModel.map((m, i) => {
                const maxCount = stats.byModel[0]._count;
                const pct = maxCount > 0 ? (m._count / maxCount) * 100 : 0;
                const barColor = BAR_COLORS[i % BAR_COLORS.length];
                const isTop3 = i < 3;

                return (
                  <div
                    key={m.model || i}
                    className="flex items-center gap-3 animate-in-up"
                    style={{ animationDelay: `${i * 40}ms` }}
                  >
                    <Badge
                      variant={isTop3 ? "default" : "outline"}
                      className={`w-6 justify-center font-mono text-[10px] ${
                        isTop3 ? "bg-gradient-to-r from-slate-700 to-slate-800 border-0" : ""
                      }`}
                    >
                      {i + 1}
                    </Badge>
                    <div className="flex-1 min-w-0">
                      <div className="flex items-center justify-between mb-1.5">
                        <span className="text-sm font-medium font-mono truncate text-slate-700">{m.model || "unknown"}</span>
                        <span className="text-xs text-slate-400 ml-2 shrink-0 font-mono">
                          {m._count} 次 · {fmt(m._sum.totalTokens ?? 0)} tokens
                        </span>
                      </div>
                      <div className="h-1.5 bg-slate-100 rounded-full overflow-hidden">
                        <div
                          className={`h-full bg-gradient-to-r ${barColor} rounded-full transition-all duration-700 ease-out`}
                          style={{ width: `${pct}%` }}
                        />
                      </div>
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
