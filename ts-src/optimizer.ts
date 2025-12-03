/**
 * Optimization logic for 3D packing of objects.
 * 
 * This module implements a heuristic algorithm for efficient placement
 * of objects in containers considering:
 * - Weight limits and distribution
 * - Stability and support
 * - Center of gravity balance
 * - Layering (heavy objects below)
 */

import type { Box3D, Container, ContainerBlueprint, PlacedBox } from './model.ts';
import {
  boxVolume,
  blueprintInstantiate,
  blueprintCanFit,
  containerCanFit,
  containerTotalWeight,
} from './model.ts';
import { intersects, overlapAreaXY } from './geometry.ts';

/**
 * Configuration for the packing algorithm.
 */
export interface PackingConfig {
  /** Step size for position grid (smaller values = more accurate, but slower) */
  gridStep: number;
  /** Minimum fraction of base area that must be supported (0.0 to 1.0) */
  supportRatio: number;
  /** Tolerance for height comparisons */
  heightEpsilon: number;
  /** General numerical tolerance */
  generalEpsilon: number;
  /** Maximum allowed center of gravity deviation (as ratio of diagonal) */
  balanceLimitRatio: number;
  /** Relative tolerance for footprint clustering to reduce backtracking */
  footprintClusterTolerance: number;
}

/**
 * Default packing configuration
 */
export const defaultPackingConfig: PackingConfig = {
  gridStep: 5.0,
  supportRatio: 0.6,
  heightEpsilon: 1e-3,
  generalEpsilon: 1e-6,
  balanceLimitRatio: 0.45,
  footprintClusterTolerance: 0.15,
};

/**
 * Support diagnostics per object
 */
export interface SupportDiagnostics {
  objectId: number;
  supportPercent: number;
  restsOnFloor: boolean;
}

/**
 * Diagnostic metrics per container for monitoring
 */
export interface ContainerDiagnostics {
  centerOfMassOffset: number;
  balanceLimit: number;
  imbalanceRatio: number;
  averageSupportPercent: number;
  minimumSupportPercent: number;
  supportSamples: SupportDiagnostics[];
}

/**
 * Summary of important metrics across all containers
 */
export interface PackingDiagnosticsSummary {
  maxImbalanceRatio: number;
  worstSupportPercent: number;
  averageSupportPercent: number;
}

/**
 * Reasons why an object could not be placed
 */
export type UnplacedReason =
  | 'too_heavy_for_container'
  | 'dimensions_exceed_container'
  | 'no_stable_position';

/**
 * Object that could not be placed
 */
export interface UnplacedBox {
  object: Box3D;
  reason: UnplacedReason;
}

/**
 * Result of the packing calculation
 */
export interface PackingResult {
  containers: Container[];
  unplaced: UnplacedBox[];
  containerDiagnostics: ContainerDiagnostics[];
  diagnosticsSummary: PackingDiagnosticsSummary;
}

/**
 * Events that occur during packing to enable live visualization
 */
export type PackEvent =
  | {
      type: 'ContainerStarted';
      id: number;
      dims: [number, number, number];
      maxWeight: number;
      label?: string;
      templateId?: number;
    }
  | {
      type: 'ObjectPlaced';
      containerId: number;
      id: number;
      pos: [number, number, number];
      weight: number;
      dims: [number, number, number];
      totalWeight: number;
    }
  | {
      type: 'ContainerDiagnostics';
      containerId: number;
      diagnostics: ContainerDiagnostics;
    }
  | {
      type: 'ObjectRejected';
      id: number;
      weight: number;
      dims: [number, number, number];
      reasonCode: string;
      reasonText: string;
    }
  | {
      type: 'Finished';
      containers: number;
      unplaced: number;
      diagnosticsSummary: PackingDiagnosticsSummary;
    };

/**
 * Main function for packing objects into containers.
 * 
 * Sorts objects by weight and volume (heavy/large first) and places
 * them one by one into containers. Creates new containers when necessary.
 */
export function packObjects(
  objects: Box3D[],
  containerTemplates: ContainerBlueprint[],
  config: PackingConfig = defaultPackingConfig
): PackingResult {
  return packObjectsWithProgress(objects, containerTemplates, config, () => {});
}

/**
 * Packing with custom configuration and live progress callback.
 * 
 * Calls a callback for each important step (suitable for SSE/WebSocket).
 */
export function packObjectsWithProgress(
  objects: Box3D[],
  containerTemplates: ContainerBlueprint[],
  config: PackingConfig,
  onEvent: (event: PackEvent) => void
): PackingResult {
  if (objects.length === 0) {
    onEvent({
      type: 'Finished',
      containers: 0,
      unplaced: 0,
      diagnosticsSummary: {
        maxImbalanceRatio: 0,
        worstSupportPercent: 100,
        averageSupportPercent: 100,
      },
    });
    return {
      containers: [],
      unplaced: [],
      containerDiagnostics: [],
      diagnosticsSummary: {
        maxImbalanceRatio: 0,
        worstSupportPercent: 100,
        averageSupportPercent: 100,
      },
    };
  }

  if (containerTemplates.length === 0) {
    const unplaced: UnplacedBox[] = objects.map((obj) => {
      onEvent({
        type: 'ObjectRejected',
        id: obj.id,
        weight: obj.weight,
        dims: obj.dims,
        reasonCode: 'dimensions_exceed_container',
        reasonText: 'Objekt passt in mindestens einer Dimension nicht in den Container',
      });
      return {
        object: obj,
        reason: 'dimensions_exceed_container',
      };
    });
    onEvent({
      type: 'Finished',
      containers: 0,
      unplaced: unplaced.length,
      diagnosticsSummary: {
        maxImbalanceRatio: 0,
        worstSupportPercent: 100,
        averageSupportPercent: 100,
      },
    });
    return {
      containers: [],
      unplaced,
      containerDiagnostics: [],
      diagnosticsSummary: {
        maxImbalanceRatio: 0,
        worstSupportPercent: 100,
        averageSupportPercent: 100,
      },
    };
  }

  // Sort templates by volume and weight
  const templates = [...containerTemplates].sort((a, b) => {
    const volA = a.dims[0] * a.dims[1] * a.dims[2];
    const volB = b.dims[0] * b.dims[1] * b.dims[2];
    return volA !== volB ? volA - volB : a.maxWeight - b.maxWeight;
  });

  // Sort objects: heavy and large first (stability principle)
  const sortedObjects = [...objects].sort((a, b) => {
    if (b.weight !== a.weight) {
      return b.weight - a.weight;
    }
    return boxVolume(b) - boxVolume(a) || a.id - b.id;
  });

  const containers: Container[] = [];
  const unplaced: UnplacedBox[] = [];
  const containerDiagnostics: ContainerDiagnostics[] = [];

  // Place each object
  for (const obj of sortedObjects) {
    let placed = false;

    // Try to place in existing containers
    for (let idx = 0; idx < containers.length; idx++) {
      if (!containerCanFit(containers[idx]!, obj)) {
        continue;
      }
      const position = findStablePosition(obj, containers[idx]!, config);
      if (position) {
        containers[idx]!.placed.push({
          object: obj,
          position,
        });
        const totalW = containerTotalWeight(containers[idx]!);
        onEvent({
          type: 'ObjectPlaced',
          containerId: idx + 1,
          id: obj.id,
          pos: position,
          weight: obj.weight,
          dims: obj.dims,
          totalWeight: totalW,
        });
        const diagnostics = computeContainerDiagnostics(containers[idx]!, config);
        containerDiagnostics[idx] = diagnostics;
        onEvent({
          type: 'ContainerDiagnostics',
          containerId: idx + 1,
          diagnostics,
        });
        placed = true;
        break;
      }
    }

    if (!placed) {
      // Try to create a new container
      const templateIndex = templates.findIndex((tpl) => blueprintCanFit(tpl, obj));
      if (templateIndex >= 0) {
        const template = templates[templateIndex]!;
        const newContainer = blueprintInstantiate(template);
        const newId = containers.length + 1;
        const position = findStablePosition(obj, newContainer, config);
        if (position) {
          newContainer.placed.push({
            object: obj,
            position,
          });
          onEvent({
            type: 'ContainerStarted',
            id: newId,
            dims: newContainer.dims,
            maxWeight: newContainer.maxWeight,
            label: newContainer.label,
            templateId: newContainer.templateId,
          });
          containers.push(newContainer);
          const totalW = containerTotalWeight(newContainer);
          onEvent({
            type: 'ObjectPlaced',
            containerId: newId,
            id: obj.id,
            pos: position,
            weight: obj.weight,
            dims: obj.dims,
            totalWeight: totalW,
          });
          const diagnostics = computeContainerDiagnostics(newContainer, config);
          containerDiagnostics.push(diagnostics);
          onEvent({
            type: 'ContainerDiagnostics',
            containerId: newId,
            diagnostics,
          });
          placed = true;
        } else {
          onEvent({
            type: 'ObjectRejected',
            id: obj.id,
            weight: obj.weight,
            dims: obj.dims,
            reasonCode: 'no_stable_position',
            reasonText: 'Keine stabile Position innerhalb des Containers gefunden',
          });
          unplaced.push({
            object: obj,
            reason: 'no_stable_position',
          });
        }
      } else {
        const reason = determineUnfitReason(templates, obj, config);
        onEvent({
          type: 'ObjectRejected',
          id: obj.id,
          weight: obj.weight,
          dims: obj.dims,
          reasonCode: reason,
          reasonText: getReasonText(reason),
        });
        unplaced.push({
          object: obj,
          reason,
        });
      }
    }
  }

  const diagnosticsSummary = summarizeDiagnostics(containerDiagnostics);
  onEvent({
    type: 'Finished',
    containers: containers.length,
    unplaced: unplaced.length,
    diagnosticsSummary,
  });

  return {
    containers,
    unplaced,
    containerDiagnostics,
    diagnosticsSummary,
  };
}

/**
 * Finds a stable position for an object in a container.
 */
function findStablePosition(
  box: Box3D,
  container: Container,
  config: PackingConfig
): [number, number, number] | null {
  if (!containerCanFit(container, box)) {
    return null;
  }

  const [boxW, boxD, boxH] = box.dims;
  const [contW, contD, contH] = container.dims;

  // Generate possible positions
  const xs = axisPositions(contW, boxW, config.gridStep);
  const ys = axisPositions(contD, boxD, config.gridStep);
  const zs = zLevels(container, boxH, config);

  // Try positions in order: lower Z first, then front-left corner preference
  for (const z of zs) {
    for (const y of ys) {
      for (const x of xs) {
        const pos: [number, number, number] = [x, y, z];
        const candidate: PlacedBox = { object: box, position: pos };

        // Check if position is valid
        if (
          !fitsInContainer(candidate, container, config) ||
          hasCollision(candidate, container)
        ) {
          continue;
        }

        // Check stability conditions
        if (!hasSufficientSupport(candidate, container, config)) {
          continue;
        }

        if (!supportsWeightCorrectly(candidate, container, config)) {
          continue;
        }

        if (!maintainsBalance(candidate, container, config)) {
          continue;
        }

        return pos;
      }
    }
  }

  return null;
}

/**
 * Generate possible X or Y positions along an axis
 */
function axisPositions(containerDim: number, boxDim: number, step: number): number[] {
  const positions: number[] = [0];
  const maxPos = containerDim - boxDim;

  if (maxPos <= 0) {
    return positions;
  }

  for (let pos = step; pos < maxPos; pos += step) {
    positions.push(pos);
  }

  if (positions[positions.length - 1] !== maxPos) {
    positions.push(maxPos);
  }

  return positions;
}

/**
 * Generate possible Z levels based on existing objects
 */
function zLevels(container: Container, boxHeight: number, config: PackingConfig): number[] {
  const levels: number[] = [0];

  // Add levels at the top of each existing box
  for (const placed of container.placed) {
    const topZ = placed.position[2] + placed.object.dims[2];
    if (topZ + boxHeight <= container.dims[2] + config.heightEpsilon) {
      if (!levels.includes(topZ)) {
        levels.push(topZ);
      }
    }
  }

  return levels.sort((a, b) => a - b);
}

/**
 * Checks if candidate fits within container bounds
 */
function fitsInContainer(
  candidate: PlacedBox,
  container: Container,
  config: PackingConfig
): boolean {
  const [x, y, z] = candidate.position;
  const [w, d, h] = candidate.object.dims;
  const [cw, cd, ch] = container.dims;
  const eps = config.generalEpsilon;

  return (
    x >= -eps &&
    y >= -eps &&
    z >= -eps &&
    x + w <= cw + eps &&
    y + d <= cd + eps &&
    z + h <= ch + eps
  );
}

/**
 * Checks if candidate collides with any existing box
 */
function hasCollision(candidate: PlacedBox, container: Container): boolean {
  return container.placed.some((placed) => intersects(candidate, placed));
}

/**
 * Checks if the candidate has sufficient support from below
 */
function hasSufficientSupport(
  candidate: PlacedBox,
  container: Container,
  config: PackingConfig
): boolean {
  const z = candidate.position[2];

  // On the floor - always supported
  if (z < config.heightEpsilon) {
    return true;
  }

  // Calculate required support area
  const baseArea = candidate.object.dims[0] * candidate.object.dims[1];
  const requiredSupport = baseArea * config.supportRatio;

  // Find supporting boxes (directly below)
  const supportingBoxes = container.placed.filter((placed) => {
    const topZ = placed.position[2] + placed.object.dims[2];
    return Math.abs(topZ - z) < config.heightEpsilon;
  });

  if (supportingBoxes.length === 0) {
    return false;
  }

  // Calculate actual support area
  let supportArea = 0;
  for (const supporting of supportingBoxes) {
    supportArea += overlapAreaXY(candidate, supporting);
  }

  return supportArea >= requiredSupport - config.generalEpsilon;
}

/**
 * Checks if the candidate respects the weight hierarchy (no heavy on light)
 */
function supportsWeightCorrectly(
  candidate: PlacedBox,
  container: Container,
  config: PackingConfig
): boolean {
  const z = candidate.position[2];

  // On the floor - no weight check needed
  if (z < config.heightEpsilon) {
    return true;
  }

  // Find boxes directly below
  const supportingBoxes = container.placed.filter((placed) => {
    const topZ = placed.position[2] + placed.object.dims[2];
    return Math.abs(topZ - z) < config.heightEpsilon;
  });

  // Check that all supporting boxes are at least as heavy
  for (const supporting of supportingBoxes) {
    const overlap = overlapAreaXY(candidate, supporting);
    if (overlap > config.generalEpsilon) {
      if (candidate.object.weight > supporting.object.weight + config.generalEpsilon) {
        return false;
      }
    }
  }

  return true;
}

/**
 * Checks if adding the candidate maintains center of gravity balance
 */
function maintainsBalance(
  candidate: PlacedBox,
  container: Container,
  config: PackingConfig
): boolean {
  // Calculate center of mass with new object
  let totalWeight = candidate.object.weight;
  let weightedX = candidate.position[0] * candidate.object.weight;
  let weightedY = candidate.position[1] * candidate.object.weight;

  for (const placed of container.placed) {
    totalWeight += placed.object.weight;
    weightedX += placed.position[0] * placed.object.weight;
    weightedY += placed.position[1] * placed.object.weight;
  }

  if (totalWeight <= config.generalEpsilon) {
    return true;
  }

  const comX = weightedX / totalWeight;
  const comY = weightedY / totalWeight;

  // Check if center of mass is within acceptable bounds
  const centerX = container.dims[0] / 2;
  const centerY = container.dims[1] / 2;

  const offsetX = comX - centerX;
  const offsetY = comY - centerY;
  const offset = Math.sqrt(offsetX * offsetX + offsetY * offsetY);

  const diagonal = Math.sqrt(
    container.dims[0] * container.dims[0] + container.dims[1] * container.dims[1]
  );
  const limit = diagonal * config.balanceLimitRatio;

  return offset <= limit + config.generalEpsilon;
}

/**
 * Computes diagnostic metrics for a container
 */
function computeContainerDiagnostics(
  container: Container,
  config: PackingConfig
): ContainerDiagnostics {
  let totalWeight = 0;
  let weightedX = 0;
  let weightedY = 0;
  const supportSamples: SupportDiagnostics[] = [];

  for (const placed of container.placed) {
    totalWeight += placed.object.weight;
    weightedX += placed.position[0] * placed.object.weight;
    weightedY += placed.position[1] * placed.object.weight;

    // Calculate support for this object
    const z = placed.position[2];
    const restsOnFloor = z < config.heightEpsilon;
    let supportPercent = 100;

    if (!restsOnFloor) {
      const baseArea = placed.object.dims[0] * placed.object.dims[1];
      const supportingBoxes = container.placed.filter((other) => {
        const topZ = other.position[2] + other.object.dims[2];
        return Math.abs(topZ - z) < config.heightEpsilon;
      });

      let supportArea = 0;
      for (const supporting of supportingBoxes) {
        supportArea += overlapAreaXY(placed, supporting);
      }
      supportPercent = (supportArea / baseArea) * 100;
    }

    supportSamples.push({
      objectId: placed.object.id,
      supportPercent,
      restsOnFloor,
    });
  }

  const comX = totalWeight > 0 ? weightedX / totalWeight : container.dims[0] / 2;
  const comY = totalWeight > 0 ? weightedY / totalWeight : container.dims[1] / 2;

  const centerX = container.dims[0] / 2;
  const centerY = container.dims[1] / 2;

  const offsetX = comX - centerX;
  const offsetY = comY - centerY;
  const centerOfMassOffset = Math.sqrt(offsetX * offsetX + offsetY * offsetY);

  const diagonal = Math.sqrt(
    container.dims[0] * container.dims[0] + container.dims[1] * container.dims[1]
  );
  const balanceLimit = diagonal * config.balanceLimitRatio;
  const imbalanceRatio = centerOfMassOffset / (balanceLimit || 1);

  const avgSupport =
    supportSamples.length > 0
      ? supportSamples.reduce((sum, s) => sum + s.supportPercent, 0) / supportSamples.length
      : 100;

  const minSupport =
    supportSamples.length > 0
      ? Math.min(...supportSamples.map((s) => s.supportPercent))
      : 100;

  return {
    centerOfMassOffset,
    balanceLimit,
    imbalanceRatio,
    averageSupportPercent: avgSupport,
    minimumSupportPercent: minSupport,
    supportSamples,
  };
}

/**
 * Summarizes diagnostics across all containers
 */
function summarizeDiagnostics(
  diagnostics: ContainerDiagnostics[]
): PackingDiagnosticsSummary {
  if (diagnostics.length === 0) {
    return {
      maxImbalanceRatio: 0,
      worstSupportPercent: 100,
      averageSupportPercent: 100,
    };
  }

  const maxImbalance = Math.max(...diagnostics.map((d) => d.imbalanceRatio));
  const worstSupport = Math.min(...diagnostics.map((d) => d.minimumSupportPercent));
  const avgSupport =
    diagnostics.reduce((sum, d) => sum + d.averageSupportPercent, 0) / diagnostics.length;

  return {
    maxImbalanceRatio: maxImbalance,
    worstSupportPercent: worstSupport,
    averageSupportPercent: avgSupport,
  };
}

/**
 * Determines why an object doesn't fit in any template
 */
function determineUnfitReason(
  templates: ContainerBlueprint[],
  object: Box3D,
  config: PackingConfig
): UnplacedReason {
  if (templates.length === 0) {
    return 'dimensions_exceed_container';
  }

  const weightBlocked = templates.every(
    (tpl) => object.weight > tpl.maxWeight + config.generalEpsilon
  );
  if (weightBlocked) {
    return 'too_heavy_for_container';
  }

  const dimensionBlocked = templates.every(
    (tpl) =>
      object.dims[0] > tpl.dims[0] + config.generalEpsilon ||
      object.dims[1] > tpl.dims[1] + config.generalEpsilon ||
      object.dims[2] > tpl.dims[2] + config.generalEpsilon
  );
  if (dimensionBlocked) {
    return 'dimensions_exceed_container';
  }

  return 'no_stable_position';
}

/**
 * Gets the text description for an unplaced reason
 */
function getReasonText(reason: UnplacedReason): string {
  switch (reason) {
    case 'too_heavy_for_container':
      return 'Objekt überschreitet das zulässige Gesamtgewicht';
    case 'dimensions_exceed_container':
      return 'Objekt passt in mindestens einer Dimension nicht in den Container';
    case 'no_stable_position':
      return 'Keine stabile Position innerhalb des Containers gefunden';
  }
}
