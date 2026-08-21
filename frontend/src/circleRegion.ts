export interface CircleRegion {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

/** Matches src-tauri/src/region.rs. CreateEllipticRgn uses these bounds. */
export function physicalCircleRegion(
  physicalWidth: number,
  physicalHeight: number,
): CircleRegion | null {
  if (physicalWidth <= 0 || physicalHeight <= 0) {
    return null;
  }
  const side = Math.min(physicalWidth, physicalHeight);
  return { left: 0, top: 0, right: side, bottom: side };
}
