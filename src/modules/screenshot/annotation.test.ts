// Adapted from Kiri integration tests where applicable. See THIRD_PARTY_NOTICES.md.
import { describe, expect, it } from "vitest";
import {
  AnnotationHistory,
  markIndexAt,
  resizeAnnotationMark,
  selectionBounds,
  translateMark,
  type AnnotationMark,
} from "./upstream/kiri/model";

function rectangle(id: number, x: number, y: number): AnnotationMark {
  return {
    kind: "rectangle",
    id,
    rect: { x, y, width: 80, height: 50 },
    color: "orange",
    width: 3,
  };
}

describe("Kiri annotation model", () => {
  it("keeps append, replace, undo and redo in sync", () => {
    const first = rectangle(1, 10, 10);
    const second = { ...first, id: 2, rect: { x: 40, y: 30, width: 80, height: 50 } };
    const history = new AnnotationHistory();

    history.append(first);
    history.replace(0, second);
    expect(history.elements).toEqual([second]);
    expect(history.canUndo).toBe(true);

    history.undo();
    expect(history.elements).toEqual([first]);
    expect(history.canRedo).toBe(true);

    history.redo();
    expect(history.elements).toEqual([second]);
  });

  it("hit-tests the topmost annotation first", () => {
    const lower = rectangle(1, 10, 10);
    const upper = rectangle(2, 20, 20);
    expect(markIndexAt([lower, upper], { x: 30, y: 30 })).toBe(1);
  });

  it("moves an annotation while clamping it to the selection", () => {
    const mark = rectangle(1, 20, 20);
    expect(
      translateMark(mark, { x: 500, y: -100 }, { x: 0, y: 0, width: 160, height: 120 }),
    ).toEqual({
      ...mark,
      rect: { x: 80, y: 0, width: 80, height: 50 },
    });
  });

  it("resizes rectangle and ellipse annotations from eight handles", () => {
    const rectangleMark = rectangle(1, 20, 20);
    expect(
      resizeAnnotationMark(
        rectangleMark,
        "bottomRight",
        { x: 160, y: 140 },
        { x: 0, y: 0, width: 200, height: 160 },
      ),
    ).toMatchObject({ rect: { x: 20, y: 20, width: 140, height: 120 } });

    const ellipse: AnnotationMark = {
      kind: "ellipse",
      id: 2,
      rect: { x: 20, y: 20, width: 80, height: 50 },
      color: "blue",
      width: 3,
    };
    expect(
      resizeAnnotationMark(
        ellipse,
        "left",
        { x: 10, y: 40 },
        { x: 0, y: 0, width: 200, height: 160 },
      ),
    ).toMatchObject({ rect: { x: 10, y: 20, width: 90, height: 50 } });
  });

  it("scales line and arrow endpoints with their selection bounds", () => {
    const line: AnnotationMark = {
      kind: "line",
      id: 1,
      start: { x: 20, y: 30 },
      end: { x: 80, y: 70 },
      color: "orange",
      width: 3,
    };
    const resized = resizeAnnotationMark(
      line,
      "bottomRight",
      { x: 160, y: 140 },
      { x: 0, y: 0, width: 200, height: 160 },
    );
    expect(resized.kind).toBe("line");
    if (resized.kind == "line") {
      expect(resized.end.x).toBeGreaterThan(line.end.x);
      expect(resized.end.y).toBeGreaterThan(line.end.y);
    }
  });

  it("computes ellipse and mosaic selection bounds", () => {
    const ellipse: AnnotationMark = {
      kind: "ellipse",
      id: 1,
      rect: { x: 10, y: 20, width: 80, height: 50 },
      color: "orange",
      width: 3,
    };
    expect(selectionBounds(ellipse)).toEqual({ x: 10, y: 20, width: 80, height: 50 });

    const mosaic: AnnotationMark = {
      kind: "mosaic",
      id: 2,
      points: [
        { x: 20, y: 30 },
        { x: 70, y: 80 },
      ],
      brushDiameter: 20,
      intensity: "standard",
      style: "pixel",
      shape: "rectangle",
    };
    expect(selectionBounds(mosaic)).toEqual({ x: 20, y: 30, width: 50, height: 50 });
  });
});
