/**
 * Example usage of the packing library
 * 
 * This file demonstrates how to use the TypeScript packing library directly
 * without going through the REST API.
 */

import { createBox3D, createContainerBlueprint } from './model.ts';
import { packObjects, packObjectsWithProgress, defaultPackingConfig } from './optimizer.ts';

/**
 * Example 1: Simple packing scenario
 */
function example1() {
  console.log('Example 1: Simple packing scenario\n');
  
  // Define objects to pack
  const objects = [
    createBox3D(1, [30, 30, 10], 50),
    createBox3D(2, [20, 50, 15], 30),
    createBox3D(3, [25, 25, 20], 40),
  ];
  
  // Define available container templates
  const templates = [
    createContainerBlueprint(0, 'Standard Box', [100, 100, 70], 500),
  ];
  
  // Run packing algorithm
  const result = packObjects(objects, templates);
  
  // Display results
  console.log(`✅ Packed ${result.containers.length} container(s)`);
  console.log(`❌ Unplaced: ${result.unplaced.length} object(s)\n`);
  
  for (let i = 0; i < result.containers.length; i++) {
    const container = result.containers[i]!;
    console.log(`Container ${i + 1} (${container.label}):`);
    console.log(`  Objects: ${container.placed.length}`);
    console.log(`  Weight: ${container.placed.reduce((sum, p) => sum + p.object.weight, 0)}kg / ${container.maxWeight}kg`);
    
    for (const placed of container.placed) {
      console.log(`    - Object ${placed.object.id} at [${placed.position.join(', ')}]`);
    }
    console.log('');
  }
}

/**
 * Example 2: Multiple container types
 */
function example2() {
  console.log('Example 2: Multiple container types\n');
  
  // Define many objects with varying weights
  const objects = [
    createBox3D(1, [40, 40, 20], 200), // Heavy
    createBox3D(2, [30, 30, 15], 150), // Heavy
    createBox3D(3, [35, 35, 10], 100), // Medium
    createBox3D(4, [20, 20, 10], 80),  // Medium
    createBox3D(5, [25, 25, 15], 60),  // Medium
    createBox3D(6, [15, 15, 10], 30),  // Light
    createBox3D(7, [10, 10, 5], 20),   // Light
  ];
  
  // Define multiple container types
  const templates = [
    createContainerBlueprint(0, 'Small Box', [60, 60, 40], 200),
    createContainerBlueprint(1, 'Medium Box', [80, 80, 50], 350),
    createContainerBlueprint(2, 'Large Box', [120, 120, 80], 600),
  ];
  
  // Run packing algorithm
  const result = packObjects(objects, templates);
  
  console.log(`✅ Packed ${result.containers.length} container(s)`);
  console.log(`❌ Unplaced: ${result.unplaced.length} object(s)\n`);
  
  for (let i = 0; i < result.containers.length; i++) {
    const container = result.containers[i]!;
    const diagnostics = result.containerDiagnostics[i]!;
    
    console.log(`Container ${i + 1} (${container.label}):`);
    console.log(`  Objects: ${container.placed.length}`);
    console.log(`  Weight: ${container.placed.reduce((sum, p) => sum + p.object.weight, 0)}kg / ${container.maxWeight}kg`);
    console.log(`  Imbalance Ratio: ${diagnostics.imbalanceRatio.toFixed(2)}`);
    console.log(`  Average Support: ${diagnostics.averageSupportPercent.toFixed(1)}%`);
    console.log('');
  }
  
  console.log('Diagnostics Summary:');
  console.log(`  Max Imbalance: ${result.diagnosticsSummary.maxImbalanceRatio.toFixed(2)}`);
  console.log(`  Worst Support: ${result.diagnosticsSummary.worstSupportPercent.toFixed(1)}%`);
  console.log(`  Avg Support: ${result.diagnosticsSummary.averageSupportPercent.toFixed(1)}%`);
  console.log('');
}

/**
 * Example 3: With live progress callback
 */
function example3() {
  console.log('Example 3: Live progress tracking\n');
  
  const objects = [
    createBox3D(1, [30, 30, 10], 50),
    createBox3D(2, [20, 50, 15], 30),
    createBox3D(3, [25, 25, 20], 40),
    createBox3D(4, [15, 15, 10], 25),
  ];
  
  const templates = [
    createContainerBlueprint(0, 'Standard Box', [100, 100, 70], 500),
  ];
  
  // Use packObjectsWithProgress to track events
  const result = packObjectsWithProgress(
    objects,
    templates,
    defaultPackingConfig,
    (event) => {
      switch (event.type) {
        case 'ContainerStarted':
          console.log(`📦 Started container ${event.id} (${event.label})`);
          break;
        case 'ObjectPlaced':
          console.log(`  ✓ Placed object ${event.id} at [${event.pos.join(', ')}]`);
          break;
        case 'ObjectRejected':
          console.log(`  ✗ Rejected object ${event.id}: ${event.reasonText}`);
          break;
        case 'Finished':
          console.log(`\n✅ Finished: ${event.containers} container(s), ${event.unplaced} unplaced`);
          break;
      }
    }
  );
  
  console.log('');
}

/**
 * Example 4: Custom packing configuration
 */
function example4() {
  console.log('Example 4: Custom packing configuration\n');
  
  const objects = [
    createBox3D(1, [30, 30, 10], 50),
    createBox3D(2, [20, 50, 15], 30),
  ];
  
  const templates = [
    createContainerBlueprint(0, 'Standard Box', [100, 100, 70], 500),
  ];
  
  // Custom configuration with finer grid for more precise placement
  const customConfig = {
    ...defaultPackingConfig,
    gridStep: 2.5, // Smaller grid step for more precise placement
    supportRatio: 0.5, // Require only 50% support instead of 60%
  };
  
  const result = packObjects(objects, templates, customConfig);
  
  console.log(`✅ Packed ${result.containers.length} container(s) with custom config`);
  console.log(`   Grid Step: ${customConfig.gridStep}`);
  console.log(`   Support Ratio: ${customConfig.supportRatio}`);
  console.log('');
  
  for (const container of result.containers) {
    for (const placed of container.placed) {
      console.log(`  Object ${placed.object.id} at [${placed.position.join(', ')}]`);
    }
  }
  console.log('');
}

// Run examples
console.log('='.repeat(60));
console.log('📦 Sort-it-now TypeScript/Bun Examples');
console.log('='.repeat(60));
console.log('');

example1();
console.log('-'.repeat(60));
console.log('');

example2();
console.log('-'.repeat(60));
console.log('');

example3();
console.log('-'.repeat(60));
console.log('');

example4();
console.log('='.repeat(60));
