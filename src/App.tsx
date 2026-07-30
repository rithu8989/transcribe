import { useCallback, useEffect, useRef, useState } from "react";
import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open, save } from "@tauri-apps/plugin-dialog";
import BrandLogo, { useSystemDark } from "./BrandLogo";
import { EXPORT_FORMATS } from "./exports";
import History from "./History";
import {
  EngineChoice,
  EngineEvent,
  Stage,
  STAGE_LABELS,
  STAGE_ORDER,
  TranscriptResult,
} from "./types";
import "./App.css";

type Phase = "idle" | "running" | "done" | "error";
type View = "transcribe" | "history";

const VIDEO_EXTS = ["mp4", "mov", "mkv", "webm", "avi", "m4v"];

// Module-level so StrictMode remounts / duplicate drop listeners share one lock.
let transcriptionInFlight = false;

function fmtClock(t: number): string {
  const m = Math.floor(t / 60);
  const s = t - m * 60;
  return `${m}:${s.toFixed(2).padStart(5, "0")}`;
}

function App() {
  const [view, setView] = useState<View>("transcribe");
  const [phase, setPhase] = useState<Phase>("idle");
  const [engine, setEngine] = useState<EngineChoice>("whisper");
  const [whisperModel, setWhisperModel] = useState("large-v3-turbo");
  const [stage, setStage] = useState<Stage>("extract");
  const [progress, setProgress] = useState<number | null>(null);
  const [result, setResult] = useState<TranscriptResult | null>(null);
  const [error, setError] = useState<string>("");
  const [dragOver, setDragOver] = useState(false);
  const [activeWord, setActiveWord] = useState(-1);
  const [exportedTo, setExportedTo] = useState<string>("");

  const mediaRef = useRef<HTMLVideoElement>(null);
  const rafRef = useRef<number>(0);
  const wordsRef = useRef<TranscriptResult["words"]>([]);
  const engineRef = useRef(engine);
  const whisperModelRef = useRef(whisperModel);
  engineRef.current = engine;
  whisperModelRef.current = whisperModel;

  const systemDark = useSystemDark();

  // Keep the Dock / taskbar icon in sync with OS appearance.
  useEffect(() => {
    invoke("set_app_icon_theme", { dark: systemDark }).catch(() => {
      /* not available outside the native shell */
    });
  }, [systemDark]);

  useEffect(() => {
    const unlisten = listen<EngineEvent>("engine-event", (e) => {
      const msg = e.payload;
      if (msg.type === "stage") {
        setStage(msg.stage);
        setProgress(null);
      } else if (msg.type === "progress") {
        setStage(msg.stage);
        setProgress(msg.progress ?? null);
      }
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  const startTranscription = useCallback(async (path: string) => {
    if (transcriptionInFlight) return;
    transcriptionInFlight = true;
    const chosenEngine = engineRef.current;
    const chosenModel = whisperModelRef.current;
    setView("transcribe");
    setPhase("running");
    setStage("extract");
    setProgress(null);
    setResult(null);
    setError("");
    setExportedTo("");
    setActiveWord(-1);
    try {
      const res = await invoke<TranscriptResult>("transcribe", {
        path,
        engine: chosenEngine,
        model: chosenEngine === "whisper" ? chosenModel : null,
      });
      wordsRef.current = res.words;
      setResult(res);
      setPhase("done");
    } catch (err) {
      setError(String(err));
      setPhase("error");
    } finally {
      transcriptionInFlight = false;
    }
  }, []);

  // Native file drag & drop — subscribe once; read engine/model from refs.
  useEffect(() => {
    let disposed = false;
    let unlistenFn: (() => void) | undefined;
    getCurrentWebview()
      .onDragDropEvent((event) => {
        if (disposed) return;
        if (event.payload.type === "over") {
          setDragOver(true);
        } else if (event.payload.type === "drop") {
          setDragOver(false);
          const paths = event.payload.paths;
          if (paths.length > 0) startTranscription(paths[0]);
        } else {
          setDragOver(false);
        }
      })
      .then((fn) => {
        if (disposed) fn();
        else unlistenFn = fn;
      });
    return () => {
      disposed = true;
      unlistenFn?.();
    };
  }, [startTranscription]);

  const browse = async () => {
    const file = await open({
      multiple: false,
      filters: [
        {
          name: "Media",
          extensions: [
            "mp3", "wav", "m4a", "aac", "flac", "ogg", "opus", "aiff", "wma",
            ...VIDEO_EXTS,
          ],
        },
      ],
    });
    if (typeof file === "string") startTranscription(file);
  };

  const cancel = async () => {
    await invoke("cancel_transcription");
    transcriptionInFlight = false;
    setPhase("idle");
  };

  // Karaoke highlight loop
  useEffect(() => {
    const tick = () => {
      const media = mediaRef.current;
      const words = wordsRef.current;
      if (media && words.length > 0 && !media.paused) {
        const t = media.currentTime;
        let lo = 0;
        let hi = words.length - 1;
        let found = -1;
        while (lo <= hi) {
          const mid = (lo + hi) >> 1;
          if (words[mid].end < t) lo = mid + 1;
          else if (words[mid].start > t) hi = mid - 1;
          else {
            found = mid;
            break;
          }
        }
        setActiveWord(found);
      }
      rafRef.current = requestAnimationFrame(tick);
    };
    rafRef.current = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(rafRef.current);
  }, []);

  const seekTo = (t: number) => {
    const media = mediaRef.current;
    if (!media) return;
    media.currentTime = t + 0.001;
    media.play();
  };

  const openHistoryItem = useCallback(async (id: string) => {
    try {
      const res = await invoke<TranscriptResult>("history_get", { id });
      wordsRef.current = res.words;
      setResult(res);
      setActiveWord(-1);
      setExportedTo("");
      setError("");
      setPhase("done");
      setView("transcribe");
    } catch (e) {
      setError(String(e));
      setPhase("error");
      setView("transcribe");
    }
  }, []);

  const doExport = async (fmt: (typeof EXPORT_FORMATS)[number]) => {
    if (!result) return;
    const base = result.sourceFile.split("/").pop()?.replace(/\.[^.]+$/, "") ?? "transcript";
    const dest = await save({
      defaultPath: `${base}.${fmt.ext}`,
      filters: [{ name: fmt.label, extensions: [fmt.ext] }],
    });
    if (!dest) return;
    await invoke("write_text_file", { path: dest, contents: fmt.make(result) });
    setExportedTo(dest);
  };

  const isVideo = result
    ? VIDEO_EXTS.includes(result.sourceFile.split(".").pop()?.toLowerCase() ?? "")
    : false;

  const stageIndex = STAGE_ORDER.indexOf(stage);

  return (
    <main className={`app ${dragOver ? "drag-over" : ""}`}>
      <header className="topbar">
        <div className="brand">
          <BrandLogo className="brand-logo" />
          <span className="brand-name">transcribe</span>
        </div>
        <nav className="tabs">
          <button
            className={`tab ${view === "transcribe" ? "active" : ""}`}
            onClick={() => setView("transcribe")}
          >
            Transcribe
          </button>
          <button
            className={`tab ${view === "history" ? "active" : ""}`}
            onClick={() => setView("history")}
          >
            History
          </button>
        </nav>
        <div className="controls">
          <label>
            Engine
            <select
              value={engine}
              onChange={(e) => setEngine(e.target.value as EngineChoice)}
              disabled={phase === "running"}
            >
              <option value="whisper">Whisper + alignment (accurate)</option>
              <option value="parakeet">Parakeet TDT (fast)</option>
            </select>
          </label>
          {engine === "whisper" && (
            <label>
              Model
              <select
                value={whisperModel}
                onChange={(e) => setWhisperModel(e.target.value)}
                disabled={phase === "running"}
              >
                <option value="large-v3-turbo">large-v3-turbo</option>
                <option value="large-v3">large-v3 (max accuracy)</option>
              </select>
            </label>
          )}
        </div>
      </header>

      {view === "history" && <History onOpen={openHistoryItem} />}

      {view === "transcribe" && (phase === "idle" || phase === "error") ? (
        <section className="dropzone" onClick={browse}>
          <div className="dropzone-inner">
            <div className="dropzone-icon">◉</div>
            <h1>Drop any audio or video file</h1>
            <p>or click to browse — everything runs locally on this Mac</p>
            {phase === "error" && <div className="error-box">{error}</div>}
          </div>
        </section>
      ) : null}

      {view === "transcribe" && phase === "running" && (
        <section className="progress-view">
          <div className="stages">
            {STAGE_ORDER.map((s, i) => (
              <div
                key={s}
                className={`stage ${i < stageIndex ? "done" : ""} ${i === stageIndex ? "active" : ""}`}
              >
                <span className="stage-marker">{i < stageIndex ? "✓" : i + 1}</span>
                {STAGE_LABELS[s]}
              </div>
            ))}
          </div>
          <div className="bar">
            <div
              className={`bar-fill ${progress === null ? "indeterminate" : ""}`}
              style={progress !== null ? { width: `${progress * 100}%` } : undefined}
            />
          </div>
          <p className="progress-note">
            {STAGE_LABELS[stage]}
            {progress !== null ? ` — ${Math.round(progress * 100)}%` : "…"}
            {stage === "load_model" && " (first run downloads the model)"}
          </p>
          <button className="ghost" onClick={cancel}>
            Cancel
          </button>
        </section>
      )}

      {view === "transcribe" && phase === "done" && result && (
        <section className="result-view">
          <div className="result-meta">
            <span className={`qa-badge ${result.qa.pass ? "pass" : "fail"}`}>
              {result.qa.pass ? "QA PASS" : `QA: ${result.qa.issues.length} issue(s)`}
            </span>
            {result.name && (
              <span className="meta-item result-name">
                {result.name}
                {result.version != null && result.version > 0 && (
                  <span className="version-pill inline">v{result.version}</span>
                )}
              </span>
            )}
            <span className="meta-item">{result.qa.wordCount} words</span>
            <span className="meta-item">{fmtClock(result.mediaDuration)}</span>
            <span className="meta-item">{result.engine}</span>
            {result.qa.interpolatedWords > 0 && (
              <span className="meta-item warn">
                {result.qa.interpolatedWords} interpolated
              </span>
            )}
            <div className="spacer" />
            {EXPORT_FORMATS.map((f) => (
              <button key={f.id} onClick={() => doExport(f)}>
                {f.label}
              </button>
            ))}
            <button
              className="ghost"
              onClick={() => {
                setPhase("idle");
                setResult(null);
              }}
            >
              New file
            </button>
          </div>

          {!result.qa.pass && (
            <div className="qa-issues">
              {result.qa.issues.slice(0, 5).map((issue, i) => (
                <div key={i}>{issue}</div>
              ))}
            </div>
          )}
          {exportedTo && <div className="export-note">Saved to {exportedTo}</div>}

          <video
            ref={mediaRef}
            className={isVideo ? "player video" : "player audio"}
            src={convertFileSrc(result.sourceFile)}
            controls
          />

          <div className="transcript">
            {result.segments.map((seg, si) => (
              <p key={si} className="segment">
                <span className="segment-time">{fmtClock(seg.start)}</span>
                {result.words.slice(seg.wordStart, seg.wordEnd).map((w, wi) => {
                  const idx = seg.wordStart + wi;
                  const lowConf = (w.confidence ?? 1) < 0.5;
                  return (
                    <span
                      key={idx}
                      className={`word ${idx === activeWord ? "active" : ""} ${w.interpolated ? "interpolated" : ""} ${lowConf ? "low-conf" : ""}`}
                      title={`${w.start.toFixed(3)}s → ${w.end.toFixed(3)}s${w.confidence != null ? ` · conf ${w.confidence.toFixed(2)}` : ""}${w.interpolated ? " · interpolated" : ""}`}
                      onClick={() => seekTo(w.start)}
                    >
                      {w.word}
                    </span>
                  );
                })}
              </p>
            ))}
          </div>
        </section>
      )}
    </main>
  );
}

export default App;
