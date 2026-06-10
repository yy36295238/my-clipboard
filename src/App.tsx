import { Fragment, useState, useEffect, useCallback, useRef, useMemo, memo } from "react";
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
  name: string;
  groupName: string;
}

type ContentTypeTab = "history" | "text" | "image" | "json" | "url" | "code" | "markdown" | "email" | "phone";
type Tab = ContentTypeTab | "favorites";
type DateFilter = "all" | "today" | "yesterday" | "beforeYesterday" | "custom";
type DatePickerTarget = "start" | "end" | null;

const contentTypes = [
  { value: "history", label: "全部" },
  { value: "text", label: "text" },
  { value: "image", label: "image" },
  { value: "json", label: "json" },
  { value: "url", label: "url" },
  { value: "code", label: "code" },
  { value: "markdown", label: "markdown" },
  { value: "email", label: "email" },
  { value: "phone", label: "phone" },
] satisfies { value: ContentTypeTab; label: string }[];

const tabOrder: Tab[] = [...contentTypes.map(t => t.value), "favorites"];

const timeAgo = (ts: number) => {
  const diff = Math.floor(Date.now() / 1000 - ts);
  if (diff < 60) return "刚刚";
  if (diff < 3600) return `${Math.floor(diff / 60)}分钟前`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}小时前`;
  return `${Math.floor(diff / 86400)}天前`;
};

// 列表用缩略图(thumb_ 前缀),加载失败时回退原图(老数据没有缩略图)
const thumbSrc = (path: string) => {
  const i = path.lastIndexOf("/");
  return path.slice(0, i + 1) + "thumb_" + path.slice(i + 1);
};

function App() {
  const [items, setItems] = useState<ClipboardItem[]>([]);
  const [query, setQuery] = useState("");
  const [debouncedQuery, setDebouncedQuery] = useState("");
  const [tab, setTab] = useState<Tab>("history");
  const [selectedIdx, setSelectedIdx] = useState(0);
  const [editing, setEditing] = useState(false);
  const [editingItem, setEditingItem] = useState<ClipboardItem | null>(null);
  const [editMode, setEditMode] = useState(false);
  const [draft, setDraft] = useState("");
  const [confirmDeleteAll, setConfirmDeleteAll] = useState(false);
  const [snippetTarget, setSnippetTarget] = useState<ClipboardItem | null>(null);
  const [snippetName, setSnippetName] = useState("");
  const [snippetGroup, setSnippetGroup] = useState("");
  const [snippetGroups, setSnippetGroups] = useState<string[]>([]);
  const [viewCopied, setViewCopied] = useState(false);
  const [hasMore, setHasMore] = useState(true);
  const [totalCount, setTotalCount] = useState(0);
  const [toast, setToast] = useState("");
  const [dateFilter, setDateFilter] = useState<DateFilter>("all");
  const [customStartDate, setCustomStartDate] = useState("");
  const [customEndDate, setCustomEndDate] = useState("");
  const [datePickerTarget, setDatePickerTarget] = useState<DatePickerTarget>(null);
  const offsetRef = useRef(0);
  const hasMoreRef = useRef(true);
  const itemsRef = useRef<ClipboardItem[]>([]);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const dateRangeRef = useRef<HTMLDivElement>(null);
  const totalCountRef = useRef(0);
  const toastTimer = useRef<number | undefined>(undefined);

  // Fix #8: Debounce search
  useEffect(() => {
    const timer = setTimeout(() => setDebouncedQuery(query), 200);
    return () => clearTimeout(timer);
  }, [query]);

  // timeAgo 定时刷新,避免面板久开后时间标签不更新
  const [, setTick] = useState(0);
  useEffect(() => {
    const timer = window.setInterval(() => setTick(v => v + 1), 30000);
    return () => clearInterval(timer);
  }, []);

  // 弹窗遮罩只在挂载时聚焦一次(内联 ref 会在每次重渲染时重新执行,
  // 把焦点从输入框抢走);若内部已有焦点元素(autoFocus 输入框)则不抢。
  const focusOverlay = useCallback((el: HTMLDivElement | null) => {
    if (el && !el.contains(document.activeElement)) el.focus();
  }, []);

  const showToast = useCallback((msg: string) => {
    setToast(msg);
    window.clearTimeout(toastTimer.current);
    toastTimer.current = window.setTimeout(() => setToast(""), 1200);
  }, []);

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

  const loadItems = useCallback(async (currentOffset = 0) => {
    let result: ClipboardItem[];
    const rangeArgs = { startAt: dateRange.startAt, endAt: dateRange.endAt };
    const hasDateFilter = dateRange.startAt !== undefined || dateRange.endAt !== undefined;
    const countArgs = {
      query: tab === "history" ? debouncedQuery : "",
      contentType: tab !== "history" && tab !== "favorites" ? tab : null,
      favoritesOnly: tab === "favorites",
      ...rangeArgs,
    };
    const total = currentOffset === 0
      ? await invoke<number>("count_items", countArgs)
      : totalCountRef.current;
    if (currentOffset === 0) {
      setTotalCount(total);
      totalCountRef.current = total;
    }
    if (tab === "favorites") {
      result = hasDateFilter
        ? await invoke("get_favorites_filtered", { offset: currentOffset, ...rangeArgs })
        : await invoke("get_favorites", { offset: currentOffset });
    } else if (tab !== "history") {
      result = hasDateFilter
        ? await invoke("get_items_by_type_filtered", { contentType: tab, offset: currentOffset, ...rangeArgs })
        : await invoke("get_items_by_type", { contentType: tab, offset: currentOffset });
    } else if (debouncedQuery) {
      result = hasDateFilter
        ? await invoke("search_items_filtered", { query: debouncedQuery, offset: currentOffset, ...rangeArgs })
        : await invoke("search_items", { query: debouncedQuery, offset: currentOffset });
    } else {
      result = hasDateFilter
        ? await invoke("get_history_filtered", { offset: currentOffset, ...rangeArgs })
        : await invoke("get_history", { offset: currentOffset });
    }
    const prev = currentOffset === 0 ? [] : itemsRef.current;
    const existingIds = new Set(prev.map(i => i.id));
    const merged = [...prev, ...result.filter(i => !existingIds.has(i.id))];
    itemsRef.current = merged;
    setItems(merged);
    setHasMore(merged.length < total);
    hasMoreRef.current = merged.length < total;
    setSelectedIdx(idx => Math.min(idx, Math.max(0, merged.length - 1)));
  }, [debouncedQuery, tab, dateRange.startAt, dateRange.endAt]);

  // query/tab/日期筛选变化时重置分页并重新加载(loadItems 的依赖即筛选条件)
  useEffect(() => {
    offsetRef.current = 0;
    hasMoreRef.current = true;
    totalCountRef.current = 0;
    itemsRef.current = [];
    loadItems(0);
  }, [loadItems]);
  useEffect(() => {
    const unlisten = listen("clipboard-updated", () => {
      loadItems(0);
    });
    return () => { unlisten.then(fn => fn()); };
  }, [loadItems]);
  useEffect(() => {
    inputRef.current?.focus();
    const unlisten = listen("panel-shown", async () => {
      // 面板重新弹出时只恢复一次 key window,避免每次点击都触发原生窗口切换而吞掉首个点击。
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

  // 单击/回车 = 粘贴
  const handlePaste = useCallback(async (content: string, contentType: string) => {
    await invoke("paste_item", { content, contentType });
  }, []);

  // 静默复制,不关闭面板
  const handleCopy = useCallback(async (content: string, contentType: string) => {
    await invoke("copy_item", { content, contentType });
  }, []);

  // 打开片段弹窗(命名 + 分组),分组列表用于自动补全
  const openSnippetModal = useCallback(async (item: ClipboardItem) => {
    setSnippetTarget(item);
    setSnippetName(item.name);
    setSnippetGroup(item.groupName);
    setSnippetGroups(await invoke<string[]>("get_snippet_groups"));
  }, []);

  // 未收藏 → 弹窗保存为片段;已收藏 → 直接移出片段库
  const handleToggleFavorite = useCallback(async (item: ClipboardItem) => {
    if (item.favorite) {
      await invoke("toggle_favorite", { id: item.id });
      loadItems(0);
    } else {
      openSnippetModal(item);
    }
  }, [loadItems, openSnippetModal]);

  const saveSnippet = async () => {
    if (!snippetTarget) return;
    try {
      if (!snippetTarget.favorite) {
        await invoke("toggle_favorite", { id: snippetTarget.id });
      }
      await invoke("set_snippet_meta", { id: snippetTarget.id, name: snippetName, groupName: snippetGroup });
      setSnippetTarget(null);
      showToast("已保存片段");
      loadItems(0);
    } catch {
      setSnippetTarget(null);
      showToast("保存失败");
    }
  };

  const handleTogglePin = useCallback(async (id: string) => {
    await invoke("toggle_pin", { id });
    loadItems(0);
  }, [loadItems]);

  // Delete immediately
  const handleDelete = useCallback(async (item: ClipboardItem) => {
    itemsRef.current = itemsRef.current.filter(i => i.id !== item.id);
    setItems(itemsRef.current);
    totalCountRef.current = Math.max(0, totalCountRef.current - 1);
    setTotalCount(totalCountRef.current);
    await invoke("delete_item", { id: item.id });
  }, []);

  const handleDeleteAll = async () => {
    await invoke("delete_all_items");
    setConfirmDeleteAll(false);
    offsetRef.current = 0;
    await loadItems(0);
  };

  const startEdit = useCallback((item: ClipboardItem) => {
    setEditingItem(item);
    setEditing(true);
    setEditMode(false);
  }, []);

  const cancelEdit = () => {
    setEditing(false);
    setEditingItem(null);
    setEditMode(false);
  };

  const beginEditMode = () => {
    if (editingItem) {
      setDraft(editingItem.content);
      setEditMode(true);
    }
  };

  const saveEdit = async () => {
    if (!editingItem) return;
    const newType = await invoke<string>("update_item", { id: editingItem.id, content: draft });
    setEditingItem({ ...editingItem, content: draft, type: newType });
    setEditMode(false);
    showToast("已保存");
    loadItems(0);
  };

  // Fix #4 & #5: Only handle global keys when not editing
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      // Block all global shortcuts when a modal is open
      if (editing || snippetTarget) return;

      if (e.metaKey && /^[1-9]$/.test(e.key)) {
        e.preventDefault();
        const target = items[Number(e.key) - 1];
        if (target) handlePaste(target.content, target.type);
      } else if (e.key === "Tab") {
        e.preventDefault();
        const cur = tabOrder.indexOf(tab);
        const next = (cur + (e.shiftKey ? tabOrder.length - 1 : 1)) % tabOrder.length;
        setTab(tabOrder[next]);
      } else if (e.key === "Backspace" && e.metaKey) {
        e.preventDefault();
        if (items[selectedIdx]) handleDelete(items[selectedIdx]);
      } else if (e.key === "ArrowDown") {
        e.preventDefault();
        setSelectedIdx(i => Math.min(i + 1, items.length - 1));
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        setSelectedIdx(i => Math.max(i - 1, 0));
      } else if (e.key === "Enter" && items[selectedIdx]) {
        e.preventDefault();
        if (e.shiftKey) {
          handleCopy(items[selectedIdx].content, items[selectedIdx].type);
          showToast("已复制");
        } else {
          handlePaste(items[selectedIdx].content, items[selectedIdx].type);
        }
      } else if (e.key === "Escape") {
        invoke("hide_window");
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [items, selectedIdx, editing, snippetTarget, tab, handlePaste, handleCopy, handleDelete, showToast]);

  useEffect(() => {
    const el = listRef.current?.children[selectedIdx] as HTMLElement;
    el?.scrollIntoView({ block: "nearest" });
  }, [selectedIdx]);

  // Fix #15: Separate pinned items
  const { pinned, unpinned } = useMemo(() => {
    const pinned = items.filter(i => i.pinned);
    const unpinned = items.filter(i => !i.pinned);
    return { pinned, unpinned };
  }, [items]);

  // 片段 tab 按分组聚合(后端已按 group_name 排序,这里保序分桶)
  const favoriteGroups = useMemo(() => {
    if (tab !== "favorites") return [];
    const map = new Map<string, ClipboardItem[]>();
    for (const item of items) {
      const key = item.groupName || "";
      const list = map.get(key);
      if (list) list.push(item);
      else map.set(key, [item]);
    }
    return [...map.entries()];
  }, [items, tab]);

  const renderRow = (item: ClipboardItem) => {
    const globalIdx = items.indexOf(item);
    return (
      <ItemRow
        key={item.id}
        item={item}
        index={globalIdx + 1}
        selected={globalIdx === selectedIdx}
        timeAgo={timeAgo(item.createdAt)}
        onPaste={handlePaste}
        onCopy={handleCopy}
        onEdit={startEdit}
        onSelect={setSelectedIdx}
        onToggleFavorite={handleToggleFavorite}
        onTogglePin={handleTogglePin}
        onSnippetEdit={openSnippetModal}
        onDelete={handleDelete}
      />
    );
  };

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
        <div className="type-tabs">
          {contentTypes.map(type => (
            <button
              key={type.value}
              className={`tab ${tab === type.value ? "tab-active" : ""}`}
              onMouseDown={() => setTab(type.value)}
              onClick={() => setTab(type.value)}
            >
              {type.label}
            </button>
          ))}
        </div>
        <span className="item-count">{items.length}/{totalCount}</span>
        <button className={`tab favorite-tab ${tab === "favorites" ? "tab-active" : ""}`} onMouseDown={() => setTab("favorites")} onClick={() => setTab("favorites")}>
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2"/></svg>
          片段
        </button>
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
        {tab === "favorites" ? (
          /* 片段按分组分区展示 */
          favoriteGroups.map(([group, list]) => (
            <Fragment key={group || "__ungrouped"}>
              <div className="section-label">
                <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5"><path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"/></svg>
                {group || "未分组"}
              </div>
              {list.map(renderRow)}
            </Fragment>
          ))
        ) : (
          <>
            {/* Pinned section */}
            {pinned.length > 0 && (
              <>
                <div className="section-label">
                  <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5"><path d="M12 17v5"/><path d="M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V17h14v-1.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V5a2 2 0 0 0-2-2h-2a2 2 0 0 0-2 2z"/></svg>
                  置顶
                </div>
                {pinned.map(renderRow)}
                {unpinned.length > 0 && <div className="section-divider" />}
              </>
            )}

            {/* Regular items */}
            {unpinned.map(renderRow)}
          </>
        )}

        {items.length === 0 && (
          tab === "image" ? (
            <div className="empty-state">
              <svg width="36" height="36" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" opacity="0.3"><rect x="3" y="3" width="18" height="18" rx="2" ry="2"/><circle cx="8.5" cy="8.5" r="1.5"/><polyline points="21 15 16 10 5 21"/></svg>
              <p>暂无图片记录</p>
              <span>复制图片后将自动显示在这里</span>
            </div>
          ) : (
            <div className="empty-state">
              <svg width="36" height="36" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" opacity="0.3">
                <rect x="9" y="9" width="13" height="13" rx="2" ry="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/>
              </svg>
              <p>暂无剪贴板记录</p>
              <span>复制内容后将自动显示在这里</span>
            </div>
          )
        )}
        {hasMore && items.length > 0 && (
          <div className="load-more-hint">滚动加载更多</div>
        )}
        {!hasMore && items.length > 0 && (
          <div className="load-more-hint">— 已加载全部 {items.length}/{totalCount} —</div>
        )}
      </div>

      {/* Footer */}
      <div className="footer">
        <span><kbd>↑↓</kbd> 导航</span>
        <span><kbd>↵/双击</kbd> 粘贴</span>
        <span><kbd>⇧↵</kbd> 复制</span>
        <span><kbd>⌘1-9</kbd> 快速粘贴</span>
        <span><kbd>⌘⌫</kbd> 删除</span>
        <span><kbd>esc</kbd> 关闭</span>
      </div>

      {toast && <div className="toast">{toast}</div>}

      {/* View / Edit modal */}
      {editing && editingItem && (
        <div className="edit-modal-overlay" onClick={cancelEdit} onKeyDown={e => { if (e.key === "Escape") { e.stopPropagation(); if (editMode) setEditMode(false); else cancelEdit(); } }} tabIndex={-1} ref={focusOverlay}>
          <div className="edit-modal" onClick={e => e.stopPropagation()}>
            <div className="view-modal-header">
              <span>{editMode ? "编辑内容" : "查看内容"}</span>
              <div className="view-modal-actions">
                {!editMode && editingItem.type !== "image" && (
                  <button className="close-btn" onClick={beginEditMode} title="编辑">
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M17 3a2.85 2.83 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5Z"/></svg>
                  </button>
                )}
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
            {editMode ? (
              <>
                <textarea
                  className="edit-textarea"
                  value={draft}
                  onChange={e => setDraft(e.target.value)}
                  spellCheck={false}
                  autoFocus
                />
                <div className="edit-actions">
                  <span className="edit-hint">esc 取消编辑</span>
                  <button className="edit-btn cancel" onClick={() => setEditMode(false)}>取消</button>
                  <button className="edit-btn save" onClick={saveEdit}>保存</button>
                </div>
              </>
            ) : editingItem.type === "image" ? (
              <img className="view-image" src={convertFileSrc(editingItem.content)} alt="clipboard image" />
            ) : (editingItem.type === "code" || editingItem.type === "json") ? (
              <pre className="view-content"><code dangerouslySetInnerHTML={{ __html: hljs.highlightAuto(editingItem.content).value }} /></pre>
            ) : (
              <pre className="view-content">{editingItem.content}</pre>
            )}
          </div>
        </div>
      )}

      {/* Snippet name/group modal */}
      {snippetTarget && (
        <div className="edit-modal-overlay" onClick={() => setSnippetTarget(null)} onKeyDown={e => { if (e.key === "Escape") { e.stopPropagation(); setSnippetTarget(null); } }} tabIndex={-1} ref={focusOverlay}>
          <div className="edit-modal edit-modal-sm" onClick={e => e.stopPropagation()}>
            <div className="view-modal-header">
              <span>{snippetTarget.favorite ? "编辑片段" : "保存为片段"}</span>
              <div className="view-modal-actions">
                <button className="close-btn" onClick={() => setSnippetTarget(null)} title="关闭">
                  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M18 6L6 18"/><path d="M6 6l12 12"/></svg>
                </button>
              </div>
            </div>
            <input
              className="edit-input"
              placeholder="片段名称(可选)"
              value={snippetName}
              onChange={e => setSnippetName(e.target.value)}
              onKeyDown={e => { if (e.key === "Enter") saveSnippet(); }}
              spellCheck={false}
              autoFocus
            />
            <input
              className="edit-input"
              placeholder="分组(可选,如:常用回复)"
              value={snippetGroup}
              onChange={e => setSnippetGroup(e.target.value)}
              onKeyDown={e => { if (e.key === "Enter") saveSnippet(); }}
              spellCheck={false}
            />
            {snippetGroups.length > 0 && (
              <div className="group-chips">
                {snippetGroups.map(g => (
                  <button
                    key={g}
                    className={`filter-chip ${snippetGroup === g ? "filter-active" : ""}`}
                    onClick={() => setSnippetGroup(snippetGroup === g ? "" : g)}
                  >
                    {g}
                  </button>
                ))}
              </div>
            )}
            <div className="confirm-actions">
              <button className="edit-btn cancel" onClick={() => setSnippetTarget(null)}>取消</button>
              <button className="edit-btn save" onClick={saveSnippet}>保存</button>
            </div>
          </div>
        </div>
      )}

      {confirmDeleteAll && (
        <div className="edit-modal-overlay" onClick={() => setConfirmDeleteAll(false)} onKeyDown={e => { if (e.key === "Escape") setConfirmDeleteAll(false); }} tabIndex={-1} ref={focusOverlay}>
          <div className="confirm-modal" onClick={e => e.stopPropagation()}>
            <div className="confirm-title">删除全部记录</div>
            <p className="confirm-text">此操作会清空剪贴板历史,收藏和置顶的内容会保留,删除后无法恢复。</p>
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

const ItemRow = memo(function ItemRow({ item, index, selected, timeAgo, onPaste, onCopy, onEdit, onSelect, onToggleFavorite, onTogglePin, onSnippetEdit, onDelete }: {
  item: ClipboardItem;
  index: number;
  selected: boolean;
  timeAgo: string;
  onPaste: (content: string, contentType: string) => void;
  onCopy: (content: string, contentType: string) => void;
  onEdit: (item: ClipboardItem) => void;
  onSelect: (idx: number) => void;
  onToggleFavorite: (item: ClipboardItem) => void;
  onTogglePin: (id: string) => void;
  onSnippetEdit: (item: ClipboardItem) => void;
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
      onClick={() => onSelect(index - 1)}
      onDoubleClick={() => onPaste(item.content, item.type)}
    >
      <span className="item-index">{index}</span>
      <div className="item-left">
        <span className={`badge badge-${item.type}`}>{item.type}</span>
      </div>
      <div className="item-body">
        {item.name && <div className="item-name">{item.name}</div>}
        {isImage ? (
          <img
            className="item-image"
            src={convertFileSrc(thumbSrc(item.content))}
            onError={(e) => {
              const img = e.currentTarget;
              const original = convertFileSrc(item.content);
              if (img.src !== original) img.src = original;
            }}
            alt="clipboard image"
          />
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
          <button onClick={(e) => { e.stopPropagation(); onToggleFavorite(item); }} className={item.favorite ? "action-active" : ""} title={item.favorite ? "移出片段" : "存为片段"}>
            <svg width="13" height="13" viewBox="0 0 24 24" fill={item.favorite ? "currentColor" : "none"} stroke="currentColor" strokeWidth="2"><polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2"/></svg>
          </button>
          {item.favorite && (
            <button onClick={(e) => { e.stopPropagation(); onSnippetEdit(item); }} title="编辑片段(命名/分组)">
              <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M12.586 2.586A2 2 0 0 0 11.172 2H4a2 2 0 0 0-2 2v7.172a2 2 0 0 0 .586 1.414l8.704 8.704a2.426 2.426 0 0 0 3.42 0l6.58-6.58a2.426 2.426 0 0 0 0-3.42z"/><circle cx="7.5" cy="7.5" r=".5" fill="currentColor"/></svg>
            </button>
          )}
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
});

export default App;
