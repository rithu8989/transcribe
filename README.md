# transcribe

**Local, word-level audio/video transcription for macOS (Apple Silicon).**

Drop any audio or video file in, get a drift-resistant transcript with per-word timestamps — built for AI video-editing pipelines where timing errors cascade into broken animations and captions.

Everything runs **on-device**. No cloud upload. No audio leaves your Mac.

> **Platform:** macOS on Apple Silicon only (M1 / M2 / M3 / M4).  
> Intel Macs and other OSes are not supported (MLX + Metal GPU).

---

## Features

- **Any media in** — mp4, mov, mkv, webm, mp3, wav, m4a, flac, aac, and more (via ffmpeg)
- **Word-level timestamps** (~±50ms with Whisper + forced alignment)
- **Two engines**
  - **Whisper + alignment** (default) — best for accented English (incl. Indian accents)
  - **Parakeet TDT** — faster single-pass native timestamps
- **QA gate** — every transcript is validated before you trust an export (monotonic times, bounds vs real media duration, gaps, confidence)
- **Click any word** to seek the built-in player (eyeball timing instantly)
- **Karaoke highlight** while audio/video plays
- **History** — auto-saved transcriptions, rename, soft-delete → Bin, restore, permanent delete
- **Version history** — re-transcribing the same file creates v2, v3… under one history card
- **Exports** — JSON (primary for AI pipelines), SRT, VTT (word-level cues), CSV
- **Theme-aware branding** — light/dark logos follow system appearance (UI + Dock)

---

## Stack

| Layer | Tech |
| --- | --- |
| Desktop shell | [Tauri 2](https://tauri.app) (Rust) |
| UI | React 19 + TypeScript + Vite 7 |
| Native APIs | `@tauri-apps/plugin-dialog`, asset protocol, Dock icon via AppKit |
| Media | Homebrew **ffmpeg** / **ffprobe** (extract 16 kHz mono WAV + independent duration) |
| ASR (default) | [whispermlx](https://pypi.org/project/whispermlx/) — Whisper large-v3 / turbo on **MLX** + Silero VAD + wav2vec2 forced alignment |
| ASR (fast) | [parakeet-mlx](https://github.com/senstella/parakeet-mlx) — NVIDIA Parakeet TDT 0.6B on **MLX** |
| Python tooling | [uv](https://github.com/astral-sh/uv) + Python 3.12 |
| Icons | SVG → PNG via `@resvg/resvg-js`, Tauri icon pipeline → `.icns` |

### Architecture

```
┌─────────────────┐     invoke / events      ┌──────────────────────┐
│  React UI       │ ◄──────────────────────► │  Rust (Tauri core)   │
│  src/           │                          │  src-tauri/          │
└─────────────────┘                          │  • ffmpeg extract    │
                                             │  • ffprobe duration  │
                                             │  • QA validator      │
                                             │  • history store     │
                                             │  • Dock icon theme   │
                                             └──────────┬───────────┘
                                                        │ NDJSON over stdio
                                                        ▼
                                             ┌──────────────────────┐
                                             │  Python engine       │
                                             │  engine/engine.py    │
                                             │  (uv run)            │
                                             └──────────────────────┘
```

Data lives under:

`~/Library/Application Support/com.transcriber.app/history/`

---

## Prerequisites

1. **Apple Silicon Mac** running a recent macOS
2. **[Homebrew](https://brew.sh)**
3. Install system tools:

```bash
brew install ffmpeg uv
```

4. **Node.js 20+** (npm comes with it)
5. **Rust** (for building the Tauri app):

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Xcode Command Line Tools are required for Rust/macOS builds:

```bash
xcode-select --install
```

---

## Setup (development)

```bash
# Clone / open the project
cd Transcriber

# JS dependencies
npm install

# Python engine + ML deps (creates engine/.venv)
cd engine && uv sync && cd ..

# Run the native app (Vite + Rust hot reload)
npm run tauri dev
```

The first launch opens a **transcribe** window. Drop a media file to try it.

### First transcription (model download)

Models download once into local caches (Hugging Face / torch hub), then the app works **offline**:

| Asset | Approx. size |
| --- | --- |
| Whisper large-v3-turbo (MLX) | ~1.6 GB |
| wav2vec2 aligner | ~360 MB |
| Parakeet TDT 0.6B v3 (optional) | ~600 MB |

Expect a longer wait on the first Whisper run; later runs are much faster.

---

## Usage

### Transcribe

1. Open the **Transcribe** tab
2. Choose engine (and Whisper model if applicable)
3. Drop a file or click to browse
4. Wait for stages: Extract → Load model → Transcribe → Align → QA
5. Click words to seek; export JSON / SRT / VTT / CSV

**UI cues**

- Amber underline = low confidence  
- Dashed amber underline = interpolated timestamp  

### History

- Every successful run is saved automatically
- Same source file again → new **version** (v2, v3…) under one card
- **Rename** applies to the whole group
- **Delete** → Bin (soft delete)
- Bin: **Restore** or **Delete forever** (hard delete); **Empty bin** clears all

### Word-level JSON (AI pipeline format)

```json
{
  "engine": "whisper-large-v3-turbo+align",
  "language": "en",
  "duration": 312.48,
  "mediaDuration": 312.5,
  "sourceFile": "/path/to/clip.mp4",
  "name": "clip",
  "version": 2,
  "qa": {
    "pass": true,
    "wordCount": 1042,
    "issues": [],
    "interpolatedWords": 0,
    "lowConfidenceWords": 3
  },
  "words": [
    { "word": "Hello", "start": 0.066, "end": 0.286, "confidence": 0.98 }
  ],
  "segments": [
    {
      "text": "Hello there.",
      "start": 0.066,
      "end": 0.586,
      "wordStart": 0,
      "wordEnd": 2
    }
  ]
}
```

### Engine A/B on your own voice

```bash
cd engine
# Prefer a 16 kHz mono WAV (or let the app extract one)
uv run engine.py --audio ~/Desktop/my-voice.wav --engine whisper
uv run engine.py --audio ~/Desktop/my-voice.wav --engine parakeet
```

Compare text quality; both produce word timestamps. Keep Whisper as default if your accent / Hinglish trips Parakeet.

---

## Project layout

```
Transcriber/
├── src/                    # React UI
│   ├── App.tsx             # Transcribe view, player, exports
│   ├── History.tsx         # History + Bin + versions
│   ├── BrandLogo.tsx       # Theme-aware logo
│   ├── exports.ts          # JSON / SRT / VTT / CSV
│   └── assets/             # logo-light.svg, logo-dark.svg
├── public/logo.svg         # Adaptive favicon
├── engine/                 # Python transcription sidecar
│   ├── engine.py           # NDJSON protocol
│   ├── pyproject.toml
│   └── .venv/              # uv-managed (gitignored)
├── src-tauri/              # Rust / Tauri
│   ├── src/
│   │   ├── lib.rs          # Commands
│   │   ├── engine.rs       # Spawn uv + stream progress
│   │   ├── media.rs        # ffmpeg / ffprobe
│   │   ├── qa.rs           # Timestamp validation
│   │   ├── history.rs      # Persist / versions / bin
│   │   └── icon.rs         # Dock icon light/dark
│   ├── icons/              # App icons + theme PNGs
│   └── tauri.conf.json
├── package.json
└── README.md
```

---

## Build a production `.app` + DMG (macOS)

This project is **Mac-only**. Packaging uses Tauri’s macOS bundler, which can emit both an `.app` and a `.dmg`.

### 1. Build

```bash
# From the repo root — ensure engine deps are synced first
cd engine && uv sync && cd ..

npm run tauri build
```

Outputs (typical locations):

```
src-tauri/target/release/bundle/macos/transcribe.app
src-tauri/target/release/bundle/dmg/transcribe_0.1.0_aarch64.dmg
```

Exact DMG filename includes version + architecture.

### 2. What “targets”: `"all"` means on Mac

In `src-tauri/tauri.conf.json`, `"bundle.targets": "all"` on macOS produces:

- **`.app`** — drag into `/Applications`
- **`.dmg`** — disk image for distribution

To build **only** DMG:

```bash
npm run tauri build -- --bundles dmg
```

Or only the app:

```bash
npm run tauri build -- --bundles app
```

### 3. Install from the DMG

1. Open the `.dmg`
2. Drag **transcribe** into **Applications**
3. First open: right-click → **Open** if Gatekeeper blocks an unsigned build

### 4. Important: current distribution model

**Dev / personal builds** expect these tools on the machine (PATH):

- `ffmpeg` / `ffprobe` (Homebrew)
- `uv` (Homebrew)

The Rust side spawns `uv run … engine/engine.py`. That works great for you and anyone who installs the same prerequisites.

A **fully self-contained** DMG for strangers (no Homebrew) still needs a follow-up:

1. Freeze the Python engine with **PyInstaller** (or similar)
2. Bundle a static **ffmpeg** binary
3. Declare both as Tauri [`externalBin`](https://v2.tauri.app/develop/sidecar/) sidecars
4. Point `src-tauri/src/engine.rs` at the sidecar instead of `uv`

Until then, treat the DMG as a packaged UI shell that still relies on `brew install ffmpeg uv` + a synced `engine/` tree (or install the engine next to the app in a documented layout).

### 5. Optional: nicer DMG with `create-dmg`

If you want a branded drag-to-Applications window after Tauri builds the `.app`:

```bash
brew install create-dmg

create-dmg \
  --volname "transcribe" \
  --window-pos 200 120 \
  --window-size 600 400 \
  --icon-size 100 \
  --icon "transcribe.app" 150 190 \
  --app-drop-link 450 190 \
  --hide-extension "transcribe.app" \
  "transcribe-installer.dmg" \
  "src-tauri/target/release/bundle/macos/"
```

### 6. Code signing & notarization (for sharing outside your Mac)

Unsigned apps trigger Gatekeeper warnings. For public distribution:

1. Apple Developer account
2. Sign the `.app` with your Developer ID Application certificate
3. Notarize with `notarytool` / Xcode
4. Staple the ticket to the app/DMG

Tauri docs: [macOS code signing](https://v2.tauri.app/distribute/sign/macos/).  
You can set signing identity via environment / `tauri.conf.json` → `bundle.macOS` when ready.

---

## Scripts reference

| Command | What it does |
| --- | --- |
| `npm run tauri dev` | Dev app (Vite + Rust) |
| `npm run build` | Frontend production build only |
| `npm run tauri build` | Release `.app` + DMG (and other bundles) |
| `cd engine && uv sync` | Install / refresh Python engine |
| `cd engine && uv run engine.py --audio FILE --engine whisper` | CLI transcription |

---

## Troubleshooting

| Problem | Fix |
| --- | --- |
| `ffmpeg not found` | `brew install ffmpeg` |
| `uv not found` | `brew install uv` |
| Engine fails on first run | Check network for model download; retry once cached |
| Dock icon doesn’t flip theme | Restart the app after a theme change (it listens for OS appearance) |
| History empty after rename / reinstall | Bundle id is `com.transcriber.app` — data stays under Application Support |
| Player silent but transcript OK | Source file moved/renamed; transcript still opens from history |
| Build fails on icon resources | Ensure `src-tauri/icons/icon-light.png` and `icon-dark.png` exist |

---

## License / privacy

- Transcription is **local-only** after models are downloaded.
- No analytics are bundled by this project.
- Upstream model licenses (OpenAI Whisper weights via MLX community, NVIDIA Parakeet, etc.) apply to the model weights you download — review those before commercial redistribution.
