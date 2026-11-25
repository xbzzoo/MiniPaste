import { useCallback, useEffect, useState } from "react";
import {
  checkShortcut,
  closeWindow,
  getShortcut,
  type ShortcutInfo,
  setShortcut,
} from "./api";
import { formatShortcut, recordKey, shortcutParts } from "./shortcut";

/**
 * 修改快捷键窗口。窗口打开期间 Rust 侧已暂停全局快捷键，按什么都能录进来。
 * 直接按组合键即可录制；↵ 保存，esc 取消。
 */
export function ShortcutSettings() {
  const [info, setInfo] = useState<ShortcutInfo | null>(null);
  /** 录到的新组合 */
  const [candidate, setCandidate] = useState<string | null>(null);
  /** 正按住、还没按主键的修饰键 */
  const [held, setHeld] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    getShortcut()
      .then(setInfo)
      .catch((err) => setError(String(err)));
  }, []);

  const choose = useCallback(async (accelerator: string) => {
    setCandidate(accelerator);
    setHeld([]);
    try {
      await checkShortcut(accelerator);
      setError(null);
    } catch (err) {
      setError(String(err));
    }
  }, []);

  const unchanged =
    !candidate ||
    !info ||
    formatShortcut(candidate) === formatShortcut(info.current);
  const canSave = !unchanged && !error && !saving;

  const save = async () => {
    if (!candidate || !canSave) return;
    setSaving(true);
    try {
      await setShortcut(candidate);
      await closeWindow();
    } catch (err) {
      setError(String(err));
      setSaving(false);
    }
  };

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      event.preventDefault();
      const bare =
        !event.metaKey && !event.ctrlKey && !event.altKey && !event.shiftKey;
      if (bare && event.key === "Escape") {
        void closeWindow();
        return;
      }
      if (bare && event.key === "Enter") {
        void save();
        return;
      }
      const recorded = recordKey(event);
      if (recorded.accelerator) {
        void choose(recorded.accelerator);
      } else {
        setHeld(recorded.held);
      }
    };
    // 松开修饰键时更新「正按住」的显示
    const handleKeyUp = (event: KeyboardEvent) =>
      setHeld(recordKey(event).held);

    window.addEventListener("keydown", handleKeyDown);
    window.addEventListener("keyup", handleKeyUp);
    return () => {
      window.removeEventListener("keydown", handleKeyDown);
      window.removeEventListener("keyup", handleKeyUp);
    };
  });

  const recording = held.length > 0;
  const shown = candidate ?? info?.current ?? "";
  const parts = recording ? [...held, "…"] : shortcutParts(shown);

  let hint = "直接按下新的组合键，需包含 ⌘、⌃、⌥ 中的至少一个";
  if (recording) hint = "继续按下一个字母、数字或功能键";
  else if (error) hint = error;
  else if (!unchanged) hint = "按 ↵ 或点「保存」生效";

  return (
    <div className="settings">
      <p className="settings-label">呼出面板的快捷键</p>
      <div
        className={`recorder${recording ? " recording" : ""}${
          error && !recording ? " invalid" : ""
        }`}
      >
        {parts.map((part, index) => (
          <kbd key={index}>{part}</kbd>
        ))}
      </div>
      <p className={`settings-hint${error && !recording ? " error" : ""}`}>
        {hint}
      </p>
      <footer className="settings-actions">
        {info ? (
          <button
            type="button"
            className="btn-link"
            onClick={() => void choose(info.defaultValue)}
          >
            恢复默认 {formatShortcut(info.defaultValue)}
          </button>
        ) : null}
        <span className="spacer" />
        <button type="button" className="btn" onClick={() => void closeWindow()}>
          取消
        </button>
        <button
          type="button"
          className="btn primary"
          disabled={!canSave}
          onClick={() => void save()}
        >
          保存
        </button>
      </footer>
    </div>
  );
}
