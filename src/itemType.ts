import type { Filter, Item, Kind, Period, SubKind } from "./api";

/** 卡片上展示的类型：文本有子类型时用子类型，否则用大类 */
export type TypeKey = Kind | SubKind;

export const typeKey = (item: Item): TypeKey => item.subKind ?? item.kind;

export const TYPE_LABEL: Record<TypeKey, string> = {
  text: "文本",
  url: "链接",
  email: "邮箱",
  color: "颜色",
  path: "路径",
  html: "富文本",
  rtf: "富文本",
  image: "图片",
  files: "文件",
};

export const FILTERS: { key: Filter; label: string }[] = [
  { key: "all", label: "全部" },
  { key: "text", label: "文本" },
  { key: "url", label: "链接" },
  { key: "color", label: "颜色" },
  { key: "image", label: "图片" },
  { key: "files", label: "文件" },
];

export const PERIOD_LABEL: Record<Period, string> = {
  day: "最近 24 小时",
  week: "最近一周",
  all: "全部时间",
};

export function relativeTime(ms: number): string {
  const minutes = Math.floor((Date.now() - ms) / 60_000);
  if (minutes < 1) return "刚刚";
  if (minutes < 60) return `${minutes} 分钟前`;

  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours} 小时前`;

  const days = Math.floor(hours / 24);
  if (days === 1) return "昨天";
  if (days < 7) return `${days} 天前`;

  const date = new Date(ms);
  return `${date.getMonth() + 1}月${date.getDate()}日`;
}
