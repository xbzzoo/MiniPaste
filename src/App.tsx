import { listen } from "@tauri-apps/api/event";
import type {
  ChangeEvent,
  CompositionEvent,
  MouseEvent,
  WheelEvent,
} from "react";
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  deleteItem,
  EVENTS,
  type Filter,
  getOverview,
  hidePanel,
  type Item,
  listItems,
  type Overview,
  pasteItem,
  showMenu,
} from "./api";
import { Card } from "./components/Card";
import { FILTERS, PERIOD_LABEL } from "./itemType";

/** 每页条数。一屏放 5～8 张卡，60 条够翻好几屏 */
const PAGE_SIZE = 60;
/** 卡片尺寸，与 styles.css 保持一致 */
const CARD_WIDTH = 236;
const CARD_GAP = 14;
const STRIDE = CARD_WIDTH + CARD_GAP;
const PADDING_X = 20;
/** 可视区左右各多渲染几张，快速滚动时不露白 */
const OVERSCAN = 3;
/** 离已加载末尾不到这么多张时，预取下一页 */
const PREFETCH = 15;
/** 停止输入这么久后才搜索（一万条记录整表搜一遍约 30ms，不必每个键都查） */
const SEARCH_DELAY = 100;
/**
 * 搜索框刚被删空的这段时间里，⌫ 不删记录：连按 ⌫ 清空搜索词时多按一下，
 * 不该把选中的卡片删掉（删除没有撤销）
 */
const BACKSPACE_GUARD = 600;

/** 列表连同加载它时用的筛选和搜索词，翻页时沿用同一组条件 */
interface ListState {
  items: Item[];
  hasMore: boolean;
  filter: Filter;
  search: string;
}

/** 当前可见的卡片下标范围（不含 overscan） */
interface Range {
  first: number;
  last: number;
}

export default function App() {
  const [filter, setFilter] = useState<Filter>("all");
  /** 搜索框里的文字，包括输入法还没上屏的拼音 */
  const [query, setQuery] = useState("");
  const [list, setList] = useState<ListState>({
    items: [],
    hasMore: false,
    filter: "all",
    search: "",
  });
  const [overview, setOverview] = useState<Overview | null>(null);
  const [selected, setSelected] = useState(0);
  const [range, setRange] = useState<Range>({ first: 0, last: 0 });

  const listRef = useRef<HTMLElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  /** 每次从头加载 +1，晚到的旧结果直接丢弃 */
  const generation = useRef(0);
  /** 正在翻页或正在从头加载时为 true，避免拿旧列表的游标去翻新条件的页 */
  const loading = useRef(false);
  /** 事件回调里要读到最新的筛选和搜索词 */
  const filterRef = useRef(filter);
  const searchRef = useRef("");
  const searchTimer = useRef<number | undefined>(undefined);
  const composing = useRef(false);
  /** 搜索框最近一次被编辑的时间，见 BACKSPACE_GUARD */
  const editedAt = useRef(0);

  const count = list.items.length;
  const terms = useMemo(
    () => list.search.split(/\s+/).filter(Boolean),
    [list.search],
  );

  /** 从第一页重新加载列表和各类型计数。返回第一页；被更新的加载取代时返回 null */
  const reload = useCallback(async (nextFilter: Filter, search: string) => {
    const gen = ++generation.current;
    loading.current = true;
    try {
      const [items, nextOverview] = await Promise.all([
        listItems(nextFilter, search, null, PAGE_SIZE),
        getOverview(search),
      ]);
      if (gen !== generation.current) return null;
      setList({
        items,
        hasMore: items.length === PAGE_SIZE,
        filter: nextFilter,
        search,
      });
      setOverview(nextOverview);
      return items;
    } catch (err) {
      console.error("load items failed", err);
      return null;
    } finally {
      if (gen === generation.current) loading.current = false;
    }
  }, []);

  /** 接着最后一条往后取一页（游标分页，翻多深都一样快） */
  const loadMore = useCallback(async () => {
    const last = list.items.at(-1);
    if (loading.current || !list.hasMore || !last) return;
    const gen = generation.current;
    loading.current = true;
    try {
      const page = await listItems(
        list.filter,
        list.search,
        { updatedAt: last.updatedAt, id: last.id },
        PAGE_SIZE,
      );
      if (gen !== generation.current) return;
      setList((prev) => ({
        ...prev,
        items: [...prev.items, ...page],
        hasMore: page.length === PAGE_SIZE,
      }));
    } catch (err) {
      console.error("load more failed", err);
    } finally {
      if (gen === generation.current) loading.current = false;
    }
  }, [list]);

  const scrollToStart = () => {
    if (listRef.current) listRef.current.scrollLeft = 0;
  };

  const changeFilter = useCallback(
    (next: Filter) => {
      filterRef.current = next;
      setFilter(next);
      setSelected(0);
      scrollToStart();
      void reload(next, searchRef.current);
    },
    [reload],
  );

  /** 按新的搜索词从头加载 */
  const applySearch = useCallback(
    (text: string) => {
      window.clearTimeout(searchTimer.current);
      searchRef.current = text;
      setSelected(0);
      scrollToStart();
      return reload(filterRef.current, text);
    },
    [reload],
  );

  const scheduleSearch = (text: string) => {
    window.clearTimeout(searchTimer.current);
    if (text === searchRef.current) return;
    searchTimer.current = window.setTimeout(
      () => void applySearch(text),
      SEARCH_DELAY,
    );
  };

  const handleQueryChange = (event: ChangeEvent<HTMLInputElement>) => {
    const text = event.target.value;
    setQuery(text);
    editedAt.current = Date.now();
    // 拼音还没上屏时不搜，等 compositionend
    if (!composing.current) scheduleSearch(text);
  };

  const handleCompositionEnd = (event: CompositionEvent<HTMLInputElement>) => {
    composing.current = false;
    scheduleSearch(event.currentTarget.value);
  };

  const clearQuery = () => {
    setQuery("");
    editedAt.current = Date.now();
    void applySearch("");
  };

  const focusSearch = () => inputRef.current?.focus();

  useEffect(() => {
    changeFilter("all");

    const unlisteners = [
      // 收起时已复位并加载好，这里再拉一次主要是刷新「x 分钟前」
      listen(EVENTS.willShow, () => {
        focusSearch();
        void reload(filterRef.current, searchRef.current);
      }),
      // 趁看不见时复位到「全部 / 不搜索 / 第一条 / 最左」，下次弹出不闪
      listen(EVENTS.didHide, () => {
        window.clearTimeout(searchTimer.current);
        setQuery("");
        searchRef.current = "";
        changeFilter("all");
      }),
      listen(EVENTS.updated, () =>
        void reload(filterRef.current, searchRef.current),
      ),
      // 显示范围变了：回到第一条
      listen(EVENTS.settings, () => {
        setSelected(0);
        scrollToStart();
        void reload(filterRef.current, searchRef.current);
      }),
    ];
    // 面板成为键盘焦点窗口时，光标回到搜索框
    window.addEventListener("focus", focusSearch);

    return () => {
      for (const unlisten of unlisteners) void unlisten.then((off) => off());
      window.removeEventListener("focus", focusSearch);
    };
  }, [changeFilter, reload]);

  // 记录可见范围。只在范围变化时更新 state，滚动中不会每帧重渲染
  useLayoutEffect(() => {
    const el = listRef.current;
    if (!el) return;
    const measure = () => {
      const first = Math.max(
        0,
        Math.floor((el.scrollLeft - PADDING_X) / STRIDE),
      );
      const last = Math.ceil(
        (el.scrollLeft + el.clientWidth - PADDING_X) / STRIDE,
      );
      setRange((prev) =>
        prev.first === first && prev.last === last ? prev : { first, last },
      );
    };
    measure();
    el.addEventListener("scroll", measure, { passive: true });
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => {
      el.removeEventListener("scroll", measure);
      observer.disconnect();
    };
  }, []);

  // 快滚到（或用方向键选到）已加载的末尾时预取下一页
  useEffect(() => {
    if (list.hasMore && Math.max(range.last, selected) >= count - PREFETCH) {
      void loadMore();
    }
  }, [range.last, selected, count, list.hasMore, loadMore]);

  // 删除或切换后，选中下标可能越界
  useEffect(() => {
    if (selected > 0 && selected >= count) {
      setSelected(Math.max(0, count - 1));
    }
  }, [count, selected]);

  // 选中项滚进可视区。卡片是虚拟化的，不一定有 DOM，直接按下标算位置
  useEffect(() => {
    const el = listRef.current;
    if (!el) return;
    const left = PADDING_X + selected * STRIDE;
    const right = left + CARD_WIDTH;
    if (left - PADDING_X < el.scrollLeft) {
      el.scrollTo({ left: left - PADDING_X, behavior: "smooth" });
    } else if (right + PADDING_X > el.scrollLeft + el.clientWidth) {
      el.scrollTo({
        left: right + PADDING_X - el.clientWidth,
        behavior: "smooth",
      });
    }
  }, [selected]);

  const paste = async (item: Item) => {
    try {
      await pasteItem(item.id);
    } catch (err) {
      console.error("paste failed", err);
    }
  };

  const remove = async (item: Item) => {
    try {
      await deleteItem(item.id);
      // 本地摘掉即可，不必整页重载；计数单独刷新
      setList((prev) => ({
        ...prev,
        items: prev.items.filter((i) => i.id !== item.id),
      }));
      setOverview(await getOverview(searchRef.current));
    } catch (err) {
      console.error("delete failed", err);
    }
  };

  const openMenu = (event: MouseEvent<HTMLButtonElement>) => {
    const rect = event.currentTarget.getBoundingClientRect();
    void showMenu(rect.left, rect.bottom + 4);
  };

  // 点卡片、按钮时不让搜索框失焦，否则接着打字就没地方去了
  const keepSearchFocus = (event: MouseEvent) => {
    if (event.target !== inputRef.current) event.preventDefault();
  };

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      // 输入法组字中（拼音还没上屏），方向键、回车、esc、删除都归输入法。
      // WebKit 里确认候选的那下回车 isComposing 已是 false，只能靠 keyCode 229 认出来
      if (event.isComposing || event.keyCode === 229) return;
      const current = list.items[selected];

      switch (event.key) {
        case "ArrowRight":
          setSelected((i) => Math.max(0, Math.min(i + 1, count - 1)));
          break;
        case "ArrowLeft":
          setSelected((i) => Math.max(i - 1, 0));
          break;
        case "Enter":
          if (query !== searchRef.current) {
            // 打完字马上回车，防抖还没到点：先搜完再粘第一条，别粘成上一轮结果里的
            void applySearch(query).then((items) => {
              if (items?.[0]) void paste(items[0]);
            });
          } else if (current) {
            void paste(current);
          }
          break;
        case "Backspace":
        case "Delete": {
          // 搜索框有字时 ⌫ 是删字；⌘⌫ 任何时候都是删记录
          const typing =
            query !== "" ||
            event.repeat ||
            Date.now() - editedAt.current < BACKSPACE_GUARD;
          if (typing && !event.metaKey) return;
          if (current) void remove(current);
          break;
        }
        case "Escape":
          if (query) clearQuery();
          else void hidePanel();
          break;
        case "Tab": {
          const index = FILTERS.findIndex(({ key }) => key === filter);
          const step = event.shiftKey ? -1 : 1;
          const next = (index + step + FILTERS.length) % FILTERS.length;
          changeFilter(FILTERS[next].key);
          break;
        }
        default:
          // 搜索框万一失焦了（比如关掉菜单后），打字时把焦点拿回来，字照样落进去
          if (
            event.key.length === 1 &&
            !event.metaKey &&
            !event.ctrlKey &&
            document.activeElement !== inputRef.current
          ) {
            focusSearch();
          }
          return;
      }
      event.preventDefault();
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  });

  // 横向列表：把鼠标滚轮的纵向滚动转成横向
  const handleWheel = (event: WheelEvent<HTMLElement>) => {
    if (!listRef.current) return;
    if (Math.abs(event.deltaY) > Math.abs(event.deltaX)) {
      listRef.current.scrollLeft += event.deltaY;
    }
  };

  const start = Math.max(0, range.first - OVERSCAN);
  const end = Math.min(count, range.last + OVERSCAN);

  return (
    <div className="sheet" onMouseDown={keepSearchFocus}>
      <header className="toolbar">
        <div className="toolbar-start">
          <label className={`search${query ? " filled" : ""}`}>
            <SearchIcon />
            <input
              ref={inputRef}
              value={query}
              placeholder="搜索全部记录"
              spellCheck={false}
              autoComplete="off"
              autoCorrect="off"
              autoFocus
              onChange={handleQueryChange}
              onCompositionStart={() => {
                composing.current = true;
              }}
              onCompositionEnd={handleCompositionEnd}
            />
            {query ? (
              <button
                type="button"
                className="search-clear"
                title="清空"
                onClick={clearQuery}
              >
                ×
              </button>
            ) : null}
          </label>
          <nav className="filters">
            {FILTERS.map(({ key, label }) => (
              <button
                key={key}
                type="button"
                className={`chip${filter === key ? " active" : ""}`}
                onClick={() => changeFilter(key)}
              >
                {label}
                <span className="chip-count">
                  {(overview?.counts[key] ?? 0).toLocaleString()}
                </span>
              </button>
            ))}
          </nav>
        </div>
        <div className="toolbar-end">
          <div className="hints">
            <kbd>←</kbd>
            <kbd>→</kbd> 选择
            <kbd>↵</kbd> 粘贴
            {query ? (
              <>
                <kbd>⌘</kbd>
                <kbd>⌫</kbd> 删除
              </>
            ) : (
              <>
                <kbd>⌫</kbd> 删除
              </>
            )}
            <kbd>tab</kbd> 切换类型
            <kbd>esc</kbd> {query ? "清空搜索" : "关闭"}
          </div>
          <button
            type="button"
            className="menu-button"
            title="显示范围、存储空间、清理记录等"
            onClick={openMenu}
          >
            {/* 搜索时不受显示范围限制，不显示它免得误会 */}
            {overview && !list.search ? PERIOD_LABEL[overview.period] : null}
            <MoreIcon />
          </button>
        </div>
      </header>

      <main ref={listRef} className="list" onWheel={handleWheel}>
        {count === 0 ? (
          <Empty overview={overview} search={list.search} />
        ) : (
          <div
            className="track"
            style={{ width: PADDING_X * 2 + count * STRIDE - CARD_GAP }}
          >
            {list.items.slice(start, end).map((item, offset) => {
              const index = start + offset;
              return (
                <Card
                  key={item.id}
                  style={{ left: PADDING_X + index * STRIDE }}
                  item={item}
                  terms={terms}
                  selected={index === selected}
                  onSelect={() => setSelected(index)}
                  onPaste={() => void paste(item)}
                  onDelete={() => void remove(item)}
                />
              );
            })}
          </div>
        )}
      </main>
    </div>
  );
}

function Empty(props: { overview: Overview | null; search: string }) {
  const { overview, search } = props;
  if (!overview) return null;

  if (search) {
    return overview.counts.all > 0 ? (
      <div className="empty">这个类型下没有匹配的记录</div>
    ) : (
      <div className="empty">
        没有找到「{search.trim()}」
        <small>图片不参与搜索</small>
      </div>
    );
  }
  if (overview.counts.all > 0) {
    return <div className="empty">这个类型下还没有记录</div>;
  }
  if (overview.period === "all") {
    return <div className="empty">还没有记录，复制点什么试试</div>;
  }
  return (
    <div className="empty">
      {PERIOD_LABEL[overview.period]}没有记录
      <small>点右上角可以切换到更长的时间范围</small>
    </div>
  );
}

function SearchIcon() {
  return (
    <svg
      className="icon"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.2"
      strokeLinecap="round"
      aria-hidden
    >
      <circle cx="11" cy="11" r="6.5" />
      <path d="m16 16 4.5 4.5" />
    </svg>
  );
}

function MoreIcon() {
  return (
    <svg className="icon" viewBox="0 0 24 24" fill="currentColor" aria-hidden>
      <circle cx="5" cy="12" r="1.8" />
      <circle cx="12" cy="12" r="1.8" />
      <circle cx="19" cy="12" r="1.8" />
    </svg>
  );
}
