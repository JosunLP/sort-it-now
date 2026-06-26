# Box Packing Concept

## Parameters

Cuboids should be packed as efficiently as possible into a cuboid container.

## Relevant Values

Maximum container weight.
Individual object weights.
Container dimensions (3D).
Individual object dimensions (3D).
Multiple container types with individual dimensions and weight limits.

## Goal

Algorithmic solution.
If container volume or base area is insufficient, objects should be packed efficiently across multiple containers of the specified sizes.

In addition to placing the objects, the calculation must report the **void volume** of every finished container — the empty space that is not occupied by any object and therefore has to be filled with packaging/cushioning material (air pillows, foam, packing paper, …) to immobilise the load during transport. This is reported per container and aggregated across the whole shipment.

Accordingly, the algorithm must be executed multiple times until all objects are packed.
Additionally, the algorithm can combine different container types to best meet requirements.

Heavy objects must always be below lighter objects, and weight must be evenly distributed across the base area.

Large objects should preferably be placed at the bottom. The base area should be loaded as evenly as possible with weight and filled as uniformly as possible with objects. Objects must not overhang in a way that would cause them to fall.

In the end, objects should be packed as space-filling and compact as possible.

Objects cannot be rotated.

## Tech Stack

Rust console application with persistent runtime and accessible interfaces

3D geometric heuristic combined with weight distribution and layering
