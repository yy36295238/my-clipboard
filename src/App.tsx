import { useState, useEffect, useCallback, useRef, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { convertFileSrc } from "@tauri-apps/api/core";
import hljs from "highlight.js/lib/core";
import javascript from "highlight.js/lib/languages/javascript";
import typescript from "highlight.js/lib/languages/typescript";
import python from "highlight.js/lib/languages/python";
import java from "highlight.js/lib/languages/java";
import json from "highlight.js/lib/languages/json";
import sql from "highlight.js/lib/languages/sql";
import css from "highlight.js/lib/languages/css";
import xml from "highlight.js/lib/languages/xml";
import bash from "highlight.js/lib/languages/bash";
import go from "highlight.js/lib/languages/go";
import rust from "highlight.js/lib/languages/rust";
import c from "highlight.js/lib/languages/c";
import cpp from "highlight.js/lib/languages/cpp";
import yaml from "highlight.js/lib/languages/yaml";
import markdown from "highlight.js/lib/languages/markdown";
import "highlight.js/styles/github-dark.css";
import "./App.css";

hljs.registerLanguage("javascript", javascript);
hljs.registerLanguage("typescript", typescript);
hljs.registerLanguage("python", python);
hljs.registerLanguage("java", java);
hljs.registerLanguage("json", json);
hljs.registerLanguage("sql", sql);
hljs.registerLanguage("css", css);
hljs.registerLanguage("xml", xml);
hljs.registerLanguage("bash", bash);
hljs.registerLanguage("go", go);
hljs.registerLanguage("rust", rust);
hljs.registerLanguage("c", c);
hljs.registerLanguage("cpp", cpp);
hljs.registerLanguage("yaml", yaml);
hljs.registerLanguage("markdown", markdown);

interface ClipboardItem {
  id: string;
  content: string;
  type: string;
  createdAt: number;
  favorite: boolean;
  pinned: boolean;
}

type Tab = "history" | "favorites" | "images";
type DateFilter = "all" | "today" | "yesterday" | "beforeYesterday" | "custom";
type DatePickerTarget = "start" | "end" | null;

function App() {
  const [items, setItems] = useState<ClipboardItem[]>([]);
  const [query, setQuery] = useState("");
  const [debouncedQuery, setDebouncedQuery] = useState("");
  const [tab, setTab] = useState<Tab>("history");
  const [selectedIdx, setSelectedIdx] = useState(0);
  const [editing, setEditing] = useState(false);
  const [editingItem, setEditingItem] = useState<ClipboardItem | null>(null);
  const [confirmDeleteAll, setConfirmDeleteAll] = useState(false);
  const [viewCopied, setViewCopied] = useState(false);
  const [hasMore, setHasMore] = useState(true);
  const [dateFilter, setDateFilter] = useState<DateFilter>("all");
  const [customStartDate, setCustomStartDate] = useState("");
  const [customEndDate, setCustomEndDate] = useState("");
  const [datePickerTarget, setDatePickerTarget] = useState<DatePickerTarget>(null);
  const offsetRef = useRef(0);
  const hasMoreRef = useRef(true);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const dateRangeRef = useRef<HTMLDivElement>(null);

  // Fix #8: Debounce search
  useEffect(() => {
    const timer = setTimeout(() => setDebouncedQuery(query), 200);
    return () => clearTimeout(timer);
  }, [query]);

  const dateRange = useMemo(() => {
    const startOfDay = (date: Date) => new Date(date.getFullYear(), date.getMonth(), date.getDate());
    const toUnix = (date: Date) => Math.floor(date.getTime() / 1000);
    const today = startOfDay(new Date());
    if (dateFilter === "today") {
      return { startAt: toUnix(today), endAt: toUnix(new Date(today.getTime() + 86400000)) };
    }
    if (dateFilter === "yesterday") {
      return { startAt: toUnix(new Date(today.getTime() - 86400000)), endAt: toUnix(today) };
    }
    if (dateFilter === "beforeYesterday") {
      return { startAt: toUnix(new Date(today.getTime() - 2 * 86400000)), endAt: toUnix(new Date(today.getTime() - 86400000)) };
    }
    if (dateFilter === "custom") {
      const startAt = customStartDate ? toUnix(new Date(`${customStartDate}T00:00:00`)) : undefined;
      const endAt = customEndDate ? toUnix(new Date(new Date(`${customEndDate}T00:00:00`).getTime() + 86400000)) : undefined;
      return { startAt, endAt };
    }
    return { startAt: undefined, endAt: undefined };
  }, [dateFilter, customStartDate, customEndDate]);

  // Reset pagination when query, tab, or date filter changes
  useEffect(() => {
    offsetRef.current = 0;
    setHasMore(true);
    hasMoreRef.current = true;
    setItems([]);
  }, [debouncedQuery, tab, dateRange.startAt, dateRange.endAt]);

  const loadItems = useCallback(async (currentOffset = 0) => {
    let result: ClipboardItem[];
    const rangeArgs = { startAt: dateRange.startAt, endAt: dateRange.endAt };
    const hasDateFilter = dateRange.startAt !== undefined || dateRange.endAt !== undefined;
    if (tab === "favorites") {
      result = hasDateFilter
        ? await invoke("get_favorites_filtered", rangeArgs)
        : await invoke("get_favorites");
      setItems(result);
      setHasMore(false);
      return;
    } else if (tab === "images") {
      result = hasDateFilter
        ? await invoke("get_images_filtered", { offset: currentOffset, ...rangeArgs })
        : await invoke("get_images", { offset: currentOffset });
    } else if (debouncedQuery) {
      result = hasDateFilter
        ? await invoke("search_items_filtered", { query: debouncedQuery, offset: currentOffset, ...rangeArgs })
        : await invoke("search_items", { query: debouncedQuery, offset: currentOffset });
    } else {
      result = hasDateFilter
        ? await invoke("get_history_filtered", { offset: currentOffset, ...rangeArgs })
        : await invoke("get_history", { offset: currentOffset });
    }
    if (currentOffset === 0) {
      setItems(result);
    } else {
      setItems(prev => {
        const existingIds = new Set(prev.map(i => i.id));
        const newItems = result.filter(i => !existingIds.has(i.id));
        return [...prev, ...newItems];
      });
    }
    setHasMore(result.length === 30);
    hasMoreRef.current = result.length === 30;
    setSelectedIdx(idx => Math.min(idx, Math.max(0, (currentOffset === 0 ? result.length : items.length + result.length) - 1)));
  }, [debouncedQuery, tab, dateRange.startAt, dateRange.endAt]);

  useEffect(() => { loadItems(0); }, [loadItems]);
  useEffect(() => {
    const unlisten = listen("clipboard-updated", () => {
      loadItems(0);
    });
    return () => { unlisten.then(fn => fn()); };
  }, [loadItems]);
  useEffect(() => {
    inputRef.current?.focus();
    const unlisten = listen("panel-shown", async () => {
      // 面板重新弹出时只恢复一次 key window，避免每次点击都触发原生窗口切换而吞掉首个点击。
      await invoke("make_key_window");
      inputRef.current?.focus();
      await invoke("poll_clipboard");
      offsetRef.current = 0;
      loadItems(0);
    });
    return () => { unlisten.then(fn => fn()); };
  }, [loadItems]);

  // Scroll to bottom → load more
  useEffect(() => {
    const el = listRef.current;
    if (!el) return;
    const onScroll = () => {
      if (!hasMoreRef.current) return;
      if (el.scrollTop + el.clientHeight >= el.scrollHeight - 40) {
        const nextOffset = offsetRef.current + 30;
        offsetRef.current = nextOffset;
        loadItems(nextOffset);
      }
    };
    el.addEventListener("scroll", onScroll);
    return () => el.removeEventListener("scroll", onScroll);
  }, [loadItems]);

  useEffect(() => {
    const onMouseDown = (event: MouseEvent) => {
      if (dateRangeRef.current && !dateRangeRef.current.contains(event.target as Node)) {
        setDatePickerTarget(null);
      }
    };
    window.addEventListener("mousedown", onMouseDown);
    return () => window.removeEventListener("mousedown", onMouseDown);
  }, []);

  // Fix #4 & #5: Only handle global keys when not editing
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      // Block all global shortcuts during editing
      if (editing) return;

      if (e.key === "ArrowDown") {
        e.preventDefault();
        setSelectedIdx(i => Math.min(i + 1, items.length - 1));
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        setSelectedIdx(i => Math.max(i - 1, 0));
      } else if (e.key === "Enter" && items[selectedIdx]) {
        e.preventDefault();
        handlePaste(items[selectedIdx].content, items[selectedIdx].type);
      } else if (e.key === "Escape") {
        invoke("hide_window");
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [items, selectedIdx, editing]);

  useEffect(() => {
    const el = listRef.current?.children[selectedIdx] as HTMLElement;
    el?.scrollIntoView({ block: "nearest" });
  }, [selectedIdx]);

  // Fix #1: Single click = paste
  const handlePaste = async (content: string, contentType: string) => {
    await invoke("paste_item", { content, contentType });
  };

  // Fix #2: Silent copy
  const handleCopy = async (content: string, contentType: string) => {
    await invoke("copy_item", { content, contentType });
  };

  const handleToggleFavorite = async (id: string) => {
    await invoke("toggle_favorite", { id });
    loadItems();
  };

  const handleTogglePin = async (id: string) => {
    await invoke("toggle_pin", { id });
    loadItems();
  };

  // Delete immediately
  const handleDelete = async (item: ClipboardItem) => {
    setItems(prev => prev.filter(i => i.id !== item.id));
    await invoke("delete_item", { id: item.id });
  };

  const handleDeleteAll = async () => {
    await invoke("delete_all_items");
    setConfirmDeleteAll(false);
    offsetRef.current = 0;
    setItems([]);
    setHasMore(false);
    hasMoreRef.current = false;
  };

  const startEdit = (item: ClipboardItem) => {
    setEditingItem(item);
    setEditing(true);
  };

  const cancelEdit = () => {
    setEditing(false);
    setEditingItem(null);
  };

  const timeAgo = (ts: number) => {
    const diff = Math.floor(Date.now() / 1000 - ts);
    if (diff < 60) return "刚刚";
    if (diff < 3600) return `${Math.floor(diff / 60)}分钟前`;
    if (diff < 86400) return `${Math.floor(diff / 3600)}小时前`;
    return `${Math.floor(diff / 86400)}天前`;
  };

  // Fix #15: Separate pinned items
  const { pinned, unpinned } = useMemo(() => {
    const pinned = items.filter(i => i.pinned);
    const unpinned = items.filter(i => !i.pinned);
    return { pinned, unpinned };
  }, [items]);

  return (
    <div className="panel">
      {/* Search */}
      <div className="search-container" onMouseDown={e => { if ((e.target as HTMLElement).tagName !== 'INPUT') invoke('start_drag'); }}>
        <svg className="search-icon" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <circle cx="11" cy="11" r="8"/><path d="m21 21-4.3-4.3"/>
        </svg>
        <input
          ref={inputRef}
          className="search-input"
          type="text"
          placeholder="搜索剪贴板历史..."
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          spellCheck={false}
        />
        <button className="close-search-btn" onMouseDown={e => e.stopPropagation()} onClick={() => invoke("hide_window")} title="关闭">
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M18 6L6 18"/><path d="M6 6l12 12"/></svg>
        </button>
      </div>

      {/* Tabs */}
      <div className="tab-bar">
        <button className={`tab ${tab === "history" ? "tab-active" : ""}`} onMouseDown={() => setTab("history")} onClick={() => setTab("history")}>
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><rect x="3" y="3" width="7" height="7" rx="1"/><rect x="14" y="3" width="7" height="7" rx="1"/><rect x="3" y="14" width="7" height="7" rx="1"/><rect x="14" y="14" width="7" height="7" rx="1"/></svg>
          全部类型
        </button>
        <button className={`tab ${tab === "images" ? "tab-active" : ""}`} onMouseDown={() => setTab("images")} onClick={() => setTab("images")}>
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><rect x="3" y="3" width="18" height="18" rx="2" ry="2"/><circle cx="8.5" cy="8.5" r="1.5"/><polyline points="21 15 16 10 5 21"/></svg>
          图片
        </button>
        <button className={`tab ${tab === "favorites" ? "tab-active" : ""}`} onMouseDown={() => setTab("favorites")} onClick={() => setTab("favorites")}>
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2"/></svg>
          收藏
        </button>
        <span className="item-count">{items.length} 条</span>
      </div>

      <div className="filter-bar">
        <button className={`filter-chip ${dateFilter === "all" ? "filter-active" : ""}`} onMouseDown={() => setDateFilter("all")} onClick={() => setDateFilter("all")}>全部</button>
        <button className={`filter-chip ${dateFilter === "today" ? "filter-active" : ""}`} onMouseDown={() => setDateFilter("today")} onClick={() => setDateFilter("today")}>今天</button>
        <button className={`filter-chip ${dateFilter === "yesterday" ? "filter-active" : ""}`} onMouseDown={() => setDateFilter("yesterday")} onClick={() => setDateFilter("yesterday")}>昨天</button>
        <button className={`filter-chip ${dateFilter === "beforeYesterday" ? "filter-active" : ""}`} onMouseDown={() => setDateFilter("beforeYesterday")} onClick={() => setDateFilter("beforeYesterday")}>前天</button>
        <button className={`filter-chip ${dateFilter === "custom" ? "filter-active" : ""}`} onMouseDown={() => setDateFilter("custom")} onClick={() => setDateFilter("custom")}>指定范围</button>
        {dateFilter === "custom" && (
          <div className="date-range" ref={dateRangeRef}>
            <button className="date-field" onClick={() => setDatePickerTarget(datePickerTarget === "start" ? null : "start")}>
              <span>开始</span>
              <strong>{customStartDate || "选择日期"}</strong>
            </button>
            <span className="date-divider" />
            <button className="date-field" onClick={() => setDatePickerTarget(datePickerTarget === "end" ? null : "end")}>
              <span>结束</span>
              <strong>{customEndDate || "选择日期"}</strong>
            </button>
            {datePickerTarget && (
              <CalendarPopover
                value={datePickerTarget === "start" ? customStartDate : customEndDate}
                onSelect={(date) => {
                  if (datePickerTarget === "start") {
                    setCustomStartDate(date);
                  } else {
                    setCustomEndDate(date);
                  }
                  setDatePickerTarget(null);
                }}
              />
            )}
          </div>
        )}
        <button className="delete-all-btn" onClick={() => setConfirmDeleteAll(true)}>删除全部</button>
      </div>

      {/* List */}
      <div className="list" ref={listRef}>
        {/* Pinned section */}
        {pinned.length > 0 && (
          <>
            <div className="section-label">
              <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5"><path d="M12 17v5"/><path d="M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V17h14v-1.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V5a2 2 0 0 0-2-2h-2a2 2 0 0 0-2 2z"/></svg>
              置顶
            </div>
            {pinned.map(item => {
              const globalIdx = items.indexOf(item);
              return (
                <ItemRow
                  key={item.id}
                  item={item}
                  index={items.indexOf(item) + 1}
                  selected={globalIdx === selectedIdx}
                  timeAgo={timeAgo(item.createdAt)}
                  onPaste={handlePaste}
                  onCopy={handleCopy}
                  onEdit={startEdit}
                  onSelect={() => setSelectedIdx(globalIdx)}
                  onToggleFavorite={handleToggleFavorite}
                  onTogglePin={handleTogglePin}
                  onDelete={handleDelete}
                />
              );
            })}
            {unpinned.length > 0 && <div className="section-divider" />}
          </>
        )}

        {/* Regular items */}
        {unpinned.map(item => {
          const globalIdx = items.indexOf(item);
          return (
            <ItemRow
              key={item.id}
              item={item}
              index={items.indexOf(item) + 1}
              selected={globalIdx === selectedIdx}
              timeAgo={timeAgo(item.createdAt)}
              onPaste={handlePaste}
              onCopy={handleCopy}
              onEdit={startEdit}
              onSelect={() => setSelectedIdx(globalIdx)}
              onToggleFavorite={handleToggleFavorite}
              onTogglePin={handleTogglePin}
              onDelete={handleDelete}
            />
          );
        })}

        {items.length === 0 && (
          <div className="empty-state">
            <svg width="36" height="36" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" opacity="0.3">
              <rect x="9" y="9" width="13" height="13" rx="2" ry="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/>
            </svg>
            <p>暂无剪贴板记录</p>
            <span>复制内容后将自动显示在这里</span>
          </div>
        )}
        {hasMore && items.length > 0 && tab !== "favorites" && (
          <div className="load-more-hint">滚动加载更多</div>
        )}
        {!hasMore && items.length > 0 && tab !== "favorites" && (
          <div className="load-more-hint">— 已加载全部 {items.length} 条 —</div>
        )}
        {tab === "images" && items.length === 0 && (
          <div className="empty-state">
            <svg width="36" height="36" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" opacity="0.3"><rect x="3" y="3" width="18" height="18" rx="2" ry="2"/><circle cx="8.5" cy="8.5" r="1.5"/><polyline points="21 15 16 10 5 21"/></svg>
            <p>暂无图片记录</p>
            <span>复制图片后将自动显示在这里</span>
          </div>
        )}
      </div>

      {/* Footer */}
      <div className="footer">
        <span><kbd>↑↓</kbd> 导航</span>
        <span><kbd>↵</kbd> 粘贴</span>
        <span><kbd>esc</kbd> 关闭</span>
      </div>

      {/* View modal */}
      {editing && editingItem && (
        <div className="edit-modal-overlay" onClick={cancelEdit} onKeyDown={e => { if (e.key === "Escape") cancelEdit(); }} tabIndex={-1} ref={el => el?.focus()}>
          <div className="edit-modal" onClick={e => e.stopPropagation()}>
            <div className="view-modal-header">
              <span>查看内容</span>
              <div className="view-modal-actions">
                <button className={`close-btn ${viewCopied ? "copied" : ""}`} onClick={() => { handleCopy(editingItem.content, editingItem.type); setViewCopied(true); setTimeout(() => setViewCopied(false), 1500); }} title={viewCopied ? "已复制" : "复制"}>
                  {viewCopied ? (
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5"><polyline points="20 6 9 17 4 12"/></svg>
                  ) : (
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>
                  )}
                </button>
                <button className="close-btn" onClick={cancelEdit} title="关闭">
                  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M18 6L6 18"/><path d="M6 6l12 12"/></svg>
                </button>
              </div>
            </div>
            {editingItem.type === "image" ? (
              <img className="view-image" src={convertFileSrc(editingItem.content)} alt="clipboard image" />
            ) : (editingItem.type === "code" || editingItem.type === "json") ? (
              <pre className="view-content"><code dangerouslySetInnerHTML={{ __html: hljs.highlightAuto(editingItem.content).value }} /></pre>
            ) : (
              <pre className="view-content">{editingItem.content}</pre>
            )}
          </div>
        </div>
      )}

      {confirmDeleteAll && (
        <div className="edit-modal-overlay" onClick={() => setConfirmDeleteAll(false)} onKeyDown={e => { if (e.key === "Escape") setConfirmDeleteAll(false); }} tabIndex={-1} ref={el => el?.focus()}>
          <div className="confirm-modal" onClick={e => e.stopPropagation()}>
            <div className="confirm-title">删除全部记录</div>
            <p className="confirm-text">此操作会清空全部剪贴板历史，包括收藏和置顶记录，删除后无法恢复。</p>
            <div className="confirm-actions">
              <button className="edit-btn cancel" onClick={() => setConfirmDeleteAll(false)}>取消</button>
              <button className="edit-btn danger" onClick={handleDeleteAll}>确认删除</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

function CalendarPopover({ value, onSelect }: {
  value: string;
  onSelect: (date: string) => void;
}) {
  const selectedDate = value ? new Date(`${value}T00:00:00`) : new Date();
  const [cursor, setCursor] = useState(() => new Date(selectedDate.getFullYear(), selectedDate.getMonth(), 1));
  const monthStart = new Date(cursor.getFullYear(), cursor.getMonth(), 1);
  const firstDay = monthStart.getDay();
  const gridStart = new Date(monthStart);
  gridStart.setDate(monthStart.getDate() - firstDay);
  const days = Array.from({ length: 42 }, (_, index) => {
    const date = new Date(gridStart);
    date.setDate(gridStart.getDate() + index);
    return date;
  });
  const formatDate = (date: Date) => {
    const year = date.getFullYear();
    const month = `${date.getMonth() + 1}`.padStart(2, "0");
    const day = `${date.getDate()}`.padStart(2, "0");
    return `${year}-${month}-${day}`;
  };
  const changeMonth = (delta: number) => {
    setCursor(current => new Date(current.getFullYear(), current.getMonth() + delta, 1));
  };

  return (
    <div className="calendar-popover">
      <div className="calendar-header">
        <button onClick={() => changeMonth(-1)} title="上个月">
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="m15 18-6-6 6-6"/></svg>
        </button>
        <span>{cursor.getFullYear()}年 {cursor.getMonth() + 1}月</span>
        <button onClick={() => changeMonth(1)} title="下个月">
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="m9 18 6-6-6-6"/></svg>
        </button>
      </div>
      <div className="calendar-weekdays">
        {["日", "一", "二", "三", "四", "五", "六"].map(day => <span key={day}>{day}</span>)}
      </div>
      <div className="calendar-grid">
        {days.map(date => {
          const dateText = formatDate(date);
          const isOutside = date.getMonth() !== cursor.getMonth();
          const isSelected = value === dateText;
          const isToday = formatDate(new Date()) === dateText;
          return (
            <button
              key={dateText}
              className={`${isOutside ? "calendar-outside" : ""} ${isSelected ? "calendar-selected" : ""} ${isToday ? "calendar-today" : ""}`}
              onClick={() => onSelect(dateText)}
            >
              {date.getDate()}
            </button>
          );
        })}
      </div>
    </div>
  );
}

function ItemRow({ item, index, selected, timeAgo, onPaste, onCopy, onEdit, onSelect, onToggleFavorite, onTogglePin, onDelete }: {
  item: ClipboardItem;
  index: number;
  selected: boolean;
  timeAgo: string;
  onPaste: (content: string, contentType: string) => void;
  onCopy: (content: string, contentType: string) => void;
  onEdit: (item: ClipboardItem) => void;
  onSelect: () => void;
  onToggleFavorite: (id: string) => void;
  onTogglePin: (id: string) => void;
  onDelete: (item: ClipboardItem) => void;
}) {
  const codeRef = useRef<HTMLElement>(null);
  const [copied, setCopied] = useState(false);
  const isCode = item.type === "code" || item.type === "json";
  const isImage = item.type === "image";

  useEffect(() => {
    if (isCode && codeRef.current) {
      codeRef.current.textContent = item.content.slice(0, 300);
      hljs.highlightElement(codeRef.current);
    }
  }, [item.content, isCode]);

  const handleCopyClick = (e: React.MouseEvent) => {
    e.stopPropagation();
    onCopy(item.content, item.type);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };

  return (
    <div
      className={`item ${selected ? "item-selected" : ""} ${item.pinned ? "item-pinned" : ""}`}
      onClick={() => onSelect()}
      onDoubleClick={() => onPaste(item.content, item.type)}
    >
      <span className="item-index">{index}</span>
      <div className="item-left">
        <span className={`badge badge-${item.type}`}>{item.type}</span>
      </div>
      <div className="item-body">
        {isImage ? (
          <img className="item-image" src={convertFileSrc(item.content)} alt="clipboard image" />
        ) : isCode ? (
          <pre className="item-code"><code ref={codeRef}>{item.content.slice(0, 300)}</code></pre>
        ) : (
          <p className="item-text">{item.content.slice(0, 150)}</p>
        )}
      </div>
      <div className="item-right">
        <span className="item-time">{timeAgo}</span>
        <div className="item-actions">
          <button onClick={handleCopyClick} className={copied ? "action-active" : ""} title={copied ? "已复制" : "复制"}>
            {copied ? (
              <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5"><polyline points="20 6 9 17 4 12"/></svg>
            ) : (
              <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><rect x="9" y="9" width="13" height="13" rx="2" ry="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>
            )}
          </button>
          <button onClick={(e) => { e.stopPropagation(); onEdit(item); }} title="查看">
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z"/><circle cx="12" cy="12" r="3"/></svg>
          </button>
          <button onClick={(e) => { e.stopPropagation(); onToggleFavorite(item.id); }} className={item.favorite ? "action-active" : ""} title={item.favorite ? "取消收藏" : "收藏"}>
            <svg width="13" height="13" viewBox="0 0 24 24" fill={item.favorite ? "currentColor" : "none"} stroke="currentColor" strokeWidth="2"><polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2"/></svg>
          </button>
          <button onClick={(e) => { e.stopPropagation(); onTogglePin(item.id); }} className={item.pinned ? "action-active" : ""} title={item.pinned ? "取消置顶" : "置顶"}>
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M12 17v5"/><path d="M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V17h14v-1.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V5a2 2 0 0 0-2-2h-2a2 2 0 0 0-2 2z"/></svg>
          </button>
          <button onClick={(e) => { e.stopPropagation(); onDelete(item); }} className="action-danger" title="删除">
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M3 6h18"/><path d="M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6"/><path d="M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2"/></svg>
          </button>
        </div>
      </div>
    </div>
  );
}

export default App;
