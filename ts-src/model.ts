/**
 * Data models for the 3D box packing simulation.
 * 
 * This module defines the fundamental data structures for 3D packing optimization:
 * - Box3D: Represents an object to be packed with dimensions and weight
 * - PlacedBox: An object with its position in the container
 * - Container: The packing container with capacity limits
 */

/**
 * Validation error for object data
 */
export class ValidationError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'ValidationError';
  }
}

/**
 * Represents a 3D object to be packed.
 */
export interface Box3D {
  id: number;
  dims: [number, number, number]; // [width, depth, height]
  weight: number;
}

/**
 * Creates a new Box3D object with validation.
 */
export function createBox3D(id: number, dims: [number, number, number], weight: number): Box3D {
  const [w, d, h] = dims;

  if (w <= 0 || !isFinite(w)) {
    throw new ValidationError(`Breite muss positiv sein, erhalten: ${w}`);
  }
  if (d <= 0 || !isFinite(d)) {
    throw new ValidationError(`Tiefe muss positiv sein, erhalten: ${d}`);
  }
  if (h <= 0 || !isFinite(h)) {
    throw new ValidationError(`Höhe muss positiv sein, erhalten: ${h}`);
  }
  if (weight <= 0 || !isFinite(weight)) {
    throw new ValidationError(`Gewicht muss positiv sein, erhalten: ${weight}`);
  }

  return { id, dims, weight };
}

/**
 * Calculates the volume of the object.
 */
export function boxVolume(box: Box3D): number {
  const [w, d, h] = box.dims;
  return w * d * h;
}

/**
 * Returns the base area of the object.
 */
export function boxBaseArea(box: Box3D): number {
  const [w, d] = box.dims;
  return w * d;
}

/**
 * A placed object with its position in the container.
 */
export interface PlacedBox {
  object: Box3D;
  position: [number, number, number]; // [x, y, z] position of lower-left corner
}

/**
 * Returns the top Z coordinate of the placed object.
 */
export function placedBoxTopZ(placed: PlacedBox): number {
  return placed.position[2] + placed.object.dims[2];
}

/**
 * Returns the center of the placed object.
 */
export function placedBoxCenter(placed: PlacedBox): [number, number, number] {
  return [
    placed.position[0] + placed.object.dims[0] / 2,
    placed.position[1] + placed.object.dims[1] / 2,
    placed.position[2] + placed.object.dims[2] / 2,
  ];
}

/**
 * Represents a packing container with capacity limits.
 */
export interface Container {
  dims: [number, number, number]; // [width, depth, height]
  maxWeight: number;
  placed: PlacedBox[];
  templateId?: number;
  label?: string;
}

/**
 * Creates a new empty container with validation.
 */
export function createContainer(
  dims: [number, number, number],
  maxWeight: number
): Container {
  const [w, d, h] = dims;

  if (w <= 0 || !isFinite(w)) {
    throw new ValidationError(`Container-Breite muss positiv sein, erhalten: ${w}`);
  }
  if (d <= 0 || !isFinite(d)) {
    throw new ValidationError(`Container-Tiefe muss positiv sein, erhalten: ${d}`);
  }
  if (h <= 0 || !isFinite(h)) {
    throw new ValidationError(`Container-Höhe muss positiv sein, erhalten: ${h}`);
  }
  if (maxWeight <= 0 || !isFinite(maxWeight)) {
    throw new ValidationError(`Maximales Gewicht muss positiv sein, erhalten: ${maxWeight}`);
  }

  return {
    dims,
    maxWeight,
    placed: [],
  };
}

/**
 * Calculates the total weight of all placed objects.
 */
export function containerTotalWeight(container: Container): number {
  return container.placed.reduce((sum, p) => sum + p.object.weight, 0);
}

/**
 * Calculates the remaining available weight.
 */
export function containerRemainingWeight(container: Container): number {
  return container.maxWeight - containerTotalWeight(container);
}

/**
 * Calculates the used volume in the container.
 */
export function containerUsedVolume(container: Container): number {
  return container.placed.reduce((sum, p) => sum + boxVolume(p.object), 0);
}

/**
 * Calculates the total volume of the container.
 */
export function containerTotalVolume(container: Container): number {
  const [w, d, h] = container.dims;
  return w * d * h;
}

/**
 * Calculates the container utilization in percent.
 */
export function containerUtilizationPercent(container: Container): number {
  const total = containerTotalVolume(container);
  if (total <= 0) {
    return 0;
  }
  return (containerUsedVolume(container) / total) * 100;
}

/**
 * Checks if an object can theoretically fit in the container.
 * Considers weight and dimensions with tolerance.
 */
export function containerCanFit(container: Container, box: Box3D): boolean {
  const tolerance = 1e-6;
  return (
    containerRemainingWeight(container) + tolerance >= box.weight &&
    box.dims[0] <= container.dims[0] + tolerance &&
    box.dims[1] <= container.dims[1] + tolerance &&
    box.dims[2] <= container.dims[2] + tolerance
  );
}

/**
 * Creates a new empty container with the same properties.
 */
export function containerEmptyLike(container: Container): Container {
  return {
    dims: container.dims,
    maxWeight: container.maxWeight,
    placed: [],
    templateId: container.templateId,
    label: container.label,
  };
}

/**
 * Sets metadata for the container (builder pattern).
 */
export function containerWithMeta(
  container: Container,
  templateId: number,
  label?: string
): Container {
  return {
    ...container,
    templateId,
    label,
  };
}

/**
 * Template for a container type.
 */
export interface ContainerBlueprint {
  id: number;
  label?: string;
  dims: [number, number, number];
  maxWeight: number;
}

/**
 * Creates a new container template after validation of parameters.
 */
export function createContainerBlueprint(
  id: number,
  label: string | undefined,
  dims: [number, number, number],
  maxWeight: number
): ContainerBlueprint {
  // Validation is ensured through createContainer
  createContainer(dims, maxWeight);
  return { id, label, dims, maxWeight };
}

/**
 * Instantiates an empty container based on this template.
 */
export function blueprintInstantiate(blueprint: ContainerBlueprint): Container {
  return {
    dims: blueprint.dims,
    maxWeight: blueprint.maxWeight,
    placed: [],
    templateId: blueprint.id,
    label: blueprint.label,
  };
}

/**
 * Checks if the object can basically fit due to dimensions and weight.
 */
export function blueprintCanFit(blueprint: ContainerBlueprint, object: Box3D): boolean {
  const tolerance = 1e-6;
  return (
    object.weight <= blueprint.maxWeight + tolerance &&
    object.dims[0] <= blueprint.dims[0] + tolerance &&
    object.dims[1] <= blueprint.dims[1] + tolerance &&
    object.dims[2] <= blueprint.dims[2] + tolerance
  );
}

/**
 * Returns the volume of the template.
 */
export function blueprintVolume(blueprint: ContainerBlueprint): number {
  const [w, d, h] = blueprint.dims;
  return w * d * h;
}
