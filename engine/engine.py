"""Transcription engine sidecar.

Reads an audio file (16kHz mono WAV preferred), transcribes it locally, and
emits NDJSON on stdout:

    {"type": "stage",    "stage": "load_model"}
    {"type": "progress", "stage": "transcribe", "progress": 0.42}
    {"type": "result",   "duration": ..., "words": [...], "segments": [...]}
    {"type": "error",    "message": "..."}

Engines:
    whisper   Whisper (MLX) + VAD segmentation + wav2vec2 forced alignment.
              Robust on accented English; word timestamps ~±50ms.
    parakeet  Parakeet TDT (MLX). Fastest; native word timestamps.
"""

from __future__ import annotations

import argparse
import json
import sys
import traceback


def emit(obj: dict) -> None:
    sys.stdout.write(json.dumps(obj, ensure_ascii=False) + "\n")
    sys.stdout.flush()


def emit_stage(stage: str) -> None:
    emit({"type": "stage", "stage": stage})


def emit_progress(stage: str, progress: float) -> None:
    emit({"type": "progress", "stage": stage, "progress": round(min(max(progress, 0.0), 1.0), 4)})


# ---------------------------------------------------------------------------
# Whisper + forced alignment
# ---------------------------------------------------------------------------

WHISPER_MODELS = {
    "large-v3": "large-v3",
    "large-v3-turbo": "large-v3-turbo",
    "medium": "medium",
    "small": "small",
}


def _reroute_logs_to_stderr() -> None:
    """stdout is reserved for the NDJSON protocol; push library logs to stderr."""
    import logging

    for logger in [logging.root, *(
        l for l in logging.Logger.manager.loggerDict.values() if isinstance(l, logging.Logger)
    )]:
        for handler in logger.handlers:
            if isinstance(handler, logging.StreamHandler) and handler.stream is sys.stdout:
                handler.setStream(sys.stderr)


def run_whisper(audio_path: str, model_name: str, language: str, batch_size: int) -> dict:
    import whispermlx

    _reroute_logs_to_stderr()
    emit_stage("load_model")
    model = whispermlx.load_model(
        WHISPER_MODELS.get(model_name, model_name),
        device="cpu",  # MLX runs on the GPU; "cpu" only routes the torch-side ops
        vad_method="silero",
        language=language,
    )

    audio = whispermlx.load_audio(audio_path)
    duration = float(len(audio)) / 16000.0

    emit_stage("transcribe")
    result = model.transcribe(
        audio,
        batch_size=batch_size,
        language=language,
        progress_callback=lambda p: emit_progress("transcribe", p / 100.0),
    )

    emit_stage("align")
    align_model, metadata = whispermlx.load_align_model(language_code=language, device="cpu")
    aligned = whispermlx.align(
        result["segments"],
        align_model,
        metadata,
        audio,
        "cpu",
        return_char_alignments=False,
        progress_callback=lambda p: emit_progress("align", p / 100.0),
    )

    words: list[dict] = []
    segments: list[dict] = []
    for seg in aligned["segments"]:
        seg_word_start = len(words)
        for w in seg.get("words", []):
            words.append(
                {
                    "word": w["word"].strip(),
                    "start": w.get("start"),
                    "end": w.get("end"),
                    "confidence": w.get("score"),
                }
            )
        segments.append(
            {
                "text": seg["text"].strip(),
                "start": seg.get("start"),
                "end": seg.get("end"),
                "wordStart": seg_word_start,
                "wordEnd": len(words),
            }
        )

    interpolated = fill_missing_timestamps(words, duration)
    return {
        "engine": f"whisper-{model_name}+align",
        "language": language,
        "duration": round(duration, 3),
        "words": words,
        "segments": segments,
        "interpolatedWords": interpolated,
    }


def fill_missing_timestamps(words: list[dict], duration: float) -> int:
    """Alignment can fail on tokens with no phonemes (digits, symbols). Fill
    those from neighbors and flag them so downstream tooling knows the timing
    is estimated rather than measured."""
    n = len(words)
    count = 0
    for i, w in enumerate(words):
        if w["start"] is not None and w["end"] is not None:
            continue
        count += 1
        prev_end = next(
            (words[j]["end"] for j in range(i - 1, -1, -1) if words[j]["end"] is not None), 0.0
        )
        next_start = next(
            (words[j]["start"] for j in range(i + 1, n) if words[j]["start"] is not None), duration
        )
        w["start"] = round(prev_end, 3)
        w["end"] = round(max(next_start, prev_end), 3)
        w["interpolated"] = True
        if w["confidence"] is None:
            w["confidence"] = 0.0
    return count


# ---------------------------------------------------------------------------
# Parakeet TDT
# ---------------------------------------------------------------------------

def run_parakeet(audio_path: str, model_name: str) -> dict:
    import soundfile as sf
    from parakeet_mlx import from_pretrained

    emit_stage("load_model")
    model = from_pretrained(model_name)

    info = sf.info(audio_path)
    duration = float(info.duration)

    chunk_duration = 120.0
    total_chunks = max(1, int(duration / (chunk_duration - 15.0)) + 1)

    def chunk_callback(current, total=None):
        try:
            done = float(current)
            whole = float(total) if total else float(total_chunks)
            emit_progress("transcribe", done / max(whole, 1.0))
        except Exception:
            pass

    emit_stage("transcribe")
    result = model.transcribe(
        audio_path,
        chunk_duration=chunk_duration,
        overlap_duration=15.0,
        chunk_callback=chunk_callback,
    )

    words: list[dict] = []
    segments: list[dict] = []
    for sentence in result.sentences:
        seg_word_start = len(words)
        for token in sentence.tokens:
            text = token.text
            # sentencepiece-style: a leading space starts a new word
            if text.startswith(" ") or len(words) == seg_word_start:
                words.append(
                    {
                        "word": text.strip(),
                        "start": round(token.start, 3),
                        "end": round(token.end, 3),
                        "confidence": round(token.confidence, 4),
                    }
                )
            else:
                w = words[-1]
                w["word"] += text
                w["end"] = round(token.end, 3)
                w["confidence"] = round(min(w["confidence"], token.confidence), 4)
        # drop empty tokens that stripped to nothing
        segments.append(
            {
                "text": sentence.text.strip(),
                "start": round(sentence.start, 3),
                "end": round(sentence.end, 3),
                "wordStart": seg_word_start,
                "wordEnd": len(words),
            }
        )

    words = [w for w in words if w["word"]]
    return {
        "engine": f"parakeet-{model_name.split('/')[-1]}",
        "language": "en",
        "duration": round(duration, 3),
        "words": words,
        "segments": segments,
        "interpolatedWords": 0,
    }


# ---------------------------------------------------------------------------

def main() -> None:
    parser = argparse.ArgumentParser(description="Local transcription engine")
    parser.add_argument("--audio", required=True, help="Path to 16kHz mono WAV")
    parser.add_argument("--engine", default="whisper", choices=["whisper", "parakeet"])
    parser.add_argument("--model", default=None)
    parser.add_argument("--language", default="en")
    parser.add_argument("--batch-size", type=int, default=8)
    args = parser.parse_args()

    try:
        if args.engine == "whisper":
            model = args.model or "large-v3-turbo"
            result = run_whisper(args.audio, model, args.language, args.batch_size)
        else:
            model = args.model or "mlx-community/parakeet-tdt-0.6b-v3"
            result = run_parakeet(args.audio, model)

        result["type"] = "result"
        emit(result)
    except Exception as exc:  # surface everything to the Rust side
        emit({"type": "error", "message": str(exc), "trace": traceback.format_exc()})
        sys.exit(1)


if __name__ == "__main__":
    main()
