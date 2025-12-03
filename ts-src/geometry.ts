/**
 * Geometric helper functions for 3D collision detection and space planning.
 * 
 * This module provides functions for checking overlaps between placed objects
 * and calculating overlaps in various dimensions.
 */

import type { PlacedBox } from './model.ts';

/**
 * Checks if two placed objects overlap spatially.
 * 
 * Uses Axis-Aligned Bounding Box (AABB) collision detection.
 * Two boxes do NOT overlap if they are separated in at least one axis.
 */
export function intersects(a: PlacedBox, b: PlacedBox): boolean {
  const [ax, ay, az] = a.position;
  const [aw, ad, ah] = a.object.dims;
  const [bx, by, bz] = b.position;
  const [bw, bd, bh] = b.object.dims;

  // Separating Axis Theorem: Objects do NOT overlap if
  // they are completely separated in any axis
  return !(
    ax + aw <= bx ||
    bx + bw <= ax ||
    ay + ad <= by ||
    by + bd <= ay ||
    az + ah <= bz ||
    bz + bh <= az
  );
}

/**
 * Calculates the overlap of two intervals in one dimension.
 * 
 * @returns Length of the overlap, at least 0.0
 */
export function overlap1d(a1: number, a2: number, b1: number, b2: number): number {
  return Math.max(0, Math.min(a2, b2) - Math.max(a1, b1));
}

/**
 * Calculates the overlap area of two rectangles in the XY plane.
 */
export function overlapAreaXY(a: PlacedBox, b: PlacedBox): number {
  const overlapX = overlap1d(
    a.position[0],
    a.position[0] + a.object.dims[0],
    b.position[0],
    b.position[0] + b.object.dims[0]
  );
  const overlapY = overlap1d(
    a.position[1],
    a.position[1] + a.object.dims[1],
    b.position[1],
    b.position[1] + b.object.dims[1]
  );
  return overlapX * overlapY;
}

/**
 * Checks if a point is inside an object.
 */
export function pointInside(point: [number, number, number], placedBox: PlacedBox): boolean {
  const [px, py, pz] = point;
  const [bx, by, bz] = placedBox.position;
  const [bw, bd, bh] = placedBox.object.dims;

  return (
    px >= bx &&
    px <= bx + bw &&
    py >= by &&
    py <= by + bd &&
    pz >= bz &&
    pz <= bz + bh
  );
}
