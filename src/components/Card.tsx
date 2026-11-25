import { convertFileSrc } from "@tauri-apps/api/core";
import type { CSSProperties, MouseEvent } from "react";
import type { Item } from "../api";
import { relativeTime, TYPE_LABEL, type TypeKey, typeKey } from "../itemType";
import { TypeIcon } from "./TypeIcon";

const MAX_FILES_SHOWN = 5;
/** 不超过这么多字、这么多行的文本算「短内容」，在卡片里居中显示 */
const SHORT_TEXT_CHARS = 60;
const SHORT_TEXT_LINES = 3;

interface CardProps {
  item: Item;
  selected: boolean;
  onSelect: () => void;
  onPaste: () => void;
  onDelete: () => void;
  /** 列表做了虚拟化，卡片由父组件绝对定位 */
  style?: CSSProperties;
}

export function Card(props: CardProps) {
  const { item, selected, onSelect, onPaste, onDelete, style } = props;
  const type = typeKey(item);

  const handleDelete = (event: MouseEvent) => {
    event.stopPropagation();
    onDelete();
  };

  return (
    <div
      style={style}
      className={`card type-${type}${selected ? " selected" : ""}`}
      onClick={onSelect}
      onDoubleClick={onPaste}
    >
      <div className="card-head">
        <span className="card-type">
          <TypeIcon type={type} />
          {TYPE_LABEL[type]}
        </span>
        <span className="card-time">{relativeTime(item.updatedAt)}</span>
        <button
          type="button"
          className="card-delete"
          title="删除"
          onClick={handleDelete}
        >
          ×
        </button>
      </div>

      <div className="card-body">
        <CardBody item={item} type={type} />
      </div>

      <div className="card-foot">{describe(item)}</div>
    </div>
  );
}

function CardBody(props: { item: Item; type: TypeKey }) {
  const { item, type } = props;

  switch (type) {
    case "image":
      return item.imagePath ? (
        <img
          className="card-image"
          src={convertFileSrc(item.imagePath)}
          alt=""
          loading="lazy"
          decoding="async"
          draggable={false}
        />
      ) : null;

    case "files": {
      const paths = item.preview.split("\n");
      const rest = paths.length - MAX_FILES_SHOWN;
      return (
        <ul className="card-files">
          {paths.slice(0, MAX_FILES_SHOWN).map((path) => (
            <li key={path} title={path}>
              <TypeIcon type="files" />
              <span>{basename(path)}</span>
            </li>
          ))}
          {rest > 0 ? <li className="card-files-more">还有 {rest} 项</li> : null}
        </ul>
      );
    }

    case "color": {
      // Rust 侧只放行不含嵌套括号的颜色值，可以直接当 background 使用
      const value = item.preview.trim();
      return (
        <div className="card-color">
          <div className="card-swatch" style={{ background: value }} />
          <code>{value}</code>
        </div>
      );
    }

    case "url": {
      const value = item.preview.trim();
      return (
        <div className="card-url">
          <strong>{hostname(value)}</strong>
          <span>{value}</span>
        </div>
      );
    }

    default:
      return (
        <p className={`card-text${isShortText(item) ? " short" : ""}`}>
          {item.preview}
        </p>
      );
  }
}

/** 长段落居中会很难读，只有短内容才居中 */
function isShortText(item: Item): boolean {
  return (
    item.charCount <= SHORT_TEXT_CHARS &&
    item.preview.split("\n").length <= SHORT_TEXT_LINES
  );
}

function describe(item: Item): string {
  switch (item.kind) {
    case "image":
      return `${item.width ?? "?"} × ${item.height ?? "?"}`;
    case "files":
      return `${item.charCount} 个项目`;
    default:
      return `${item.charCount} 个字符`;
  }
}

function basename(path: string): string {
  return path.split("/").filter(Boolean).pop() ?? path;
}

function hostname(url: string): string {
  try {
    return new URL(url.startsWith("www.") ? `https://${url}` : url).hostname;
  } catch {
    return url;
  }
}
