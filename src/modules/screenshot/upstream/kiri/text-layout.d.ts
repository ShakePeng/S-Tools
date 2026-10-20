// Adapted from Kiri (MIT License), commit 52d2d2f8fd613ce3341cbfae371009f06b123903.
// Copyright (c) 2026 yuxino. See THIRD_PARTY_NOTICES.md.

export interface TextFrame {
  x: number;
  y: number;
  width: number;
  height: number;
}

type MeasureText = (text: string) => number;

export const TEXT_TAB_SIZE: number;
export function textLineRuns(text: string, measureText: MeasureText): {
  runs: { text: string; x: number }[];
  width: number;
};
export function textEditorInsets(uiScale?: number): { x: number; y: number };

export function layoutTextLines(
  text: string,
  maxWidth: number,
  measureText: MeasureText,
): string[];

export function fitTextEditorFrame(options: {
  text: string;
  fontSize: number;
  x: number;
  y: number;
  maxWidth: number;
  boundsWidth: number;
  boundsHeight: number;
  measureText: MeasureText;
  uiScale?: number;
}): TextFrame;
