/**
 * REST API for the packing service.
 * 
 * Provides HTTP endpoints for communication with the frontend.
 * Uses Bun's native HTTP server and supports CORS.
 */

import type { Server } from 'bun';
import type { Box3D, ContainerBlueprint } from './model.ts';
import type { PackingConfig, PackEvent } from './optimizer.ts';
import { createBox3D, createContainerBlueprint, ValidationError } from './model.ts';
import { packObjectsWithProgress, defaultPackingConfig } from './optimizer.ts';

/**
 * API configuration
 */
export interface ApiConfig {
  host: string;
  port: number;
}

/**
 * Request structure for the packing endpoint
 */
interface ContainerRequest {
  name?: string;
  dims: [number, number, number];
  max_weight: number;
}

/**
 * Request structure for pack endpoint
 */
interface PackRequest {
  containers: ContainerRequest[];
  objects: Array<{
    id: number;
    dims: [number, number, number];
    weight: number;
  }>;
}

/**
 * Converts API requests to internal models
 */
function parsePackRequest(req: PackRequest): {
  objects: Box3D[];
  templates: ContainerBlueprint[];
} {
  const objects: Box3D[] = [];
  const templates: ContainerBlueprint[] = [];

  for (const objReq of req.objects) {
    try {
      const box = createBox3D(objReq.id, objReq.dims, objReq.weight);
      objects.push(box);
    } catch (error) {
      if (error instanceof ValidationError) {
        throw new Error(`Invalid object ${objReq.id}: ${error.message}`);
      }
      throw error;
    }
  }

  for (let i = 0; i < req.containers.length; i++) {
    const contReq = req.containers[i]!;
    try {
      const blueprint = createContainerBlueprint(
        i,
        contReq.name,
        contReq.dims,
        contReq.max_weight
      );
      templates.push(blueprint);
    } catch (error) {
      if (error instanceof ValidationError) {
        throw new Error(`Invalid container ${i}: ${error.message}`);
      }
      throw error;
    }
  }

  return { objects, templates };
}

/**
 * Formats packing result for API response
 */
function formatPackResult(result: ReturnType<typeof packObjectsWithProgress>) {
  return {
    results: result.containers.map((container, idx) => ({
      id: idx + 1,
      template_id: container.templateId ?? 0,
      label: container.label ?? 'Container',
      dims: container.dims,
      max_weight: container.maxWeight,
      total_weight: container.placed.reduce((sum, p) => sum + p.object.weight, 0),
      placed: container.placed.map((p) => ({
        id: p.object.id,
        pos: p.position,
        weight: p.object.weight,
        dims: p.object.dims,
      })),
    })),
    unplaced: result.unplaced.map((u) => ({
      id: u.object.id,
      weight: u.object.weight,
      dims: u.object.dims,
      reason: u.reason,
    })),
    diagnostics_summary: result.diagnosticsSummary,
  };
}

/**
 * CORS headers
 */
const corsHeaders = {
  'Access-Control-Allow-Origin': '*',
  'Access-Control-Allow-Methods': 'GET, POST, OPTIONS',
  'Access-Control-Allow-Headers': 'Content-Type',
};

/**
 * Handles OPTIONS requests for CORS preflight
 */
function handleOptions(): Response {
  return new Response(null, {
    status: 204,
    headers: corsHeaders,
  });
}

/**
 * Handles GET / - serves basic info
 */
function handleRoot(): Response {
  const html = `
<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <title>Sort-it-now API</title>
    <style>
        body { font-family: system-ui, sans-serif; max-width: 800px; margin: 50px auto; padding: 20px; }
        h1 { color: #333; }
        .endpoint { background: #f5f5f5; padding: 10px; margin: 10px 0; border-radius: 5px; }
        code { background: #e0e0e0; padding: 2px 5px; border-radius: 3px; }
    </style>
</head>
<body>
    <h1>🚀 Sort-it-now API - TypeScript/Bun Version</h1>
    <p>A 3D box packing optimization service.</p>
    
    <h2>Endpoints</h2>
    <div class="endpoint">
        <strong>POST /pack</strong><br>
        Pack objects into containers (batch mode)
    </div>
    <div class="endpoint">
        <strong>POST /pack_stream</strong><br>
        Pack objects with live progress streaming (SSE)
    </div>
    <div class="endpoint">
        <strong>GET /docs</strong><br>
        API documentation
    </div>
    
    <h2>Example Request</h2>
    <pre><code>{
  "containers": [
    { "name": "Standard", "dims": [100, 100, 70], "max_weight": 500 }
  ],
  "objects": [
    { "id": 1, "dims": [30, 30, 10], "weight": 50 },
    { "id": 2, "dims": [20, 50, 15], "weight": 30 }
  ]
}</code></pre>
</body>
</html>`;
  
  return new Response(html, {
    headers: {
      'Content-Type': 'text/html; charset=utf-8',
      ...corsHeaders,
    },
  });
}

/**
 * Handles POST /pack - batch packing
 */
async function handlePack(request: Request, config: PackingConfig): Promise<Response> {
  try {
    const body = await request.json();
    const { objects, templates } = parsePackRequest(body as PackRequest);
    
    const result = packObjectsWithProgress(objects, templates, config, () => {});
    
    return new Response(JSON.stringify(formatPackResult(result)), {
      headers: {
        'Content-Type': 'application/json',
        ...corsHeaders,
      },
    });
  } catch (error) {
    console.error('Error in /pack:', error);
    return new Response(
      JSON.stringify({
        error: error instanceof Error ? error.message : 'Unknown error',
      }),
      {
        status: 400,
        headers: {
          'Content-Type': 'application/json',
          ...corsHeaders,
        },
      }
    );
  }
}

/**
 * Handles POST /pack_stream - streaming packing with SSE
 */
async function handlePackStream(request: Request, config: PackingConfig): Promise<Response> {
  try {
    const body = await request.json();
    const { objects, templates } = parsePackRequest(body as PackRequest);
    
    // Create a ReadableStream for SSE
    const stream = new ReadableStream({
      start(controller) {
        packObjectsWithProgress(objects, templates, config, (event: PackEvent) => {
          const data = `data: ${JSON.stringify(event)}\n\n`;
          controller.enqueue(new TextEncoder().encode(data));
        });
        controller.close();
      },
    });
    
    return new Response(stream, {
      headers: {
        'Content-Type': 'text/event-stream',
        'Cache-Control': 'no-cache',
        'Connection': 'keep-alive',
        ...corsHeaders,
      },
    });
  } catch (error) {
    console.error('Error in /pack_stream:', error);
    return new Response(
      JSON.stringify({
        error: error instanceof Error ? error.message : 'Unknown error',
      }),
      {
        status: 400,
        headers: {
          'Content-Type': 'application/json',
          ...corsHeaders,
        },
      }
    );
  }
}

/**
 * Handles GET /docs - API documentation
 */
function handleDocs(): Response {
  const docs = {
    openapi: '3.0.0',
    info: {
      title: 'Sort-it-now API',
      version: '1.0.0',
      description: '3D Box Packing Optimization API - TypeScript/Bun Version',
    },
    paths: {
      '/pack': {
        post: {
          summary: 'Pack objects into containers',
          requestBody: {
            required: true,
            content: {
              'application/json': {
                schema: {
                  type: 'object',
                  properties: {
                    containers: {
                      type: 'array',
                      items: {
                        type: 'object',
                        properties: {
                          name: { type: 'string' },
                          dims: {
                            type: 'array',
                            items: { type: 'number' },
                            minItems: 3,
                            maxItems: 3,
                          },
                          max_weight: { type: 'number' },
                        },
                      },
                    },
                    objects: {
                      type: 'array',
                      items: {
                        type: 'object',
                        properties: {
                          id: { type: 'number' },
                          dims: {
                            type: 'array',
                            items: { type: 'number' },
                            minItems: 3,
                            maxItems: 3,
                          },
                          weight: { type: 'number' },
                        },
                      },
                    },
                  },
                },
              },
            },
          },
          responses: {
            '200': {
              description: 'Packing result',
            },
          },
        },
      },
      '/pack_stream': {
        post: {
          summary: 'Pack objects with live progress streaming',
          description: 'Server-Sent Events (SSE) endpoint for real-time packing progress',
          responses: {
            '200': {
              description: 'Event stream',
            },
          },
        },
      },
    },
  };
  
  return new Response(JSON.stringify(docs, null, 2), {
    headers: {
      'Content-Type': 'application/json',
      ...corsHeaders,
    },
  });
}

/**
 * Starts the API server
 */
export async function startApiServer(
  apiConfig: ApiConfig,
  packingConfig: PackingConfig = defaultPackingConfig
): Promise<Server> {
  const server = Bun.serve({
    hostname: apiConfig.host,
    port: apiConfig.port,
    async fetch(request) {
      const url = new URL(request.url);
      
      // Handle OPTIONS for CORS preflight
      if (request.method === 'OPTIONS') {
        return handleOptions();
      }
      
      // Route requests
      if (url.pathname === '/' && request.method === 'GET') {
        return handleRoot();
      }
      
      if (url.pathname === '/pack' && request.method === 'POST') {
        return handlePack(request, packingConfig);
      }
      
      if (url.pathname === '/pack_stream' && request.method === 'POST') {
        return handlePackStream(request, packingConfig);
      }
      
      if (url.pathname === '/docs' && request.method === 'GET') {
        return handleDocs();
      }
      
      // 404
      return new Response('Not Found', {
        status: 404,
        headers: corsHeaders,
      });
    },
  });
  
  console.log(`🚀 Packing Service läuft auf http://${apiConfig.host}:${apiConfig.port}`);
  console.log(`📖 API Docs: http://${apiConfig.host}:${apiConfig.port}/docs`);
  
  return server;
}
