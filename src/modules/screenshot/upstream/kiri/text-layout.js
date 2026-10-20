// Adapted from Kiri (MIT License), commit 52d2d2f8fd613ce3341cbfae371009f06b123903.
// Copyright (c) 2026 yuxino. See THIRD_PARTY_NOTICES.md.

export const TEXT_TAB_SIZE = 8;

/** Canvas does not apply CSS tab stops; share their positions with the editor. */
export function textLineRuns(text, measureText) {
  const tabWidth = Math.max(1, measureText(" ".repeat(TEXT_TAB_SIZE)));
  const runs = [];
  let x = 0;
  for (const [index, part] of text.split("\t").entries()) {
    if (index) x = (Math.floor(x / tabWidth) + 1) * tabWidth;
    if (part) runs.push({ text: part, x });
    x += measureText(part);
  }
  return { runs, width: x };
}

export function textEditorInsets(uiScale = 1) {
  // Textarea has 8/5px padding and a 1px border on each side.
  return { x: 9 * uiScale, y: 6 * uiScale };
}

export function layoutTextLines(text, maxWidth, measureText) {
  // Spec §5.5: wrap within the rect width. CJK text has no spaces, so
  // break per character for CJK runs while keeping Latin word wrapping.
  // Explicit empty lines are retained so the editor and exported annotation
  // agree on the text frame height.
  const lines = [];
  const measureLine = (value) => textLineRuns(value, measureText).width;
  const availableWidth = Math.max(1, maxWidth);
  const paragraphs = text.split(/\r?\n/);
  paragraphs.forEach((paragraph) => {
    let line = "";
    const flush = () => {
      lines.push(line);
      line = "";
    };
    let index = 0;
    while (index < paragraph.length) {
      const ch = paragraph[index];
      const isCjk = /[\u3040-\u30ff\u3400-\u4dbf\u4e00-\u9fff\uf900-\ufaff\uac00-\ud7af]/.test(ch);
      const candidate = line ? `${line}${ch}` : ch;
      if (measureLine(candidate) > availableWidth && line !== "") {
        flush();
        continue;
      }
      if (isCjk) {
        line += ch;
      } else {
        const rest = paragraph.slice(index);
        const wordMatch = rest.match(/^\s*\S+/);
        const word = wordMatch ? wordMatch[0] : ch;
        const candidateWord = line
          ? `${line}${word}`
          : index === 0
            ? word
            : word.trimStart();
        if (measureLine(candidateWord) > availableWidth && line) {
          flush();
          continue;
        }
        if (measureLine(candidateWord) > availableWidth) {
          // A single URL or other unbroken Latin run must still stay inside
          // the annotation frame, matching the textarea's break-word style.
          for (const character of candidateWord) {
            if (line && measureLine(`${line}${character}`) > availableWidth) {
              flush();
            }
            line += character;
          }
        } else {
          line = candidateWord;
        }
        index += word.length - 1;
      }
      index += 1;
    }
    flush();
  });
  return lines;
}

export function fitTextEditorFrame(options) {
  const {
    text,
    fontSize,
    x,
    y,
    maxWidth,
    boundsWidth,
    boundsHeight,
    measureText,
    uiScale = 1,
  } = options;
  const safeBoundsWidth = Math.max(1, boundsWidth);
  const safeBoundsHeight = Math.max(1, boundsHeight);
  const widthLimit = Math.max(1, Math.min(maxWidth, safeBoundsWidth));
  const insets = textEditorInsets(uiScale);
  const longestExplicitLine = text
    .split(/\r?\n/)
    .reduce((longest, line) => Math.max(longest, textLineRuns(line, measureText).width), 0);
  const width = Math.min(
    Math.max(Math.min(120 * uiScale, widthLimit), Math.ceil(longestExplicitLine) + 2 * insets.x + 2 * uiScale),
    widthLimit,
  );
  const visualLineCount = layoutTextLines(text, Math.max(1, width - 2 * insets.x), measureText).length;
  const lineHeight = fontSize * 1.25;
  const height = Math.min(
    Math.max(Math.min(34 * uiScale, safeBoundsHeight), Math.ceil(visualLineCount * lineHeight) + 2 * insets.y),
    safeBoundsHeight,
  );
  return {
    x: Math.min(Math.max(0, x), Math.max(0, safeBoundsWidth - width)),
    y: Math.min(Math.max(0, y), Math.max(0, safeBoundsHeight - height)),
    width,
    height,
  };
}
