// Adapted from Kiri (MIT License), commit 52d2d2f8fd613ce3341cbfae371009f06b123903.
// Copyright (c) 2026 yuxino. See THIRD_PARTY_NOTICES.md.

import type { AnnotationMark, MosaicIntensity } from "./model";
import { MOSAIC_VIEW_BLOCK_SIZE } from "./model";

type MosaicMark = Extract<AnnotationMark, {kind: "mosaic"}>;

export function mosaicDocumentBlurRadius(diameter: number, intensity: MosaicIntensity): number {
  const factor = intensity === "soft" ? .18 : intensity === "standard" ? .25 : .34;
  return Math.max(2, Math.round(diameter * factor));
}

/** Each style is deterministic; within a style the stronger effect paints last. */
export function orderedMosaics(marks: readonly MosaicMark[]): MosaicMark[] {
  const strength = (mark: MosaicMark) => mark.style === "pixel"
    ? MOSAIC_VIEW_BLOCK_SIZE[mark.intensity]
    : mosaicDocumentBlurRadius(mark.brushDiameter, mark.intensity);
  return [...marks].sort((a, b) => (a.style === b.style ? strength(a) - strength(b)
    : a.style === "pixel" ? -1 : 1));
}
