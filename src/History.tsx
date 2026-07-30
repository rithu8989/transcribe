import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ask } from "@tauri-apps/plugin-dialog";
import type { HistoryGroup, HistoryMeta } from "./types";

function fmtDate(ms: number): string {
  return new Date(ms).toLocaleString(undefined, {
    day: "numeric",
    month: "short",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function fmtDuration(s: number): string {
  const m = Math.floor(s / 60);
  const sec = Math.round(s % 60);
  return m > 0 ? `${m}m ${sec}s` : `${sec}s`;
}

interface Props {
  onOpen: (id: string) => void;
}

export default function History({ onOpen }: Props) {
  const [groups, setGroups] = useState<HistoryGroup[]>([]);
  const [showBin, setShowBin] = useState(false);
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [renamingId, setRenamingId] = useState<string | null>(null);
  const [renameValue, setRenameValue] = useState("");
  const [error, setError] = useState("");

  const refresh = useCallback(async () => {
    try {
      setGroups(await invoke<HistoryGroup[]>("history_list"));
      setError("");
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const active = groups.filter((g) => g.deletedAt == null);
  const binned = groups.filter((g) => g.deletedAt != null);
  const shown = showBin ? binned : active;

  const toggleExpand = (groupId: string) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(groupId)) next.delete(groupId);
      else next.add(groupId);
      return next;
    });
  };

  const startRename = (group: HistoryGroup) => {
    setRenamingId(group.groupId);
    setRenameValue(group.name);
  };

  const commitRename = async () => {
    if (!renamingId) return;
    const name = renameValue.trim();
    if (name) {
      try {
        await invoke("history_rename", { groupId: renamingId, name });
      } catch (e) {
        setError(String(e));
      }
    }
    setRenamingId(null);
    refresh();
  };

  const softDelete = async (groupId: string) => {
    await invoke("history_delete", { groupId });
    refresh();
  };

  const restore = async (groupId: string) => {
    await invoke("history_restore", { groupId });
    refresh();
  };

  const hardDeleteGroup = async (group: HistoryGroup) => {
    const ok = await ask(
      `Permanently delete "${group.name}" and all ${group.versions.length} version(s)? This cannot be undone.`,
      { title: "Delete forever", kind: "warning" },
    );
    if (!ok) return;
    await invoke("history_hard_delete", { groupId: group.groupId });
    refresh();
  };

  const hardDeleteVersion = async (version: HistoryMeta, groupName: string) => {
    const ok = await ask(
      `Permanently delete ${groupName} · v${version.version}? This cannot be undone.`,
      { title: "Delete version", kind: "warning" },
    );
    if (!ok) return;
    await invoke("history_hard_delete_version", { id: version.id });
    refresh();
  };

  const emptyBin = async () => {
    const ok = await ask(
      `Permanently delete all ${binned.length} item(s) in the bin? This cannot be undone.`,
      { title: "Empty bin", kind: "warning" },
    );
    if (!ok) return;
    await invoke("history_empty_bin");
    refresh();
  };

  const latest = (g: HistoryGroup) => g.versions[0];

  return (
    <section className="history-view">
      <div className="history-toolbar">
        <div className="seg-control">
          <button
            className={!showBin ? "seg active" : "seg"}
            onClick={() => setShowBin(false)}
          >
            Transcriptions ({active.length})
          </button>
          <button
            className={showBin ? "seg active" : "seg"}
            onClick={() => setShowBin(true)}
          >
            Bin ({binned.length})
          </button>
        </div>
        {showBin && binned.length > 0 && (
          <button className="danger" onClick={emptyBin}>
            Empty bin
          </button>
        )}
      </div>

      {error && <div className="error-box history-error">{error}</div>}

      {shown.length === 0 ? (
        <div className="history-empty">
          {showBin
            ? "The bin is empty."
            : "No transcriptions yet — drop a file in the Transcribe tab."}
        </div>
      ) : (
        <div className="history-list">
          {shown.map((group) => {
            const head = latest(group);
            const isOpen = expanded.has(group.groupId);
            const multi = group.versions.length > 1;
            return (
              <div key={group.groupId} className={`history-card ${isOpen ? "open" : ""}`}>
                <div className="history-row">
                  <button
                    className={`expand-btn ${multi ? "" : "invisible"}`}
                    onClick={() => multi && toggleExpand(group.groupId)}
                    aria-label={isOpen ? "Collapse versions" : "Expand versions"}
                    disabled={!multi}
                  >
                    {isOpen ? "▾" : "▸"}
                  </button>
                  <div
                    className="history-main"
                    onClick={() =>
                      renamingId !== group.groupId &&
                      !showBin &&
                      onOpen(head.id)
                    }
                    role="button"
                  >
                    {renamingId === group.groupId ? (
                      <input
                        className="rename-input"
                        value={renameValue}
                        autoFocus
                        onChange={(e) => setRenameValue(e.target.value)}
                        onKeyDown={(e) => {
                          if (e.key === "Enter") commitRename();
                          if (e.key === "Escape") setRenamingId(null);
                        }}
                        onBlur={commitRename}
                        onClick={(e) => e.stopPropagation()}
                      />
                    ) : (
                      <span className="history-name">
                        {group.name}
                        {multi && (
                          <span className="version-pill">
                            {group.versions.length} versions
                          </span>
                        )}
                      </span>
                    )}
                    <span className="history-sub">
                      v{head.version} · {fmtDate(head.createdAt)} ·{" "}
                      {fmtDuration(head.duration)} · {head.wordCount} words ·{" "}
                      {head.engine}
                      {!head.qaPass && (
                        <span className="history-qa-fail"> · QA issues</span>
                      )}
                    </span>
                  </div>
                  <div className="history-actions">
                    {showBin ? (
                      <>
                        <button className="ghost" onClick={() => restore(group.groupId)}>
                          Restore
                        </button>
                        <button className="danger" onClick={() => hardDeleteGroup(group)}>
                          Delete forever
                        </button>
                      </>
                    ) : (
                      <>
                        <button className="ghost" onClick={() => startRename(group)}>
                          Rename
                        </button>
                        <button className="ghost" onClick={() => onOpen(head.id)}>
                          Open latest
                        </button>
                        <button
                          className="ghost danger-ghost"
                          onClick={() => softDelete(group.groupId)}
                        >
                          Delete
                        </button>
                      </>
                    )}
                  </div>
                </div>

                {isOpen && multi && (
                  <div className="version-list">
                    {group.versions.map((v) => (
                      <div key={v.id} className="version-row">
                        <div
                          className="version-main"
                          onClick={() => !showBin && onOpen(v.id)}
                          role="button"
                        >
                          <span className="version-label">v{v.version}</span>
                          <span className="history-sub">
                            {fmtDate(v.createdAt)} · {fmtDuration(v.duration)} ·{" "}
                            {v.wordCount} words · {v.engine}
                            {!v.qaPass && (
                              <span className="history-qa-fail"> · QA issues</span>
                            )}
                          </span>
                        </div>
                        <div className="history-actions">
                          {!showBin && (
                            <button className="ghost" onClick={() => onOpen(v.id)}>
                              Open
                            </button>
                          )}
                          <button
                            className="ghost danger-ghost"
                            onClick={() => hardDeleteVersion(v, group.name)}
                          >
                            Delete forever
                          </button>
                        </div>
                      </div>
                    ))}
                  </div>
                )}
              </div>
            );
          })}
        </div>
      )}
    </section>
  );
}
