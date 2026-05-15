import { useState, useEffect, useCallback, useRef, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import { convertFileSrc } from "@tauri-apps/api/core";
import hljs from "highlight.js/lib/core";
import javascript from "highlight.js/lib/languages/javascript";
import python from "highlight.js/lib/languages/python";
import java from "highlight.js/lib/languages/java";
import json from "highlight.js/lib/languages/json";
import sql from "highlight.js/lib/languages/sql";
import "highlight.js/styles/github-dark.css";
import "./App.css";

hljs.registerLanguage("javascript", javascript);
hljs.registerLanguage("python", python);
hljs.registerLanguage("java", java);
hljs.registerLanguage("json", json);
hljs.registerLanguage("sql", sql);

interface ClipboardItem {
  id: string;
  content: string;
  type: string;
  createdAt: number;
  favorite: boolean;
  pinned: boolean;
}

type Tab = "history" | "favorites";

function App() {
  const [items, setItems] = useState<ClipboardItem[]>([]);
  const [query, setQuery] = useState("");
  const [debouncedQuery, setDebouncedQuery] = useState("");
  const [tab, setTab] = useState<Tab>("history");
  const [selectedIdx, setSelectedIdx] = useState(0);
  const [editing, setEditing] = useState(false);
  const [editingItem, setEditingItem] = useState<ClipboardItem | null>(null);
  const [editContent, setEditContent] = useState("");
  const [viewCopied, setViewCopied] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  // Fix #8: Debounce search
  useEffect(() => {
    const timer = setTimeout(() => setDebouncedQuery(query), 200);
    return () => clearTimeout(timer);
  }, [query]);

  const loadItems = useCallback(async () => {
    let result: ClipboardItem[];
    if (tab === "favorites") {
      result = await invoke("get_favorites");
    } else if (debouncedQuery) {
      result = await invoke("search_items", { query: debouncedQuery });
    } else {
      result = await invoke("get_history");
    }
    // Fix #6: Don't reset selectedIdx if items haven't changed
    setItems(prev => {
      const prevIds = prev.map(i => i.id).join(",");
      const newIds = result.map(i => i.id).join(",");
      if (prevIds === newIds) {
        // Update content in place without resetting selection
        return result;
      }
      // Only reset if list actually changed
      setSelectedIdx(idx => Math.min(idx, Math.max(0, result.length - 1)));
      return result;
    });
  }, [debouncedQuery, tab]);

  useEffect(() => { loadItems(); }, [loadItems]);
  useEffect(() => {
    const interval = setInterval(loadItems, 2000);
    return () => clearInterval(interval);
  }, [loadItems]);
  useEffect(() => { inputRef.current?.focus(); }, []);

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

  const handleEdit = async (id: string, content: string) => {
    await invoke("update_item", { id, content });
    setEditing(false);
    setEditingItem(null);
    loadItems();
  };

  const startEdit = (item: ClipboardItem) => {
    setEditingItem(item);
    setEditContent(item.content);
    setEditing(true);
  };

  const cancelEdit = () => {
    setEditing(false);
    setEditingItem(null);
  };

  // Fix #14: Use content length + newlines to determine modal
  const needsModal = (content: string) => content.includes("\n") || content.length > 80;

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
      <div className="search-container">
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
        <kbd className="shortcut-hint">ESC</kbd>
      </div>

      {/* Tabs */}
      <div className="tab-bar">
        <button className={`tab ${tab === "history" ? "tab-active" : ""}`} onClick={() => setTab("history")}>
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><circle cx="12" cy="12" r="10"/><polyline points="12 6 12 12 16 14"/></svg>
          最近
        </button>
        <button className={`tab ${tab === "favorites" ? "tab-active" : ""}`} onClick={() => setTab("favorites")}>
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2"/></svg>
          收藏
        </button>
        <span className="item-count">{items.length} 条</span>
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
      </div>

      {/* Preview panel for selected item */}
      {items[selectedIdx] && items[selectedIdx].content.length > 150 && !editing && (
        <div className="preview-panel">
          <pre className="preview-content">{items[selectedIdx].content.slice(0, 800)}</pre>
        </div>
      )}

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
            ) : (
              <pre className="view-content">{editingItem.content}</pre>
            )}
          </div>
        </div>
      )}
    </div>
  );
}

function ItemRow({ item, selected, timeAgo, onPaste, onCopy, onEdit, onSelect, onToggleFavorite, onTogglePin, onDelete }: {
  item: ClipboardItem;
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
