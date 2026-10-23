export interface Point {
  x: number;
  y: number;
}

export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface Size {
  width: number;
  height: number;
}

export function clampPoint(point: Point, width: number, height: number): Point {
  return {
    x: Math.max(0, Math.min(width, point.x)),
    y: Math.max(0, Math.min(height, point.y)),
  };
}

export function normalizeRect(start: Point, end: Point): Rect {
  return {
    x: Math.min(start.x, end.x),
    y: Math.min(start.y, end.y),
    width: Math.abs(end.x - start.x),
    height: Math.abs(end.y - start.y),
  };
}

export function canCompleteSelection(
  selection: Rect | null,
  minimumSide = 4,
): selection is Rect {
  return Boolean(selection && selection.width >= minimumSide && selection.height >= minimumSide);
}

export function positionFloatingToolbar(
  selection: Rect,
  toolbar: Size,
  viewport: Rect,
  gap = 10,
  margin = 12,
): Point {
  const usableWidth = Math.max(0, viewport.width - margin * 2);
  const usableHeight = Math.max(0, viewport.height - margin * 2);
  const width = Math.min(toolbar.width, usableWidth);
  const height = Math.min(toolbar.height, usableHeight);
  const minX = viewport.x + margin;
  const maxX = viewport.x + viewport.width - margin - width;
  const centeredX = selection.x + selection.width / 2 - width / 2;
  const x = Math.max(minX, Math.min(maxX, centeredX));
  const below = selection.y + selection.height + gap;
  const above = selection.y - gap - height;
  const bottomLimit = viewport.y + viewport.height - margin - height;
  let y = below;
  if (below > bottomLimit) {
    y = above >= viewport.y + margin ? above : bottomLimit;
  }
  return {
    x,
    y: Math.max(viewport.y + margin, y),
  };
}

export function mapRectToImage(
  rect: Rect,
  viewportWidth: number,
  viewportHeight: number,
  imageWidth: number,
  imageHeight: number,
): Rect {
  const scaleX = imageWidth / viewportWidth;
  const scaleY = imageHeight / viewportHeight;
  return {
    x: Math.max(0, Math.round(rect.x * scaleX)),
    y: Math.max(0, Math.round(rect.y * scaleY)),
    width: Math.max(1, Math.round(rect.width * scaleX)),
    height: Math.max(1, Math.round(rect.height * scaleY)),
  };
}
import {
  ALL_HANDLES,
  hitTestHandle,
  resized,
  standardized,
  type SelectionHandle,
} from "./upstream/kiri/geom";

export function selectionHandleAt(
  point: Point,
  selection: Rect,
  radius = 9,
): SelectionHandle | null {
  return hitTestHandle(point, selection, radius);
}

export function resizeSelection(
  selection: Rect,
  handle: SelectionHandle,
  point: Point,
  bounds: Rect,
): Rect {
  return resized(selection, handle, point, bounds, 8);
}

export function resizeSelectionProportionally(
  selection: Rect,
  handle: SelectionHandle,
  point: Point,
  bounds: Rect,
): Rect {
  const free = resizeSelection(selection, handle, point, bounds);
  if (!["topLeft", "topRight", "bottomRight", "bottomLeft"].includes(handle)) {
    return free;
  }
  const original = standardized(selection);
  const ratio = original.width / Math.max(1, original.height);
  const anchor = {
    x: handle.includes("Left")
      ? original.x + original.width
      : original.x,
    y: handle.startsWith("top")
      ? original.y + original.height
      : original.y,
  };
  let width = Math.max(8, free.width);
  let height = width / ratio;
  if (height < 8) {
    height = 8;
    width = height * ratio;
  }
  const maxWidth = handle.includes("Left")
    ? anchor.x - bounds.x
    : bounds.x + bounds.width - anchor.x;
  const maxHeight = handle.startsWith("top")
    ? anchor.y - bounds.y
    : bounds.y + bounds.height - anchor.y;
  const fit = Math.min(1, maxWidth / width, maxHeight / height);
  width *= fit;
  height *= fit;
  return {
    x: handle.includes("Left") ? anchor.x - width : anchor.x,
    y: handle.startsWith("top") ? anchor.y - height : anchor.y,
    width,
    height,
  };
}

export function selectionHandlePoints(selection: Rect): Point[] {
  return ALL_HANDLES.map((handle) => {
    switch (handle) {
      case "topLeft":
        return { x: selection.x, y: selection.y };
      case "top":
        return { x: selection.x + selection.width / 2, y: selection.y };
      case "topRight":
        return { x: selection.x + selection.width, y: selection.y };
      case "right":
        return { x: selection.x + selection.width, y: selection.y + selection.height / 2 };
      case "bottomRight":
        return { x: selection.x + selection.width, y: selection.y + selection.height };
      case "bottom":
        return { x: selection.x + selection.width / 2, y: selection.y + selection.height };
      case "bottomLeft":
        return { x: selection.x, y: selection.y + selection.height };
      case "left":
        return { x: selection.x, y: selection.y + selection.height / 2 };
    }
  });
}
