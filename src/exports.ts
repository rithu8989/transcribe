import type { TranscriptResult } from "./types";

function pad(n: number, width: number): string {
  return String(n).padStart(width, "0");
}

function formatTime(seconds: number, msSeparator: string): string {
  const total = Math.max(0, seconds);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = Math.floor(total % 60);
  const ms = Math.round((total - Math.floor(total)) * 1000);
  return `${pad(h, 2)}:${pad(m, 2)}:${pad(s, 2)}${msSeparator}${pad(ms, 3)}`;
}

export function toJson(result: TranscriptResult): string {
  const { engine, language, duration, mediaDuration, sourceFile, words, segments, qa } = result;
  return JSON.stringify(
    { engine, language, duration, mediaDuration, sourceFile, qa, words, segments },
    null,
    2,
  );
}

export function toSrt(result: TranscriptResult): string {
  return result.segments
    .map((seg, i) => {
      const start = formatTime(seg.start, ",");
      const end = formatTime(seg.end, ",");
      return `${i + 1}\n${start} --> ${end}\n${seg.text}\n`;
    })
    .join("\n");
}

/** VTT with one cue per segment plus word-level cue timing tags, so players
 * that support it can do karaoke-style highlighting. */
export function toVtt(result: TranscriptResult): string {
  const cues = result.segments.map((seg) => {
    const start = formatTime(seg.start, ".");
    const end = formatTime(seg.end, ".");
    const words = result.words.slice(seg.wordStart, seg.wordEnd);
    const body =
      words.length > 0
        ? words
            .map((w, i) =>
              i === 0 ? w.word : `<${formatTime(w.start, ".")}>${w.word}`,
            )
            .join(" ")
        : seg.text;
    return `${start} --> ${end}\n${body}\n`;
  });
  return `WEBVTT\n\n${cues.join("\n")}`;
}

export function toCsv(result: TranscriptResult): string {
  const header = "index,word,start,end,duration,confidence,interpolated";
  const rows = result.words.map((w, i) => {
    const word = `"${w.word.replace(/"/g, '""')}"`;
    const dur = (w.end - w.start).toFixed(3);
    return `${i},${word},${w.start.toFixed(3)},${w.end.toFixed(3)},${dur},${w.confidence ?? ""},${w.interpolated ? "true" : "false"}`;
  });
  return [header, ...rows].join("\n");
}

export const EXPORT_FORMATS = [
  { id: "json", label: "JSON", ext: "json", make: toJson },
  { id: "srt", label: "SRT", ext: "srt", make: toSrt },
  { id: "vtt", label: "VTT", ext: "vtt", make: toVtt },
  { id: "csv", label: "CSV", ext: "csv", make: toCsv },
] as const;
