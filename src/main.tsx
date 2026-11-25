import { getCurrentWindow } from "@tauri-apps/api/window";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import { ShortcutSettings } from "./ShortcutSettings";
import "./styles.css";

// 两个窗口共用一个页面，按窗口 label 决定渲染哪个（与 Rust 侧 label 常量一致）
const label = getCurrentWindow().label;
document.documentElement.dataset.view = label;

// 正式包里屏蔽 WebView 默认右键菜单（重新载入等），开发时保留以便「检查元素」
if (!import.meta.env.DEV) {
  window.addEventListener("contextmenu", (event) => event.preventDefault());
}

const root = document.getElementById("root");
if (root) {
  createRoot(root).render(
    <StrictMode>
      {label === "shortcut" ? <ShortcutSettings /> : <App />}
    </StrictMode>,
  );
}
