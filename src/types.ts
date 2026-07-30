export interface Word {
  word: string;
  start: number;
  end: number;
  confidence: number | null;
  interpolated?: boolean;
}

export interface Segment {
  text: string;
  start: number;
  end: number;
  wordStart: number;
  wordEnd: number;
}

export interface QaReport {
  pass: boolean;
  wordCount: number;
  issues: string[];
  interpolatedWords: number;
  lowConfidenceWords: number;
  maxGapSeconds: number;
  durationDeltaSeconds: number;
}

export interface TranscriptResult {
  engine: string;
  language: string;
  duration: number;
  words: Word[];
  segments: Segment[];
  interpolatedWords: number;
  qa: QaReport;
  sourceFile: string;
  mediaDuration: number;
  historyId?: string;
  name?: string;
  version?: number;
  groupId?: string;
}

export interface HistoryMeta {
  id: string;
  groupId: string;
  version: number;
  name: string;
  createdAt: number;
  deletedAt: number | null;
  engine: string;
  duration: number;
  wordCount: number;
  sourceFile: string;
  qaPass: boolean;
}

export interface HistoryGroup {
  groupId: string;
  name: string;
  sourceFile: string;
  deletedAt: number | null;
  versions: HistoryMeta[];
}

export type EngineChoice = "whisper" | "parakeet";

export type Stage =
  | "extract"
  | "load_model"
  | "transcribe"
  | "align"
  | "qa";

export interface EngineEvent {
  type: "stage" | "progress";
  stage: Stage;
  progress?: number;
}

export const STAGE_LABELS: Record<Stage, string> = {
  extract: "Extracting audio",
  load_model: "Loading model",
  transcribe: "Transcribing",
  align: "Aligning words",
  qa: "Validating timestamps",
};

export const STAGE_ORDER: Stage[] = [
  "extract",
  "load_model",
  "transcribe",
  "align",
  "qa",
];
