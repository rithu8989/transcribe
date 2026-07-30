"""Smoke test: whispermlx transcription + wav2vec2 forced alignment on a test WAV."""

import json
import sys
import time

import whispermlx

AUDIO = sys.argv[1] if len(sys.argv) > 1 else "/tmp/transcriber-test/test.wav"

t0 = time.time()
print("loading whisper model...", flush=True)
model = whispermlx.load_model(
    "large-v3-turbo",
    device="cpu",
    vad_method="silero",
    language="en",
)
print(f"model loaded in {time.time() - t0:.1f}s", flush=True)

audio = whispermlx.load_audio(AUDIO)
t1 = time.time()
result = model.transcribe(audio, batch_size=8, language="en")
print(f"transcribed in {time.time() - t1:.1f}s", flush=True)

t2 = time.time()
align_model, metadata = whispermlx.load_align_model(language_code="en", device="cpu")
aligned = whispermlx.align(
    result["segments"], align_model, metadata, audio, "cpu",
    return_char_alignments=False,
)
print(f"aligned in {time.time() - t2:.1f}s", flush=True)

words = []
for seg in aligned["segments"]:
    for w in seg.get("words", []):
        words.append(w)

print(f"\n{len(words)} words:")
for w in words:
    start = w.get("start")
    end = w.get("end")
    score = w.get("score")
    print(f"  {start!s:>8} -> {end!s:>8}  ({score})  {w['word']}")

# QA: monotonicity check
ok = True
prev_end = 0.0
for w in words:
    if "start" not in w:
        print(f"MISSING TIMESTAMP: {w}")
        ok = False
        continue
    if w["start"] < prev_end - 0.01:
        print(f"NON-MONOTONIC: {w}")
        ok = False
    if w["end"] < w["start"]:
        print(f"END BEFORE START: {w}")
        ok = False
    prev_end = w["end"]

print("\nQA:", "PASS" if ok else "FAIL")
