/**
 * Main entry point for the Sort-it-now TypeScript/Bun application.
 * 
 * This is a complete TypeScript rewrite of the Rust-based 3D box packing optimizer.
 * Built with Bun for maximum performance and can be compiled to a single executable.
 */

import { loadConfig } from './config.ts';
import { startApiServer } from './api.ts';

/**
 * Main application entry point
 */
async function main() {
  console.log('🚀 Sort-it-now - TypeScript/Bun Version');
  console.log('📦 3D Box Packing Optimizer\n');

  // Load configuration from environment
  const config = loadConfig();

  console.log('⚙️  Configuration:');
  console.log(`   API Host: ${config.api.host}`);
  console.log(`   API Port: ${config.api.port}`);
  console.log(`   Grid Step: ${config.packing.gridStep}`);
  console.log(`   Support Ratio: ${config.packing.supportRatio}`);
  console.log('');

  // Start the API server
  try {
    await startApiServer(config.api, config.packing);
    console.log('✅ Server started successfully');
    console.log('');
    console.log('Try it out:');
    console.log(`   curl http://${config.api.host}:${config.api.port}/`);
    console.log(`   curl http://${config.api.host}:${config.api.port}/docs`);
  } catch (error) {
    console.error('❌ Failed to start server:', error);
    process.exit(1);
  }
}

// Run the application
main().catch((error) => {
  console.error('❌ Fatal error:', error);
  process.exit(1);
});
