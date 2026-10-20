// Adapted from Kiri (MIT License), commit 52d2d2f8fd613ce3341cbfae371009f06b123903.
// Copyright (c) 2026 yuxino. See THIRD_PARTY_NOTICES.md.

interface CompositionKey {
  isComposing?: boolean;
  keyCode?: number;
  target?: EventTarget | null;
  nativeEvent?: { isComposing?: boolean; keyCode?: number; target?: EventTarget | null };
}
interface TextKey extends CompositionKey {
  key: string;
  metaKey?: boolean;
  ctrlKey?: boolean;
  shiftKey?: boolean;
  preventDefault(): void;
  stopPropagation(): void;
}
export function setTextComposition(target: EventTarget, active: boolean): void;
export function isTextComposition(event: CompositionKey): boolean;
export function handleTextEditorKey(event: TextKey, actions: {
  cancel(): void; commit(): void; undo(): void; redo(): void; finish?(): void;
  nativeHistory?(command: "undo" | "redo"): boolean;
  multiline?: boolean;
}, nativeUndo?: boolean): void;
