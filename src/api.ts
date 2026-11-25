import { invoke } from "@tauri-apps/api/core";

export type Kind = "text" | "image" | "files";
export type SubKind = "url" | "email" | "color" | "path" | "html" | "rtf";

/** 与 Rust `db::Filter` 对应 */
export type Filter = "all" | "text" | "url" | "color" | "image" | "files";
/** 与 Rust `config::Period` 对应 */
export type Period = "day" | "week" | "all";

/** 与 Rust `db::ItemView` 对应 */
export interface Item {
  id: string;
  kind: Kind;
  subKind: SubKind | null;
  /** text：前 400 个字符；files：换行分隔的全部路径 */
  preview: string;
  /** text：字符数；files：文件个数 */
  charCount: number;
  /** 图片：列表显示用的文件（大图是缩略图） */
  imagePath: string | null;
  width: number | null;
  height: number | null;
  updatedAt: number;
}

/** 分页游标：上一页最后一条 */
export interface Cursor {
  updatedAt: number;
  id: string;
}

export interface Overview {
  period: Period;
  counts: Record<Filter, number>;
}

/** 与 Rust 侧事件名常量保持一致 */
export const EVENTS = {
  updated: "clipboard://updated",
  willShow: "panel://will-show",
  didHide: "panel://did-hide",
  settings: "settings://changed",
} as const;

export const listItems = (filter: Filter, cursor: Cursor | null, limit: number) =>
  invoke<Item[]>("list_items", { filter, cursor, limit });
export const getOverview = () => invoke<Overview>("get_overview");
export const pasteItem = (id: string) => invoke<void>("paste_item", { id });
export const deleteItem = (id: string) => invoke<void>("delete_item", { id });
export const hidePanel = () => invoke<void>("hide_panel");
/** 坐标为相对面板左上角的 CSS 像素 */
export const showMenu = (x: number, y: number) =>
  invoke<void>("show_menu", { x, y });

export interface ShortcutInfo {
  current: string;
  defaultValue: string;
}

export const getShortcut = () => invoke<ShortcutInfo>("get_shortcut");
/** 校验失败时 reject，错误信息可直接展示 */
export const checkShortcut = (accelerator: string) =>
  invoke<void>("check_shortcut", { accelerator });
export const setShortcut = (accelerator: string) =>
  invoke<void>("set_shortcut", { accelerator });
/** 关闭调用方自己的窗口 */
export const closeWindow = () => invoke<void>("close_window");
