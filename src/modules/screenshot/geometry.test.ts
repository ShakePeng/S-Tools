import { describe, expect, it } from "vitest";
import {
  canCompleteSelection,
  clampPoint,
  hasPointerMoved,
  mapRectToImage,
  normalizeRect,
  positionFloatingToolbar,
  resizeSelection,
  resizeSelectionProportionally,
  selectionHandleAt,
} from "./geometry";

describe("screenshot geometry", () => {
  it("clamps cursor points to the viewport", () => {
    expect(clampPoint({ x: -5, y: 20 }, 100, 80)).toEqual({ x: 0, y: 20 });
    expect(clampPoint({ x: 120, y: 100 }, 100, 80)).toEqual({ x: 100, y: 80 });
  });

  it("normalizes reverse drag direction", () => {
    expect(normalizeRect({ x: 80, y: 60 }, { x: 20, y: 10 })).toEqual({
      x: 20,
      y: 10,
      width: 60,
      height: 50,
    });
  });

  it("ignores click-sized jitter when deciding whether a drag started", () => {
    expect(hasPointerMoved({ x: 20, y: 20 }, { x: 22, y: 21 })).toBe(false);
    expect(hasPointerMoved({ x: 20, y: 20 }, { x: 23, y: 20 })).toBe(true);
  });

  it("maps logical CSS pixels into retina image pixels", () => {
    expect(mapRectToImage({ x: 10, y: 20, width: 100, height: 50 }, 1000, 500, 2000, 1000)).toEqual({
      x: 20,
      y: 40,
      width: 200,
      height: 100,
    });
  });

  it("detects and resizes eight-handle selections", () => {
    const selection = { x: 20, y: 20, width: 100, height: 80 };
    expect(selectionHandleAt({ x: 20, y: 20 }, selection)).toBe("topLeft");
    expect(
      resizeSelection(
        selection,
        "bottomRight",
        { x: 160, y: 140 },
        { x: 0, y: 0, width: 200, height: 160 },
      ),
    ).toEqual({ x: 20, y: 20, width: 140, height: 120 });
  });

  it("keeps every resize handle inside the display bounds", () => {
    const selection = { x: 20, y: 20, width: 100, height: 80 };
    const bounds = { x: 0, y: 0, width: 200, height: 160 };
    expect(resizeSelection(selection, "topLeft", { x: -20, y: -20 }, bounds)).toEqual({
      x: 0,
      y: 0,
      width: 120,
      height: 100,
    });
    expect(resizeSelection(selection, "left", { x: 118, y: 40 }, bounds)).toEqual({
      x: 112,
      y: 20,
      width: 8,
      height: 80,
    });
    expect(resizeSelection(selection, "bottom", { x: 60, y: 190 }, bounds)).toEqual({
      x: 20,
      y: 20,
      width: 100,
      height: 140,
    });
  });

  it("only completes a usable selection", () => {
    expect(canCompleteSelection(null)).toBe(false);
    expect(canCompleteSelection({ x: 0, y: 0, width: 3, height: 20 })).toBe(false);
    expect(canCompleteSelection({ x: 0, y: 0, width: 8, height: 8 })).toBe(true);
  });

  it("keeps corner aspect ratio when proportional resize is requested", () => {
    expect(
      resizeSelectionProportionally(
        { x: 20, y: 20, width: 100, height: 80 },
        "bottomRight",
        { x: 160, y: 140 },
        { x: 0, y: 0, width: 200, height: 160 },
      ),
    ).toEqual({ x: 20, y: 20, width: 140, height: 112 });
  });

  it("places the toolbar below the selection and clamps it horizontally", () => {
    const viewport = { x: 0, y: 0, width: 800, height: 600 };
    expect(
      positionFloatingToolbar(
        { x: 300, y: 120, width: 200, height: 100 },
        { width: 400, height: 48 },
        viewport,
      ),
    ).toEqual({ x: 200, y: 230 });
    expect(
      positionFloatingToolbar(
        { x: 760, y: 120, width: 20, height: 100 },
        { width: 400, height: 48 },
        viewport,
      ),
    ).toEqual({ x: 388, y: 230 });
  });

  it("falls back above the selection or to the bottom edge", () => {
    const viewport = { x: 0, y: 0, width: 800, height: 600 };
    expect(
      positionFloatingToolbar(
        { x: 100, y: 480, width: 200, height: 100 },
        { width: 300, height: 48 },
        viewport,
      ),
    ).toEqual({ x: 50, y: 422 });
    expect(
      positionFloatingToolbar(
        { x: 0, y: 0, width: 800, height: 600 },
        { width: 900, height: 80 },
        viewport,
      ),
    ).toEqual({ x: 12, y: 508 });
  });
});
