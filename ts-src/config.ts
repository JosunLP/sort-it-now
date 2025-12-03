/**
 * Configuration management for the application.
 * 
 * Loads configuration from environment variables with fallback defaults.
 */

import type { PackingConfig } from './optimizer.ts';
import type { ApiConfig } from './api.ts';
import { defaultPackingConfig } from './optimizer.ts';

/**
 * Complete application configuration
 */
export interface AppConfig {
  api: ApiConfig;
  packing: PackingConfig;
}

/**
 * Parses a number from environment variable with fallback
 */
function parseNumber(value: string | undefined, defaultValue: number): number {
  if (!value) {
    return defaultValue;
  }
  const parsed = parseFloat(value);
  return !isNaN(parsed) && isFinite(parsed) ? parsed : defaultValue;
}

/**
 * Loads configuration from environment variables
 */
export function loadConfig(): AppConfig {
  // API configuration
  const apiHost = process.env.SORT_IT_NOW_API_HOST || '0.0.0.0';
  const apiPort = parseNumber(process.env.SORT_IT_NOW_API_PORT, 8080);

  // Packing configuration
  const gridStep = parseNumber(
    process.env.SORT_IT_NOW_PACKING_GRID_STEP,
    defaultPackingConfig.gridStep
  );
  const supportRatio = parseNumber(
    process.env.SORT_IT_NOW_PACKING_SUPPORT_RATIO,
    defaultPackingConfig.supportRatio
  );
  const heightEpsilon = parseNumber(
    process.env.SORT_IT_NOW_PACKING_HEIGHT_EPSILON,
    defaultPackingConfig.heightEpsilon
  );
  const generalEpsilon = parseNumber(
    process.env.SORT_IT_NOW_PACKING_GENERAL_EPSILON,
    defaultPackingConfig.generalEpsilon
  );
  const balanceLimitRatio = parseNumber(
    process.env.SORT_IT_NOW_PACKING_BALANCE_LIMIT_RATIO,
    defaultPackingConfig.balanceLimitRatio
  );
  const footprintClusterTolerance = parseNumber(
    process.env.SORT_IT_NOW_PACKING_FOOTPRINT_CLUSTER_TOLERANCE,
    defaultPackingConfig.footprintClusterTolerance
  );

  return {
    api: {
      host: apiHost,
      port: apiPort,
    },
    packing: {
      gridStep,
      supportRatio,
      heightEpsilon,
      generalEpsilon,
      balanceLimitRatio,
      footprintClusterTolerance,
    },
  };
}
