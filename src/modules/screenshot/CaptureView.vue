<script setup lang="ts">
import { listen } from "@tauri-apps/api/event";
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, watch } from "vue";
import UiIcon from "../../components/UiIcon.vue";
import { assetUrl, call } from "../../lib/ipc";
import type { CaptureAction, CaptureRect, CaptureSession } from "../../types";
import {
  canCompleteSelection,
  clampPoint,
  normalizeRect,
  positionFloatingToolbar,
  resizeSelection,
  resizeSelectionProportionally,
  selectionHandleAt,
  snapPoint,
  type Point,
  windowRectAt,
} from "./geometry";
import {
  ALL_HANDLES,
  contains,
  handlePoint,
  hitTestHandle,
  type SelectionHandle,
} from "./upstream/kiri/geom";
import {
  AnnotationHistory,
  COLOR_HEX,
  COLOR_PRESETS,
  DEFAULT_APPEARANCE,
  applyAnnotationAppearance,
  dragAnnotationHandle,
  markIndexAt,
  selectionBounds,
  translateMark,
  type AnnotationMark,
  type AppearanceSettings,
  type ColorPreset,
} from "./upstream/kiri/model";
import { renderAll, type RenderContext } from "./upstream/kiri/render";

type EditorTool =
  | "select"
  | "rectangle"
  | "ellipse"
  | "line"
  | "arrow"
  | "pen"
  | "text"
  | "mosaic";

interface Interaction {
  kind: "new" | "selection-move" | "selection-resize" | "mark-move" | "mark-resize" | "draw";
  start: Point;
  originalSelection?: CaptureRect;
  originalMark?: AnnotationMark;
  originalMarks?: AnnotationMark[];
  markIndex?: number;
  handle?: SelectionHandle;
  draft?: AnnotationMark;
}

const session = ref<CaptureSession | null>(null);
const imageSrc = ref("");
const imageElement = ref<HTMLImageElement | null>(null);
const loading = ref(true);
const error = ref("");
const saving = ref(false);
const selection = ref<CaptureRect | null>(null);
const hoverWindow = ref<CaptureRect | null>(null);
const cursor = reactive({ x: 0, y: 0 });
const tool = ref<EditorTool>("select");
const selectedIndex = ref<number | null>(null);
const marks = ref<AnnotationMark[]>([]);
const annotationCanvas = ref<HTMLCanvasElement | null>(null);
const interaction = ref<Interaction | null>(null);
const draft = ref<AnnotationMark | null>(null);
const canUndo = ref(false);
const canRedo = ref(false);
const textEditor = reactive({
  visible: false,
  x: 0,
  y: 0,
  value: "",
  editingIndex: null as number | null,
});
const textComposing = ref(false);
const toolbarElement = ref<HTMLElement | null>(null);
const toolbarSize = reactive({ width: 0, height: 0 });

const appearance = reactive<AppearanceSettings>({
  ...DEFAULT_APPEARANCE,
  colorPreset: "orange",
  shapeWidth: 3,
  penWidth: 3,
  mosaicShape: "rectangle",
  mosaicStyle: "pixel",
  mosaicIntensity: "standard",
});

const history = new AnnotationHistory();
let nextMarkId = 1;
let activeSessionId: string | null = null;
let loadGeneration = 0;
let unlistenStarted: (() => void) | null = null;
let unlistenEnded: (() => void) | null = null;
let toolbarObserver: ResizeObserver | null = null;

const viewport = computed<CaptureRect>(() => ({
  x: 0,
  y: 0,
  width: window.innerWidth,
  height: window.innerHeight,
}));

const selectedMarkBounds = computed<CaptureRect | null>(() => {
  if (selectedIndex.value == null) return null;
  const mark = marks.value[selectedIndex.value];
  return mark ? selectionBounds(mark) : null;
});

const dimension = computed(() => {
  if (!selection.value || !session.value) return "";
  const scale = session.value.scaleFactor || 1;
  return `${Math.round(selection.value.width * scale)} × ${Math.round(selection.value.height * scale)}`;
});

const magnifierStyle = computed(() => {
  const zoom = 7;
  return {
    left: `${cursor.x + 20}px`,
    top: `${cursor.y + 20}px`,
    backgroundImage: imageSrc.value ? `url(${imageSrc.value})` : "none",
    backgroundSize: `${window.innerWidth * zoom}px ${window.innerHeight * zoom}px`,
    backgroundPosition: `${-cursor.x * zoom + 34}px ${-cursor.y * zoom + 34}px`,
  };
});

const showToolbar = computed(() => {
  const gesture = interaction.value?.kind;
  return (
    canCompleteSelection(selection.value) &&
    gesture != "new" &&
    gesture != "selection-move" &&
    gesture != "selection-resize"
  );
});

const toolbarStyle = computed(() => {
  if (!selection.value) return {};
  const position = positionFloatingToolbar(
    selection.value,
    {
      width: toolbarSize.width || 620,
      height: toolbarSize.height || 44,
    },
    viewport.value,
  );
  return {
    left: `${position.x}px`,
    top: `${position.y}px`,
  };
});

function cloneMark(mark: AnnotationMark): AnnotationMark {
  return JSON.parse(JSON.stringify(mark)) as AnnotationMark;
}

function syncHistory() {
  marks.value = history.elements.slice();
  canUndo.value = history.canUndo;
  canRedo.value = history.canRedo;
  redrawAnnotations();
}

function localPoint(point: { x: number; y: number }): { x: number; y: number } {
  if (!selection.value) return point;
  return {
    x: point.x - selection.value.x,
    y: point.y - selection.value.y,
  };
}

function selectionBoundsRect(): CaptureRect {
  if (!selection.value) return viewport.value;
  return {
    x: 0,
    y: 0,
    width: selection.value.width,
    height: selection.value.height,
  };
}

function pointFromEvent(event: PointerEvent) {
  return clampPoint(
    { x: event.clientX, y: event.clientY },
    window.innerWidth,
    window.innerHeight,
  );
}

function snapSelectionPoint(point: Point): Point {
  const windows = session.value?.windowRects || [];
  const xTargets = [0, window.innerWidth];
  const yTargets = [0, window.innerHeight];
  for (const rect of windows) {
    xTargets.push(rect.x, rect.x + rect.width);
    yTargets.push(rect.y, rect.y + rect.height);
  }
  return snapPoint(point, xTargets, yTargets);
}

function startsNewSelection() {
  selection.value = null;
  selectedIndex.value = null;
  draft.value = null;
}

function createMark(kind: Exclude<EditorTool, "select" | "text">, start: Point): AnnotationMark {
  const id = nextMarkId++;
  const color = appearance.colorPreset;
  const width = kind == "pen" ? appearance.penWidth : appearance.shapeWidth;
  switch (kind) {
    case "rectangle":
      return { kind, id, rect: normalizeRect(start, start), color, width };
    case "ellipse":
      return { kind, id, rect: normalizeRect(start, start), color, width };
    case "line":
      return { kind, id, start, end: { ...start }, color, width };
    case "arrow":
      return { kind, id, start, end: { ...start }, color, width };
    case "pen":
      return { kind, id, points: [{ ...start }], color, width };
    case "mosaic":
      return {
        kind,
        id,
        points: [{ ...start }, { ...start }],
        brushDiameter: appearance.mosaicBrushDiameter,
        intensity: appearance.mosaicIntensity,
        style: appearance.mosaicStyle,
        shape: "rectangle",
      };
  }
}

function updateDraft(mark: AnnotationMark, current: Point, origin: Point): AnnotationMark {
  if (mark.kind == "rectangle" || mark.kind == "ellipse") {
    return { ...mark, rect: normalizeRect(origin, current) };
  }
  if (mark.kind == "line" || mark.kind == "arrow") {
    return { ...mark, end: { ...current } };
  }
  if (mark.kind == "pen") {
    return { ...mark, points: [...mark.points, { ...current }] };
  }
  if (mark.kind == "mosaic") {
    return { ...mark, points: [mark.points[0], { ...current }] };
  }
  return mark;
}

function markIsValid(mark: AnnotationMark): boolean {
  if (mark.kind == "pen") return mark.points.length > 1;
  if (mark.kind == "line" || mark.kind == "arrow") {
    return Math.hypot(mark.end.x - mark.start.x, mark.end.y - mark.start.y) >= 4;
  }
  const bounds = selectionBounds(mark);
  return bounds.width >= 4 && bounds.height >= 4;
}

function onPointerDown(event: PointerEvent) {
  if (saving.value || !session.value) return;
  if ((event.target as HTMLElement).closest(".capture-toolbar, .text-editor, .capture-cancel")) {
    return;
  }
  const point = pointFromEvent(event);
  const local = localPoint(point);

  if (!selection.value) {
    const snapped = snapSelectionPoint(point);
    interaction.value = { kind: "new", start: snapped };
    selection.value = { ...snapped, width: 0, height: 0 };
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    return;
  }

  if (tool.value == "text") {
    if (contains(selection.value, point)) {
      const index = selectedIndex.value;
      const existing = index == null ? null : marks.value[index];
      if (existing?.kind == "text" && index != null) {
        openTextEditor(existing, index);
      } else {
        openTextEditorAt(point);
      }
    }
    return;
  }

  if (tool.value == "select") {
    if (selectedIndex.value != null) {
      const selected = marks.value[selectedIndex.value];
      if (selected) {
        const handle = hitTestHandle(local, selectionBounds(selected), 9);
        if (handle) {
          interaction.value = {
            kind: "mark-resize",
            start: point,
            originalMark: cloneMark(selected),
            markIndex: selectedIndex.value,
            handle,
          };
          (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
          return;
        }
      }
    }

    const hitIndex = markIndexAt(marks.value, local);
    if (hitIndex != null) {
      selectedIndex.value = hitIndex;
      interaction.value = {
        kind: "mark-move",
        start: point,
        originalMark: cloneMark(marks.value[hitIndex]),
        markIndex: hitIndex,
      };
      redrawAnnotations();
      (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
      return;
    }

    const selectionHandle = selectionHandleAt(point, selection.value, 10);
    if (selectionHandle) {
      interaction.value = {
        kind: "selection-resize",
        start: point,
        originalSelection: { ...selection.value },
        handle: selectionHandle,
      };
      (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
      return;
    }

    if (contains(selection.value, point)) {
      selectedIndex.value = null;
      interaction.value = {
        kind: "selection-move",
        start: point,
        originalSelection: { ...selection.value },
      };
      (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
      return;
    }

    startsNewSelection();
    const snapped = snapSelectionPoint(point);
    interaction.value = { kind: "new", start: snapped };
    selection.value = { ...snapped, width: 0, height: 0 };
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    return;
  }

  if (!contains(selection.value, point)) {
    startsNewSelection();
    const snapped = snapSelectionPoint(point);
    interaction.value = { kind: "new", start: snapped };
    selection.value = { ...snapped, width: 0, height: 0 };
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    return;
  }

  const nextDraft = createMark(tool.value, local);
  draft.value = nextDraft;
  interaction.value = { kind: "draw", start: local, draft: nextDraft };
  selectedIndex.value = null;
  (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
  redrawAnnotations();
}

function onPointerMove(event: PointerEvent) {
  const point = pointFromEvent(event);
  cursor.x = point.x;
  cursor.y = point.y;

  if (!interaction.value) {
    hoverWindow.value = selection.value
      ? null
      : windowRectAt(point, session.value?.windowRects || [], viewport.value);
    return;
  }

  const active = interaction.value;
  if (active.kind == "new") {
    selection.value = normalizeRect(active.start, snapSelectionPoint(point));
    return;
  }
  if (active.kind == "selection-move" && active.originalSelection) {
    const width = active.originalSelection.width;
    const height = active.originalSelection.height;
    selection.value = {
      x: Math.max(0, Math.min(window.innerWidth - width, active.originalSelection.x + point.x - active.start.x)),
      y: Math.max(0, Math.min(window.innerHeight - height, active.originalSelection.y + point.y - active.start.y)),
      width,
      height,
    };
    nextTick(redrawAnnotations);
    return;
  }
  if (active.kind == "selection-resize" && active.originalSelection && active.handle) {
    const resize = event.shiftKey ? resizeSelectionProportionally : resizeSelection;
    selection.value = resize(active.originalSelection, active.handle, snapSelectionPoint(point), viewport.value);
    nextTick(redrawAnnotations);
    return;
  }
  if (!selection.value) return;

  const local = localPoint(point);
  if (active.kind == "draw" && draft.value) {
    draft.value = updateDraft(draft.value, local, active.start);
    redrawAnnotations();
    return;
  }
  if (active.kind == "mark-move" && active.originalMark && active.markIndex != null) {
    const mark = translateMark(
      active.originalMark,
      { x: point.x - active.start.x, y: point.y - active.start.y },
      selectionBoundsRect(),
    );
    marks.value[active.markIndex] = mark;
    redrawAnnotations();
    return;
  }
  if (active.kind == "mark-resize" && active.originalMark && active.markIndex != null && active.handle) {
    const mark = dragAnnotationHandle(
      active.originalMark,
      active.handle,
      { x: point.x - active.start.x, y: point.y - active.start.y },
      selectionBoundsRect(),
    );
    marks.value[active.markIndex] = mark;
    redrawAnnotations();
  }
}

function onPointerUp(event: PointerEvent) {
  const active = interaction.value;
  interaction.value = null;
  if (!active) return;

  if (active.kind == "new") {
    if (!selection.value || selection.value.width < 8 || selection.value.height < 8) {
      if (hoverWindow.value) {
        selection.value = { ...hoverWindow.value };
      } else {
        selection.value = null;
      }
    }
    hoverWindow.value = null;
    selectedIndex.value = null;
    nextTick(redrawAnnotations);
    return;
  }

  if (active.kind == "draw" && draft.value) {
    if (markIsValid(draft.value)) {
      history.append(draft.value);
      selectedIndex.value = history.elements.length - 1;
      syncHistory();
    }
    draft.value = null;
    return;
  }

  if ((active.kind == "mark-move" || active.kind == "mark-resize") && active.markIndex != null) {
    const current = marks.value[active.markIndex];
    if (current && active.originalMark) {
      marks.value[active.markIndex] = active.originalMark;
      history.replace(active.markIndex, current);
      syncHistory();
    }
  }

  try {
    (event.currentTarget as HTMLElement).releasePointerCapture(event.pointerId);
  } catch {
    // pointer capture may already have been released by the host webview.
  }
}

function openTextEditorAt(point: Point) {
  textEditor.visible = true;
  textEditor.x = point.x;
  textEditor.y = point.y;
  textEditor.value = "";
  textEditor.editingIndex = null;
  nextTick(() => document.querySelector<HTMLInputElement>(".capture-text-input")?.focus());
}

function openTextEditor(mark: Extract<AnnotationMark, { kind: "text" }>, index: number) {
  if (!selection.value) return;
  textEditor.visible = true;
  textEditor.x = selection.value.x + mark.rect.x;
  textEditor.y = selection.value.y + mark.rect.y;
  textEditor.value = mark.text;
  textEditor.editingIndex = index;
  nextTick(() => document.querySelector<HTMLInputElement>(".capture-text-input")?.focus());
}

function commitText() {
  if (!selection.value) return;
  const text = textEditor.value.trim();
  if (!text) {
    textEditor.visible = false;
    return;
  }
  const canvas = document.createElement("canvas");
  const context = canvas.getContext("2d");
  if (!context) return;
  const fontSize = appearance.textFontSize;
  context.font = `600 ${fontSize}px -apple-system, "Segoe UI", "PingFang SC", sans-serif`;
  const lines = wrapText(context, text, 260);
  const width = Math.max(...lines.map((line) => context.measureText(line).width), 20);
  const height = Math.max(1, lines.length) * fontSize * 1.35;
  const mark: AnnotationMark = {
    kind: "text",
    id: textEditor.editingIndex == null ? nextMarkId++ : marks.value[textEditor.editingIndex]?.id || nextMarkId++,
    text,
    rect: {
      x: textEditor.x - selection.value.x,
      y: textEditor.y - selection.value.y,
      width: width + 8,
      height,
    },
    color: appearance.colorPreset,
    background: "transparent",
    fontSize,
  };
  if (textEditor.editingIndex == null) {
    history.append(mark);
    selectedIndex.value = history.elements.length - 1;
  } else {
    history.replace(textEditor.editingIndex, mark);
    selectedIndex.value = textEditor.editingIndex;
  }
  textEditor.visible = false;
  syncHistory();
}

function onTextEditorKeydown(event: KeyboardEvent) {
  if (event.isComposing || textComposing.value) return;
  if (event.key == "Enter") {
    event.preventDefault();
    commitText();
    return;
  }
  if (event.key == "Escape") {
    event.preventDefault();
    textEditor.visible = false;
  }
}

function wrapText(context: CanvasRenderingContext2D, text: string, maxWidth: number): string[] {
  const result: string[] = [];
  for (const paragraph of text.split("\n")) {
    let line = "";
    for (const character of paragraph || " ") {
      const candidate = line + character;
      if (line && context.measureText(candidate).width > maxWidth) {
        result.push(line);
        line = character;
      } else {
        line = candidate;
      }
    }
    result.push(line || " ");
  }
  return result;
}

function redrawAnnotations() {
  const canvas = annotationCanvas.value;
  const image = imageElement.value;
  if (!canvas || !selection.value || !image) return;
  const dpr = window.devicePixelRatio || 1;
  const width = Math.max(1, selection.value.width);
  const height = Math.max(1, selection.value.height);
  if (canvas.width != Math.round(width * dpr) || canvas.height != Math.round(height * dpr)) {
    canvas.width = Math.round(width * dpr);
    canvas.height = Math.round(height * dpr);
  }
  const context = canvas.getContext("2d");
  if (!context) return;
  context.setTransform(dpr, 0, 0, dpr, 0, 0);
  const renderContext: RenderContext = {
    ctx: context,
    sourceImage: image,
    sourceWidth: image.naturalWidth,
    sourceHeight: image.naturalHeight,
    sourceOffset: { x: selection.value.x, y: selection.value.y },
    regionSize: { x: 0, y: 0, width, height },
    scaleX: image.naturalWidth / window.innerWidth,
    scaleY: image.naturalHeight / window.innerHeight,
    viewScaleX: 1,
    viewScaleY: 1,
    exporting: false,
  };
  renderAll(renderContext, marks.value, { draft: draft.value });
}

function undo() {
  history.undo();
  selectedIndex.value = null;
  syncHistory();
}

function redo() {
  history.redo();
  selectedIndex.value = null;
  syncHistory();
}

function deleteSelected() {
  if (selectedIndex.value == null) return;
  history.remove(selectedIndex.value);
  selectedIndex.value = null;
  syncHistory();
}

function setTool(next: EditorTool) {
  tool.value = next;
  if (next != "select") selectedIndex.value = null;
  redrawAnnotations();
}

function setColor(color: ColorPreset) {
  appearance.colorPreset = color;
  applyAppearanceToSelected({ colorPreset: color });
}

function applyAppearanceToSelected(patch: Partial<AppearanceSettings>) {
  if (selectedIndex.value != null) {
    const mark = marks.value[selectedIndex.value];
    if (mark) {
      history.replace(selectedIndex.value, applyAnnotationAppearance(mark, patch));
      syncHistory();
    }
  }
}

function setStrokeWidth(value: number) {
  if (tool.value == "pen") appearance.penWidth = value;
  else appearance.shapeWidth = value;
  applyAppearanceToSelected({ penWidth: value, shapeWidth: value });
}

function setStrokeWidthFromInput(event: Event) {
  setStrokeWidth(Number((event.target as HTMLInputElement).value));
}

function setMosaicStyle(style: "pixel" | "blur") {
  appearance.mosaicStyle = style;
  applyAppearanceToSelected({ mosaicStyle: style });
}

function setMosaicIntensity(intensity: "soft" | "standard" | "strong") {
  appearance.mosaicIntensity = intensity;
  applyAppearanceToSelected({ mosaicIntensity: intensity });
}

function setMosaicIntensityFromSelect(event: Event) {
  const value = (event.target as HTMLSelectElement).value;
  setMosaicIntensity(value as "soft" | "standard" | "strong");
}

function onKeydown(event: KeyboardEvent) {
  if (textEditor.visible) return;
  if (event.key == "Escape") {
    event.preventDefault();
    void cancel();
    return;
  }
  if (event.key == "Enter" && selection.value) {
    event.preventDefault();
    void complete("copy");
    return;
  }
  if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() == "s" && selection.value) {
    event.preventDefault();
    void complete("save");
    return;
  }
  if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() == "z") {
    event.preventDefault();
    if (event.shiftKey) redo();
    else undo();
    return;
  }
  if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() == "c" && selection.value) {
    if (window.getSelection()?.toString()) return;
    event.preventDefault();
    void complete("copy");
  }
  if ((event.key == "Backspace" || event.key == "Delete") && selectedIndex.value != null) {
    event.preventDefault();
    deleteSelected();
  }
}

function onDoubleClick(event: MouseEvent) {
  if (!canCompleteSelection(selection.value)) return;
  const point = clampPoint({ x: event.clientX, y: event.clientY }, window.innerWidth, window.innerHeight);
  if (!contains(selection.value, point)) return;
  if (tool.value == "select") {
    const local = localPoint(point);
    const hit = markIndexAt(marks.value, local);
    if (hit != null) {
      const mark = marks.value[hit];
      if (mark.kind == "text") {
        selectedIndex.value = hit;
        openTextEditor(mark, hit);
        return;
      }
    }
  }
  void complete("copy");
}

function exportDataUrl(): string {
  if (!session.value || !canCompleteSelection(selection.value) || !imageElement.value) {
    throw new Error("截图尚未准备完成");
  }
  const image = imageElement.value;
  const scaleX = image.naturalWidth / window.innerWidth;
  const scaleY = image.naturalHeight / window.innerHeight;
  const sel = selection.value;
  const output = document.createElement("canvas");
  output.width = Math.max(1, Math.round(sel.width * scaleX));
  output.height = Math.max(1, Math.round(sel.height * scaleY));
  const context = output.getContext("2d");
  if (!context) throw new Error("无法创建导出画布");
  const renderContext: RenderContext = {
    ctx: context,
    sourceImage: image,
    sourceWidth: image.naturalWidth,
    sourceHeight: image.naturalHeight,
    sourceOffset: { x: sel.x, y: sel.y },
    regionSize: { x: 0, y: 0, width: sel.width, height: sel.height },
    scaleX,
    scaleY,
    viewScaleX: 1,
    viewScaleY: 1,
    exporting: true,
  };
  renderAll(renderContext, marks.value);
  return output.toDataURL("image/png");
}

async function complete(action: CaptureAction) {
  if (!canCompleteSelection(selection.value) || saving.value) return;
  const sessionId = session.value?.sessionId;
  if (!sessionId) return;
  saving.value = true;
  error.value = "";
  try {
    let savePath: string | null = null;
    if (action == "save") {
      savePath = await call<string | null>("choose_capture_save_path");
      if (!savePath) {
        saving.value = false;
        return;
      }
    }
    const dataUrl = exportDataUrl();
    await call("complete_capture", {
      request: {
        sessionId,
        dataUrl,
        action,
        savePath,
      },
    });
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : String(reason);
    saving.value = false;
  }
}

async function cancel() {
  if (saving.value) return;
  const sessionId = session.value?.sessionId;
  if (!sessionId) return;
  saving.value = true;
  await call("cancel_capture", { sessionId }).catch(() => undefined);
}

function handleStyle(handle: SelectionHandle) {
  const positions: Record<SelectionHandle, string> = {
    topLeft: "handle-top-left",
    top: "handle-top",
    topRight: "handle-top-right",
    right: "handle-right",
    bottomRight: "handle-bottom-right",
    bottom: "handle-bottom",
    bottomLeft: "handle-bottom-left",
    left: "handle-left",
  };
  return positions[handle];
}

function clearImageSource() {
  if (imageSrc.value.startsWith("blob:")) {
    URL.revokeObjectURL(imageSrc.value);
  }
  imageSrc.value = "";
}

function resetEditorState() {
  clearImageSource();
  selection.value = null;
  hoverWindow.value = null;
  selectedIndex.value = null;
  interaction.value = null;
  draft.value = null;
  history.clear();
  marks.value = [];
  canUndo.value = false;
  canRedo.value = false;
  textEditor.visible = false;
  textEditor.value = "";
  textEditor.editingIndex = null;
  textComposing.value = false;
  tool.value = "select";
  nextMarkId = 1;
  loading.value = true;
  error.value = "";
  saving.value = false;
  toolbarSize.width = 0;
  toolbarSize.height = 0;
}

function resetCaptureView() {
  loadGeneration += 1;
  activeSessionId = null;
  session.value = null;
  resetEditorState();
}

async function loadCaptureSession(nextSession: CaptureSession) {
  if (!nextSession.sessionId || activeSessionId == nextSession.sessionId) return;
  activeSessionId = nextSession.sessionId;
  const generation = ++loadGeneration;
  resetEditorState();
  session.value = nextSession;
  let objectUrl = "";
  try {
    let source = assetUrl(nextSession.imagePath);
    if (!source) throw new Error("截图图片路径无效");
    if (generation != loadGeneration || activeSessionId != nextSession.sessionId) {
      if (objectUrl) URL.revokeObjectURL(objectUrl);
      return;
    }

    try {
      const probe = new Image();
      probe.src = source;
      await probe.decode();
    } catch {
      const bytes = await call<ArrayBuffer>("get_capture_image_bytes", {
        sessionId: nextSession.sessionId,
      });
      objectUrl = URL.createObjectURL(new Blob([bytes], { type: "image/png" }));
      source = objectUrl;
      const fallbackProbe = new Image();
      fallbackProbe.src = source;
      await fallbackProbe.decode();
    }
    if (generation != loadGeneration || activeSessionId != nextSession.sessionId) {
      if (objectUrl) URL.revokeObjectURL(objectUrl);
      return;
    }

    imageSrc.value = source;
    loading.value = false;
    await nextTick();
    if (imageElement.value) {
      await imageElement.value.decode().catch(() => undefined);
    }
    if (generation != loadGeneration || activeSessionId != nextSession.sessionId) {
      if (objectUrl && imageSrc.value != objectUrl) URL.revokeObjectURL(objectUrl);
      return;
    }
    await call("show_capture_window", { sessionId: nextSession.sessionId });
  } catch (reason) {
    if (generation != loadGeneration || activeSessionId != nextSession.sessionId) {
      if (objectUrl && imageSrc.value != objectUrl) URL.revokeObjectURL(objectUrl);
      return;
    }
    if (objectUrl && imageSrc.value != objectUrl) URL.revokeObjectURL(objectUrl);
    error.value = reason instanceof Error ? reason.message : String(reason);
    loading.value = false;
    await nextTick();
    await call("show_capture_window", { sessionId: nextSession.sessionId }).catch(() => undefined);
  }
}

function updateToolbarSize() {
  const element = toolbarElement.value;
  if (!element) return;
  const bounds = element.getBoundingClientRect();
  toolbarSize.width = bounds.width;
  toolbarSize.height = bounds.height;
}

watch(showToolbar, async (visible) => {
  toolbarObserver?.disconnect();
  if (!visible) {
    toolbarSize.width = 0;
    toolbarSize.height = 0;
    return;
  }
  await nextTick();
  const element = toolbarElement.value;
  if (!element || !toolbarObserver) return;
  toolbarObserver.observe(element);
  updateToolbarSize();
});

onMounted(async () => {
  window.addEventListener("keydown", onKeydown);
  toolbarObserver =
    typeof ResizeObserver == "undefined" ? null : new ResizeObserver(updateToolbarSize);
  try {
    unlistenStarted = await listen<CaptureSession>("capture://started", (event) => {
      void loadCaptureSession(event.payload);
    });
    unlistenEnded = await listen("capture://ended", () => {
      resetCaptureView();
    });
    const active = await call<CaptureSession | null>("get_active_capture_session");
    if (active) {
      void loadCaptureSession(active);
    }
  } catch (reason) {
    error.value = reason instanceof Error ? reason.message : String(reason);
    loading.value = false;
  }
});

onBeforeUnmount(() => {
  loadGeneration += 1;
  activeSessionId = null;
  window.removeEventListener("keydown", onKeydown);
  unlistenStarted?.();
  unlistenEnded?.();
  toolbarObserver?.disconnect();
  clearImageSource();
});
</script>

<template>
  <main
    class="capture-root"
    @contextmenu.prevent
    @pointerdown="onPointerDown"
    @pointermove="onPointerMove"
    @pointerup="onPointerUp"
    @pointercancel="onPointerUp"
    @dblclick="onDoubleClick"
  >
    <img
      v-if="imageSrc"
      ref="imageElement"
      class="capture-image"
      :src="imageSrc"
      alt=""
      draggable="false"
    />

    <div
      v-if="hoverWindow"
      class="hover-window"
      :style="{
        left: `${hoverWindow.x}px`,
        top: `${hoverWindow.y}px`,
        width: `${hoverWindow.width}px`,
        height: `${hoverWindow.height}px`,
      }"
    ></div>

    <div
      v-if="selection"
      class="capture-shade"
      :style="{
        left: `${selection.x}px`,
        top: `${selection.y}px`,
        width: `${selection.width}px`,
        height: `${selection.height}px`,
      }"
    ></div>

    <div
      v-if="selection"
      class="annotation-host"
      :style="{
        left: `${selection.x}px`,
        top: `${selection.y}px`,
        width: `${selection.width}px`,
        height: `${selection.height}px`,
      }"
    >
      <canvas ref="annotationCanvas" class="annotation-canvas"></canvas>
      <div
        v-if="selectedMarkBounds"
        class="mark-selection"
        :style="{
          left: `${selectedMarkBounds.x}px`,
          top: `${selectedMarkBounds.y}px`,
          width: `${selectedMarkBounds.width}px`,
          height: `${selectedMarkBounds.height}px`,
        }"
      >
        <span
          v-for="handle in ALL_HANDLES"
          :key="handle"
          :class="['selection-handle', handleStyle(handle)]"
        ></span>
      </div>
    </div>

    <div
      v-if="selection"
      class="selection-border"
      :style="{
        left: `${selection.x}px`,
        top: `${selection.y}px`,
        width: `${selection.width}px`,
        height: `${selection.height}px`,
      }"
    >
      <span
        v-for="handle in ALL_HANDLES"
        :key="handle"
        :class="['selection-handle', handleStyle(handle)]"
      ></span>
      <span class="dimension-label">{{ dimension }}</span>
    </div>

    <div v-if="selection && !interaction" class="magnifier" :style="magnifierStyle">
      <div class="crosshair-h"></div>
      <div class="crosshair-v"></div>
      <span>{{ Math.round(cursor.x * (session?.scaleFactor || 1)) }}, {{ Math.round(cursor.y * (session?.scaleFactor || 1)) }}</span>
    </div>

    <div
      v-if="textEditor.visible"
      class="text-editor"
      :style="{ left: `${textEditor.x}px`, top: `${textEditor.y}px` }"
      @pointerdown.stop
    >
      <input
        v-model="textEditor.value"
        class="capture-text-input"
        :style="{ color: COLOR_HEX[appearance.colorPreset] }"
        @keydown="onTextEditorKeydown"
        @compositionstart="textComposing = true"
        @compositionend="textComposing = false"
      />
      <button class="primary-button" @click="commitText">确定</button>
    </div>

    <div
      v-if="showToolbar"
      ref="toolbarElement"
      class="capture-toolbar"
      :style="toolbarStyle"
      @pointerdown.stop
    >
      <div class="tool-group">
        <button :class="{ active: tool == 'select' }" title="选择/移动" @click="setTool('select')">
          <UiIcon name="screenshot" />
        </button>
        <button :class="{ active: tool == 'rectangle' }" title="矩形" @click="setTool('rectangle')">
          <span class="tool-shape rect"></span>
        </button>
        <button :class="{ active: tool == 'ellipse' }" title="椭圆" @click="setTool('ellipse')">
          <span class="tool-shape ellipse"></span>
        </button>
        <button :class="{ active: tool == 'line' }" title="直线" @click="setTool('line')">
          <span class="tool-line"></span>
        </button>
        <button :class="{ active: tool == 'arrow' }" title="箭头" @click="setTool('arrow')">
          <UiIcon name="arrow" />
        </button>
        <button :class="{ active: tool == 'pen' }" title="画笔" @click="setTool('pen')">
          <UiIcon name="edit" />
        </button>
        <button :class="{ active: tool == 'text' }" title="文字" @click="setTool('text')">
          <strong>Aa</strong>
        </button>
        <button :class="{ active: tool == 'mosaic' }" title="马赛克" @click="setTool('mosaic')">
          <span class="mosaic-icon"></span>
        </button>
      </div>

      <div class="tool-divider"></div>
      <div class="color-presets">
        <button
          v-for="preset in COLOR_PRESETS.slice(0, 6)"
          :key="preset"
          :class="{ active: appearance.colorPreset == preset }"
          :style="{ background: COLOR_HEX[preset] }"
          :title="preset"
          @click="setColor(preset)"
        ></button>
      </div>

      <label v-if="tool == 'pen' || tool == 'rectangle' || tool == 'ellipse' || tool == 'line' || tool == 'arrow'" class="width-control">
        <input
          :value="tool == 'pen' ? appearance.penWidth : appearance.shapeWidth"
          type="range"
          min="1"
          max="10"
          @input="setStrokeWidthFromInput"
        />
        <span>{{ tool == 'pen' ? appearance.penWidth : appearance.shapeWidth }}px</span>
      </label>

      <template v-if="tool == 'mosaic'">
        <div class="tool-divider"></div>
        <button
          :class="{ active: appearance.mosaicStyle == 'pixel' }"
          title="像素马赛克"
          @click="setMosaicStyle('pixel')"
        >
          <span class="mosaic-icon"></span>
        </button>
        <button
          :class="{ active: appearance.mosaicStyle == 'blur' }"
          title="模糊"
          @click="setMosaicStyle('blur')"
        >
          <span class="blur-icon"></span>
        </button>
        <select
          :value="appearance.mosaicIntensity"
          class="toolbar-select"
          @change="setMosaicIntensityFromSelect"
        >
          <option value="soft">弱</option>
          <option value="standard">中</option>
          <option value="strong">强</option>
        </select>
      </template>

      <div class="tool-divider"></div>
      <button title="撤销" :disabled="!canUndo" @click="undo">
        <span class="undo-mark">↶</span>
      </button>
      <button title="重做" :disabled="!canRedo" @click="redo">
        <span class="undo-mark">↷</span>
      </button>
      <button title="删除选中标注" :disabled="selectedIndex == null" @click="deleteSelected">
        <UiIcon name="trash" :size="15" />
      </button>

      <div class="tool-divider"></div>
      <button title="取消截图" @click="cancel">
        <UiIcon name="close" />
      </button>
      <button class="tool-action secondary" :disabled="saving" @click="complete('save')">
        <UiIcon name="save" />
        保存
      </button>
      <button class="tool-action secondary" :disabled="saving" @click="complete('pin')">
        <UiIcon name="pin" />
        贴图
      </button>
      <button class="tool-action primary" :disabled="saving" @click="complete('copy')">
        <span v-if="saving" class="spinner"></span>
        <UiIcon v-else name="copy" />
        复制
      </button>
    </div>

    <button
      v-if="!loading && !selection"
      type="button"
      class="capture-cancel"
      @pointerdown.stop
      @click.stop="cancel"
    >
      <UiIcon name="close" :size="15" />
      取消截图
    </button>

    <div v-if="loading" class="capture-center">
      <span class="spinner"></span>
      正在准备截图…
    </div>
    <div v-else-if="error && !imageSrc" class="capture-center capture-error">
      <UiIcon name="alert" :size="26" />
      <span>{{ error }}</span>
      <button class="secondary-button" @click="cancel">关闭</button>
    </div>
    <div v-if="error && imageSrc" class="capture-toast">{{ error }}</div>
    <div v-if="!selection && !loading && imageSrc" class="capture-hint">
      拖动框选区域，单击选择窗口
      <kbd>Esc</kbd>
      退出
    </div>
  </main>
</template>

<style scoped>
.capture-root {
  position: relative;
  width: 100%;
  height: 100%;
  overflow: hidden;
  background: #0b0c0e;
  cursor: crosshair;
  user-select: none;
}

.capture-image {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  object-fit: fill;
  pointer-events: none;
}

.hover-window {
  position: absolute;
  z-index: 2;
  border: 2px solid rgba(247, 147, 30, 0.95);
  border-radius: 2px;
  box-shadow:
    0 0 0 1px rgba(0, 0, 0, 0.5),
    0 0 20px rgba(247, 147, 30, 0.2);
  pointer-events: none;
}

.capture-shade {
  position: absolute;
  z-index: 3;
  border: 1px solid rgba(247, 147, 30, 0.9);
  box-shadow: 0 0 0 9999px rgba(5, 6, 8, 0.52);
  pointer-events: none;
}

.annotation-host {
  position: absolute;
  z-index: 4;
  pointer-events: none;
}

.annotation-canvas {
  width: 100%;
  height: 100%;
  pointer-events: none;
}

.selection-border {
  position: absolute;
  z-index: 7;
  border: 1px solid #f7931e;
  pointer-events: none;
}

.mark-selection {
  position: absolute;
  z-index: 1;
  border: 1px dashed rgba(255, 255, 255, 0.9);
  pointer-events: none;
}

.selection-handle {
  position: absolute;
  width: 7px;
  height: 7px;
  border: 1px solid #1b1409;
  border-radius: 2px;
  background: #f7931e;
}

.handle-top-left {
  top: -4px;
  left: -4px;
}

.handle-top {
  top: -4px;
  left: 50%;
  transform: translateX(-50%);
}

.handle-top-right {
  top: -4px;
  right: -4px;
}

.handle-right {
  top: 50%;
  right: -4px;
  transform: translateY(-50%);
}

.handle-bottom-right {
  right: -4px;
  bottom: -4px;
}

.handle-bottom {
  bottom: -4px;
  left: 50%;
  transform: translateX(-50%);
}

.handle-bottom-left {
  bottom: -4px;
  left: -4px;
}

.handle-left {
  top: 50%;
  left: -4px;
  transform: translateY(-50%);
}

.dimension-label {
  position: absolute;
  top: -28px;
  left: -1px;
  padding: 4px 7px;
  border-radius: 5px;
  background: #17181b;
  color: #f3f0e9;
  font-size: 10px;
  white-space: nowrap;
}

.magnifier {
  position: fixed;
  z-index: 20;
  width: 68px;
  height: 68px;
  border: 1px solid rgba(255, 255, 255, 0.65);
  border-radius: 12px;
  background-repeat: no-repeat;
  box-shadow: 0 12px 35px rgba(0, 0, 0, 0.45);
  pointer-events: none;
}

.magnifier > span {
  position: absolute;
  top: 71px;
  left: 0;
  width: 100%;
  color: rgba(255, 255, 255, 0.8);
  font-size: 8px;
  text-align: center;
  white-space: nowrap;
}

.crosshair-h,
.crosshair-v {
  position: absolute;
  background: rgba(247, 147, 30, 0.9);
}

.crosshair-h {
  top: 50%;
  right: 0;
  left: 0;
  height: 1px;
}

.crosshair-v {
  top: 0;
  bottom: 0;
  left: 50%;
  width: 1px;
}

.capture-toolbar {
  position: fixed;
  z-index: 30;
  display: flex;
  align-items: center;
  gap: 4px;
  max-width: calc(100vw - 24px);
  padding: 6px;
  border: 1px solid rgba(255, 255, 255, 0.14);
  border-radius: 13px;
  background: #1b1c20;
  box-shadow: 0 18px 55px rgba(0, 0, 0, 0.5);
  overflow-x: auto;
  cursor: default;
}

.tool-group {
  display: flex;
  gap: 2px;
}

.capture-toolbar button {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 4px;
  width: 31px;
  height: 31px;
  padding: 0;
  border: 0;
  border-radius: 7px;
  background: transparent;
  color: #a8a7a2;
  font-size: 9px;
  cursor: pointer;
}

.capture-toolbar button,
.capture-toolbar .tool-divider,
.capture-toolbar .color-presets,
.capture-toolbar .width-control {
  flex-shrink: 0;
}

.capture-toolbar button:hover:not(:disabled) {
  background: #2b2c31;
  color: #f3f0e9;
}

.capture-toolbar button.active {
  background: rgba(247, 147, 30, 0.18);
  color: #ffb340;
}

.tool-shape {
  display: block;
  width: 14px;
  height: 12px;
  border: 1.5px solid currentColor;
}

.tool-shape.ellipse {
  border-radius: 50%;
}

.tool-line {
  width: 15px;
  height: 1.5px;
  background: currentColor;
  transform: rotate(-35deg);
}

.mosaic-icon {
  width: 13px;
  height: 13px;
  background:
    linear-gradient(90deg, currentColor 30%, transparent 30% 65%, currentColor 65%),
    linear-gradient(currentColor 30%, transparent 30% 65%, currentColor 65%);
  opacity: 0.8;
}

.blur-icon {
  width: 13px;
  height: 13px;
  border-radius: 50%;
  background: currentColor;
  filter: blur(2px);
  opacity: 0.65;
}

.tool-divider {
  width: 1px;
  height: 22px;
  margin: 0 3px;
  background: rgba(255, 255, 255, 0.1);
}

.color-presets {
  display: flex;
  gap: 4px;
  padding: 0 3px;
}

.color-presets button {
  width: 13px;
  height: 13px;
  border: 1px solid rgba(255, 255, 255, 0.5);
  border-radius: 50%;
}

.color-presets button.active {
  box-shadow: 0 0 0 2px #1b1c20, 0 0 0 3px #f7931e;
}

.width-control {
  display: flex;
  align-items: center;
  gap: 5px;
  padding: 0 4px;
  color: #8e8d88;
  font-size: 8px;
}

.width-control input {
  width: 52px;
  accent-color: #f7931e;
}

.toolbar-select {
  height: 25px;
  border: 1px solid rgba(255, 255, 255, 0.12);
  border-radius: 6px;
  background: #24252a;
  color: #ddd9d1;
  font-size: 8px;
}

.undo-mark {
  font-size: 19px;
  line-height: 1;
}

.capture-toolbar .tool-action {
  width: auto;
  padding: 0 10px;
  font-size: 9px;
  font-weight: 650;
}

.capture-toolbar .tool-action.secondary {
  background: #28292e;
  color: #dad7d0;
}

.capture-toolbar .tool-action.primary {
  background: #f7931e;
  color: #1c1307;
}

.text-editor {
  position: fixed;
  z-index: 35;
  display: flex;
  gap: 6px;
  padding: 5px;
  border: 1px solid rgba(255, 255, 255, 0.2);
  border-radius: 9px;
  background: #1b1c20;
  box-shadow: 0 12px 36px rgba(0, 0, 0, 0.4);
}

.capture-text-input {
  width: 190px;
  padding: 7px 8px;
  border: 0;
  border-radius: 6px;
  outline: 0;
  background: rgba(255, 255, 255, 0.08);
}

.capture-cancel {
  position: fixed;
  z-index: 40;
  top: 18px;
  left: 18px;
  display: flex;
  align-items: center;
  gap: 6px;
  height: 34px;
  padding: 0 12px;
  border: 1px solid rgba(255, 255, 255, 0.18);
  border-radius: 9px;
  background: rgba(20, 21, 24, 0.9);
  color: rgba(255, 255, 255, 0.84);
  font-size: 10px;
  cursor: pointer;
  backdrop-filter: blur(8px);
}

.capture-center {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 10px;
  width: 100%;
  height: 100%;
  color: rgba(255, 255, 255, 0.65);
  font-size: 11px;
}

.capture-error {
  flex-direction: column;
}

.capture-hint,
.capture-toast {
  position: fixed;
  z-index: 25;
  left: 50%;
  color: rgba(255, 255, 255, 0.85);
  transform: translateX(-50%);
  pointer-events: none;
}

.capture-hint {
  bottom: 32px;
  display: flex;
  align-items: center;
  gap: 7px;
  padding: 8px 12px;
  border: 1px solid rgba(255, 255, 255, 0.13);
  border-radius: 999px;
  background: rgba(20, 21, 24, 0.88);
  font-size: 10px;
}

.capture-hint kbd {
  padding: 2px 5px;
  border: 1px solid rgba(255, 255, 255, 0.18);
  border-radius: 4px;
  font-size: 8px;
}

.capture-toast {
  top: 18px;
  padding: 8px 12px;
  border-radius: 8px;
  background: rgba(255, 107, 104, 0.9);
  color: white;
  font-size: 10px;
}
</style>
