import * as THREE from 'https://esm.sh/three@0.163.0';
import { OrbitControls } from 'https://esm.sh/three@0.163.0/examples/jsm/controls/OrbitControls.js';

let currentContainerIndex = 0;
let packingResults = null;
let animationStep = 0;
let isAnimating = false;
let animationInterval = null;
let liveMode = false;
let liveContainers = [];
let liveUnplaced = [];
let es = null;
let liveDiagnosticsSummary = null;
let statusState = {
  mode: 'Idle',
  phase: 'Ready',
  level: 'info',
  message:
    'Use the configuration dialog to tune containers and objects, then start a batch or live run.',
  placedCount: 0,
  totalObjects: 0,
  containerCount: 0,
};

// Epsilon constants for floating point comparisons
// These match the Rust backend configuration (general_epsilon = 1e-6)
const EPSILON_COMPARISON = 1e-6; // For dimension comparisons and fitting checks
const EPSILON_DEDUPLICATION = 1e-6; // For exact equality checks in deduplication (matches backend)
const STORAGE_KEY = 'sort-it-now-config-v1';
const DEFAULT_ANIMATION_DELAY_MS = window.matchMedia?.(
  '(prefers-reduced-motion: reduce)'
)?.matches
  ? 1400
  : 800;

// Configurable parameters
const DEFAULT_CONFIG = {
  containers: [
    { id: 1, name: 'Standard 70×60×30', dims: [70, 60, 30], maxWeight: 500 },
    { id: 2, name: 'Kompakt 50×50×20', dims: [50, 50, 20], maxWeight: 300 },
  ],
  objects: [
    { id: 1, dims: [30, 30, 10], weight: 50 },
    { id: 2, dims: [20, 50, 15], weight: 30 },
    { id: 3, dims: [10, 20, 5], weight: 10 },
    { id: 4, dims: [50, 30, 5], weight: 70 },
    { id: 5, dims: [60, 40, 10], weight: 90 },
    { id: 6, dims: [15, 15, 15], weight: 20 },
    { id: 7, dims: [25, 30, 10], weight: 40 },
    { id: 8, dims: [35, 20, 10], weight: 60 },
  ],
  allowRotations: false,
};

function cloneConfigValue(value) {
  return JSON.parse(JSON.stringify(value));
}

function escapeHtml(value) {
  return String(value)
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&#39;');
}

function normalizeConfig(rawConfig) {
  const fallback = cloneConfigValue(DEFAULT_CONFIG);
  const ensureDims = (dims) => {
    if (!Array.isArray(dims) || dims.length !== 3) return null;
    const numbers = dims.map((value) => Number(value));
    return numbers.every((value) => Number.isFinite(value) && value > 0)
      ? numbers
      : null;
  };

  const containers = Array.isArray(rawConfig?.containers)
    ? rawConfig.containers
        .map((container, index) => {
          const dims = ensureDims(container?.dims);
          const maxWeight = Number(container?.maxWeight);
          if (!dims || !Number.isFinite(maxWeight) || maxWeight <= 0) {
            return null;
          }

          return {
            id: Number.isFinite(Number(container?.id))
              ? Number(container.id)
              : index + 1,
            name:
              typeof container?.name === 'string' && container.name.trim().length
                ? container.name.trim()
                : null,
            dims,
            maxWeight,
          };
        })
        .filter(Boolean)
    : [];

  const objects = Array.isArray(rawConfig?.objects)
    ? rawConfig.objects
        .map((obj, index) => {
          const dims = ensureDims(obj?.dims);
          const weight = Number(obj?.weight);
          if (!dims || !Number.isFinite(weight) || weight <= 0) {
            return null;
          }

          return {
            id: Number.isFinite(Number(obj?.id)) ? Number(obj.id) : index + 1,
            dims,
            weight,
          };
        })
        .filter(Boolean)
    : [];

  return {
    containers: containers.length ? containers : fallback.containers,
    objects: objects.length ? objects : fallback.objects,
    allowRotations: rawConfig?.allowRotations === true,
  };
}

function persistConfig() {
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(config));
  } catch (error) {
    console.warn('Could not persist configuration locally.', error);
  }
}

function loadInitialConfig() {
  try {
    const stored = window.localStorage.getItem(STORAGE_KEY);
    if (!stored) return cloneConfigValue(DEFAULT_CONFIG);
    return normalizeConfig(JSON.parse(stored));
  } catch (error) {
    console.warn('Could not restore saved configuration, using defaults.', error);
    return cloneConfigValue(DEFAULT_CONFIG);
  }
}

let config = loadInitialConfig();

function computeNextObjectId() {
  return (
    config.objects.reduce((max, obj) => {
      const id = Number.isFinite(obj.id) ? obj.id : 0;
      return id > max ? id : max;
    }, 0) + 1
  );
}

function dimsAlmostEqual(a, b, epsilon = EPSILON_DEDUPLICATION) {
  return (
    Math.abs(a[0] - b[0]) <= epsilon &&
    Math.abs(a[1] - b[1]) <= epsilon &&
    Math.abs(a[2] - b[2]) <= epsilon
  );
}

function generateOrientations(dims) {
  const [w, d, h] = dims;
  const variants = [
    [w, d, h],
    [w, h, d],
    [d, w, h],
    [d, h, w],
    [h, w, d],
    [h, d, w],
  ];

  return variants.reduce((unique, current) => {
    if (!unique.some((existing) => dimsAlmostEqual(existing, current))) {
      unique.push(current);
    }
    return unique;
  }, []);
}

function fitsContainerWithRotation(objectDims, containerDims, allowRotation) {
  const orientations = allowRotation
    ? generateOrientations(objectDims)
    : [objectDims];

  return orientations.some(([w, d, h]) => {
    return (
      w <= containerDims[0] + EPSILON_COMPARISON &&
      d <= containerDims[1] + EPSILON_COMPARISON &&
      h <= containerDims[2] + EPSILON_COMPARISON
    );
  });
}

const scene = new THREE.Scene();
scene.background = new THREE.Color(0x111111);

const camera = new THREE.PerspectiveCamera(
  60,
  window.innerWidth / window.innerHeight,
  0.1,
  1000
);
camera.position.set(150, 100, 150);

const renderer = new THREE.WebGLRenderer({ antialias: true });
renderer.setSize(window.innerWidth, window.innerHeight);
document.getElementById('container').appendChild(renderer.domElement);

scene.add(new THREE.AmbientLight(0xffffff, 0.5));
const light1 = new THREE.DirectionalLight(0xffffff, 0.8);
light1.position.set(100, 200, 100);
scene.add(light1);

const controls = new OrbitControls(camera, renderer.domElement);
controls.enableDamping = true;
controls.minDistance = 50;
controls.maxDistance = 500;

function clearScene() {
  const objectsToRemove = scene.children.filter(
    (obj) =>
      obj instanceof THREE.Mesh ||
      obj instanceof THREE.LineSegments ||
      obj instanceof THREE.Sprite
  );
  objectsToRemove.forEach((obj) => {
    if (obj.geometry) obj.geometry.dispose();
    if (obj.material) {
      if (Array.isArray(obj.material)) {
        obj.material.forEach((m) => m.dispose());
      } else {
        obj.material.dispose();
      }
    }
    scene.remove(obj);
  });
}

function drawContainerFrame(width, depth, height) {
  const geometry = new THREE.BoxGeometry(width, height, depth);
  const edges = new THREE.EdgesGeometry(geometry);
  const material = new THREE.LineBasicMaterial({ color: 0x00aaff });
  const wireframe = new THREE.LineSegments(edges, material);
  wireframe.position.set(width / 2, height / 2, depth / 2);
  scene.add(wireframe);

  const gridHelper = new THREE.GridHelper(
    Math.max(width, depth),
    10,
    0x444444,
    0x222222
  );
  gridHelper.position.set(width / 2, 0, depth / 2);
  scene.add(gridHelper);
}

function drawBox(obj, color, { opacity = 1.0, isActive = false } = {}) {
  const [x, y, z] = obj.pos;
  const [dx, dy, dz] = obj.dims;
  const geometry = new THREE.BoxGeometry(dx, dz, dy);
  const material = new THREE.MeshStandardMaterial({
    color,
    opacity,
    transparent: opacity < 1.0,
    metalness: 0.3,
    roughness: 0.7,
    emissive: isActive ? 0xffcc00 : 0x000000,
    emissiveIntensity: isActive ? 0.35 : 0,
  });
  const cube = new THREE.Mesh(geometry, material);
  cube.position.set(x + dx / 2, z + dz / 2, y + dy / 2);
  scene.add(cube);

  if (isActive) {
    const highlight = new THREE.LineSegments(
      new THREE.EdgesGeometry(geometry),
      new THREE.LineBasicMaterial({ color: 0xffcc00 })
    );
    highlight.position.copy(cube.position);
    scene.add(highlight);
  }
}

const COLOR_PALETTE = [
  0xff5555, 0x55ff55, 0x5555ff, 0xffcc00, 0x00ffff, 0xff00ff, 0xffff55,
  0xaa55ff, 0x55ffaa, 0xff7755, 0x77ff55, 0x5577ff, 0xffaa00, 0x00aaff,
  0xaa00ff, 0x55aaff, 0xaaff55, 0xff55aa, 0x55aaff, 0xffaa55, 0x55ffaa,
];

function visualizeContainer(container, containerDims, activeObjectId = null) {
  clearScene();
  drawContainerFrame(...containerDims);
  const sortedObjects = [...container.placed].sort(
    (a, b) => a.pos[2] - b.pos[2]
  );
  sortedObjects.forEach((obj, i) => {
    drawBox(obj, COLOR_PALETTE[i % COLOR_PALETTE.length], {
      opacity: 1.0,
      isActive: activeObjectId === obj.id,
    });
  });
  updateStats(container, containerDims);
}

function animateContainer(container, containerDims, step) {
  clearScene();
  drawContainerFrame(...containerDims);
  const sortedObjects = [...container.placed].sort(
    (a, b) => a.pos[2] - b.pos[2]
  );
  const objectsToShow = sortedObjects.slice(0, step + 1);
  objectsToShow.forEach((obj, i) =>
    drawBox(
      obj,
      COLOR_PALETTE[i % COLOR_PALETTE.length],
      {
        opacity: i === step ? 0.7 : 1.0,
        isActive: i === step,
      }
    )
  );
  updateStats(container, containerDims, step + 1);
}

function showToast(message, type = 'info', timeoutMs = 4200) {
  const region = document.getElementById('toastRegion');
  if (!region) return;
  const toast = document.createElement('div');
  toast.className = `toast ${type}`;
  toast.textContent = message;
  region.appendChild(toast);
  window.setTimeout(() => toast.remove(), timeoutMs);
}

function setStatus(nextStatus = {}) {
  statusState = {
    ...statusState,
    ...nextStatus,
    totalObjects: config.objects.length,
  };
  renderStatus();
}

function renderStatus() {
  const target = document.getElementById('statusContent');
  if (!target) return;

  const safeTotal = Math.max(statusState.totalObjects || config.objects.length, 0);
  const progress =
    safeTotal > 0
      ? Math.min(100, (statusState.placedCount / safeTotal) * 100)
      : 0;
  const issueCount = collectConfigIssues().length;

  target.innerHTML = `
    <div class="status-row">
      <span class="pill ${statusState.level}">Mode: ${statusState.mode}</span>
      <span class="pill ${statusState.level}">Phase: ${statusState.phase}</span>
      <span class="pill info">Containers: ${statusState.containerCount}</span>
    </div>
    <p class="status-message">${statusState.message}</p>
    <div class="status-grid">
      <div class="metric-card">
        <strong>Placed</strong>
        <span>${statusState.placedCount} / ${safeTotal}</span>
      </div>
      <div class="metric-card">
        <strong>Remaining</strong>
        <span>${Math.max(safeTotal - statusState.placedCount, 0)}</span>
      </div>
      <div class="metric-card">
        <strong>Config</strong>
        <span>${issueCount === 0 ? 'Ready' : `${issueCount} issue(s)`}</span>
      </div>
    </div>
    <div class="progress-track" aria-label="Packing progress">
      <div class="progress-fill" style="width: ${progress.toFixed(1)}%"></div>
    </div>
  `;
}

function updateUnplacedPanel(items = []) {
  const target = document.getElementById('unplacedList');
  if (!target) return;

  if (!Array.isArray(items) || items.length === 0) {
    target.className = 'empty-state';
    target.textContent = 'All configured objects fit so far.';
    return;
  }

  target.className = 'issue-list';
  target.innerHTML = items
    .map((item) => {
      const dims = Array.isArray(item.dims) ? item.dims.join(' × ') : '—';
      const reason = item.reason_text ?? item.reason ?? 'No reason provided';
      const weight = Number(item.weight);
      const weightText = Number.isFinite(weight) ? `${weight.toFixed(1)} kg` : '—';

      return `
        <li class="unplaced-item">
          <strong>Object ${escapeHtml(item.id)}</strong>
          <div class="unplaced-meta">
            ${escapeHtml(dims)} · ${escapeHtml(weightText)}<br />
            ${escapeHtml(reason)}
          </div>
        </li>
      `;
    })
    .join('');
}

function updateStats(container, dims, visibleCount = null) {
  const objectCount =
    visibleCount !== null ? visibleCount : container.placed.length;
  const containerVolume = dims[0] * dims[1] * dims[2];
  const usedVolume = container.placed
    .slice(0, objectCount)
    .reduce((sum, obj) => sum + obj.dims[0] * obj.dims[1] * obj.dims[2], 0);
  const utilization = ((usedVolume / containerVolume) * 100).toFixed(1);
  const unplacedCount = liveMode
    ? liveUnplaced.length
    : packingResults && Array.isArray(packingResults.unplaced)
    ? packingResults.unplaced.length
    : 0;
  const totalWeight =
    typeof container.total_weight === 'number'
      ? container.total_weight
      : typeof container.totalWeight === 'number'
      ? container.totalWeight
      : 0;
  const maxWeight =
    container.max_weight ??
    container.maxWeight ??
    config.containers[0]?.maxWeight ??
    null;
  const label = container.label ?? null;
  const containerTitle = label
    ? `${label} (Container ${currentContainerIndex + 1})`
    : `${liveMode ? 'Live Container' : 'Container'} ${
        currentContainerIndex + 1
      }`;
  const diagnostics = container.diagnostics ?? null;
  const summary = liveMode
    ? liveDiagnosticsSummary
    : packingResults?.diagnostics_summary ?? null;

  const formatPercent = (value, fractionDigits = 1) => {
    if (!Number.isFinite(value)) return '—';
    return `${(value * 100).toFixed(fractionDigits)}%`;
  };

  const formatPlainPercent = (value, fractionDigits = 1) => {
    if (!Number.isFinite(value)) return '—';
    return `${value.toFixed(fractionDigits)}%`;
  };

  const limitText = Number.isFinite(diagnostics?.balance_limit)
    ? `${diagnostics.balance_limit.toFixed(1)} cm`
    : '—';
  const offsetText = Number.isFinite(diagnostics?.center_of_mass_offset)
    ? `${diagnostics.center_of_mass_offset.toFixed(1)} cm`
    : '—';

  const diagnosticsHtml = diagnostics
    ? `
    <p><strong>Balance:</strong> ${formatPercent(
      diagnostics.imbalance_ratio
    )} (Limit ${limitText})</p>
    <p><strong>Center of Mass Offset:</strong> ${offsetText}</p>
    <p><strong>Support:</strong> Ø ${formatPlainPercent(
      diagnostics.average_support_percent
    )} · min ${formatPlainPercent(diagnostics.minimum_support_percent)}</p>
  `
    : '';

  const summaryHtml = summary
    ? `
      <hr />
      <p><strong>Diagnostics (total):</strong></p>
      <p>Max. Imbalance: ${formatPercent(summary.max_imbalance_ratio)}</p>
      <p>Support Ø / min: ${formatPlainPercent(
        summary.average_support_percent
      )} · ${formatPlainPercent(summary.worst_support_percent)}</p>
    `
    : '';

  document.getElementById('stats').innerHTML = `
    <h3>${escapeHtml(containerTitle)} / ${
    liveMode
      ? liveContainers.length || 1
      : packingResults
      ? packingResults.results.length
      : 1
   }
     </h3>
    <p><strong>Dimensions:</strong> ${escapeHtml(dims.join(' × '))}</p>
    <p><strong>Objects:</strong> ${objectCount} / ${container.placed.length}</p>
    <p><strong>Weight:</strong> ${totalWeight.toFixed(2)} kg${
    maxWeight ? ` / ${maxWeight} kg` : ''
  }</p>
    <p><strong>Utilization:</strong> ${utilization}%</p>
    ${
      unplacedCount > 0
        ? `<p><strong>Not packed:</strong> ${unplacedCount}</p>`
        : ''
    }
    ${diagnosticsHtml}
    ${summaryHtml}
  `;
}

// Configuration Management
function openConfigModal() {
  const modal = document.getElementById('configModal');
  modal.style.display = 'block';

  renderContainerTypesList();
  renderObjectsList();
  renderConfigValidationSummary();

  const rotationsCheckbox = document.getElementById('allowRotationsCheckbox');
  if (rotationsCheckbox) {
    rotationsCheckbox.checked = !!config.allowRotations;
  }
}

function closeConfigModal() {
  document.getElementById('configModal').style.display = 'none';
}

function renderContainerTypesList() {
  const container = document.getElementById('containerTypesList');
  container.innerHTML = config.containers
    .map(
      (entry, index) => `
    <div class="object-item">
      <div class="object-header">
        <h4>Container Type ${index + 1}</h4>
        <button class="btn-remove" onclick="removeContainerType(${index})">🗑️ Remove</button>
      </div>
      <div class="form-group">
        <label>Label</label>
        <input type="text" value="${
          escapeHtml(entry.name ?? '')
        }" placeholder="Optional" maxlength="60"
               oninput="updateContainerType(${index}, 'name', null, this.value)" />
      </div>
      <div class="form-group">
        <label>Dimensions (W × D × H)</label>
        <div class="form-row">
          <input type="number" value="${entry.dims[0]}" min="1" step="5"
                 onchange="updateContainerType(${index}, 'dims', 0, this.value)" />
          <input type="number" value="${entry.dims[1]}" min="1" step="5"
                 onchange="updateContainerType(${index}, 'dims', 1, this.value)" />
          <input type="number" value="${entry.dims[2]}" min="1" step="5"
                 onchange="updateContainerType(${index}, 'dims', 2, this.value)" />
        </div>
      </div>
      <div class="form-group">
        <label>Maximum Weight (kg)</label>
        <input type="number" value="${entry.maxWeight}" min="0.1" step="5"
               onchange="updateContainerType(${index}, 'maxWeight', null, this.value)" />
      </div>
    </div>
  `
    )
    .join('');
}

window.updateContainerType = function (index, field, subIndex, rawValue) {
  const entry = config.containers[index];
  if (!entry) return;

  if (field === 'dims') {
    const value = parseFloat(rawValue);
    if (!Number.isFinite(value) || value <= 0) {
      showToast('Please enter a positive number for the container dimension.', 'error');
      renderContainerTypesList();
      renderConfigValidationSummary();
      return;
    }
    entry.dims[subIndex] = value;
  } else if (field === 'maxWeight') {
    const value = parseFloat(rawValue);
    if (!Number.isFinite(value) || value <= 0) {
      showToast('Please enter a positive maximum weight.', 'error');
      renderContainerTypesList();
      renderConfigValidationSummary();
      return;
    }
    entry.maxWeight = value;
  } else if (field === 'name') {
    const value = rawValue.trim();
    entry.name = value.length > 0 ? value : null;
  }

  renderConfigValidationSummary();
  renderStatus();
};

window.removeContainerType = function (index) {
  config.containers.splice(index, 1);
  renderContainerTypesList();
  renderConfigValidationSummary();
  renderStatus();
};

function addContainerType() {
  const nextId =
    config.containers.length > 0
      ? Math.max(...config.containers.map((c) => c.id ?? 0)) + 1
      : 1;
  config.containers.push({
    id: nextId,
    name: null,
    dims: [50, 50, 20],
    maxWeight: 250,
  });
  renderContainerTypesList();
  renderConfigValidationSummary();
  renderStatus();
}

function renderObjectsList() {
  const container = document.getElementById('objectsList');
  container.innerHTML = config.objects
    .map(
      (obj, index) => `
    <div class="object-item">
      <div class="object-header">
        <h4>Object ${obj.id}</h4>
        <button class="btn-remove" onclick="removeObject(${index})">🗑️ Remove</button>
      </div>
      <div class="form-group">
        <label>Dimensions (W × D × H)</label>
        <div class="form-row">
          <input type="number" value="${obj.dims[0]}" min="1" step="5"
                 onchange="updateObject(${index}, 'dims', 0, this.value)" />
          <input type="number" value="${obj.dims[1]}" min="1" step="5"
                 onchange="updateObject(${index}, 'dims', 1, this.value)" />
          <input type="number" value="${obj.dims[2]}" min="1" step="5"
                 onchange="updateObject(${index}, 'dims', 2, this.value)" />
        </div>
      </div>
      <div class="form-group">
        <label>Weight (kg)</label>
        <input type="number" value="${obj.weight}" min="0.1" step="1"
               onchange="updateObject(${index}, 'weight', null, this.value)" />
      </div>
    </div>
  `
    )
    .join('');
}

window.updateObject = function (index, field, subIndex, value) {
  if (field === 'dims') {
    const numValue = parseFloat(value);
    if (!Number.isFinite(numValue) || numValue <= 0) {
      showToast('Please enter a positive number for the object dimension.', 'error');
      renderObjectsList();
      renderConfigValidationSummary();
      return;
    }
    config.objects[index].dims[subIndex] = numValue;
  } else if (field === 'weight') {
    const numValue = parseFloat(value);
    if (!Number.isFinite(numValue) || numValue <= 0) {
      showToast('Please enter a positive object weight.', 'error');
      renderObjectsList();
      renderConfigValidationSummary();
      return;
    }
    config.objects[index].weight = numValue;
  }

  renderConfigValidationSummary();
  renderStatus();
};

window.removeObject = function (index) {
  config.objects.splice(index, 1);
  renderObjectsList();
  renderConfigValidationSummary();
  renderStatus();
};

function addObject() {
  const newId = computeNextObjectId();
  config.objects.push({
    id: newId,
    dims: [20, 20, 10],
    weight: 25,
  });
  renderObjectsList();
  renderConfigValidationSummary();
  renderStatus();
}

function saveConfig() {
  if (config.containers.length === 0) {
    showToast('At least one container type is required.', 'error');
    return;
  }

  const invalidContainer = config.containers.find(
    (c) =>
      !Array.isArray(c.dims) ||
      c.dims.length !== 3 ||
      c.dims.some((d) => !Number.isFinite(d) || d <= 0) ||
      !Number.isFinite(c.maxWeight) ||
      c.maxWeight <= 0
  );
  if (invalidContainer) {
    showToast(
      'Please check dimensions and maximum weights of the container types.',
      'error'
    );
    return;
  }

  if (config.objects.length === 0) {
    showToast('At least one object is required.', 'error');
    return;
  }

  const invalidObject = config.objects.find(
    (o) =>
      !Array.isArray(o.dims) ||
      o.dims.length !== 3 ||
      o.dims.some((d) => !Number.isFinite(d) || d <= 0) ||
      !Number.isFinite(o.weight) ||
      o.weight <= 0
  );
  if (invalidObject) {
    showToast('Please check dimensions and weight of the objects.', 'error');
    return;
  }

  persistConfig();
  closeConfigModal();
  renderConfigValidationSummary();
  setStatus({
    mode: 'Idle',
    phase: 'Config saved',
    level: 'success',
    message:
      'Configuration saved locally. It will be restored automatically on the next page load.',
  });
  showToast('Configuration saved locally.', 'success');
  console.log('✅ Configuration saved:', config);
}

function describeContainerType(container, index) {
  const name = container.name?.trim();
  const label = name && name.length ? name : `Container Type ${index + 1}`;
  return `${label} (${container.dims.join(' × ')} | ${container.maxWeight}kg)`;
}

function collectConfigIssues() {
  const issues = [];

  if (config.containers.length === 0) {
    issues.push({
      type: 'config',
      message: 'No container type is defined.',
    });
    return issues;
  }

  config.containers.forEach((container, index) => {
    const invalidDims =
      !Array.isArray(container.dims) ||
      container.dims.length !== 3 ||
      container.dims.some((d) => !Number.isFinite(d) || d <= 0);
    if (invalidDims) {
      issues.push({
        type: 'container',
        index,
        message: `${describeContainerType(
          container,
          index
        )} has invalid dimensions.`,
      });
    }

    if (!Number.isFinite(container.maxWeight) || container.maxWeight <= 0) {
      issues.push({
        type: 'container',
        index,
        message: `${describeContainerType(
          container,
          index
        )} has an invalid maximum weight.`,
      });
    }
  });

  config.objects.forEach((obj) => {
    const validContainers = config.containers.filter((container) => {
      return fitsContainerWithRotation(
        obj.dims,
        container.dims,
        config.allowRotations === true
      );
    });

    if (validContainers.length === 0) {
      issues.push({
        id: obj.id,
        type: 'dimensions',
        details: config.allowRotations
          ? 'No container type offers enough space, even with rotation.'
          : 'No container type offers enough space.',
      });
      return;
    }

    const weightCapable = validContainers.filter(
      (container) => obj.weight <= container.maxWeight + 1e-6
    );

    if (weightCapable.length === 0) {
      issues.push({
        id: obj.id,
        type: 'weight',
        details: 'No container type supports the object weight.',
      });
    }
  });

  return issues;
}

function ensureConfigValidOrNotify() {
  const issues = collectConfigIssues();
  if (issues.length === 0) {
    return true;
  }

  renderConfigValidationSummary();
  openConfigModal();
  showToast(
    `Configuration review needed: ${issues.length} issue(s) require attention before packing.`,
    'warning',
    5000
  );
  setStatus({
    phase: 'Needs review',
    level: 'warning',
    message:
      'The configuration contains issues. Review the warnings in the configuration dialog before starting a run.',
  });
  return false;
}

function renderConfigValidationSummary() {
  const target = document.getElementById('configValidationSummary');
  if (!target) return;

  const issues = collectConfigIssues();
  if (issues.length === 0) {
    target.dataset.state = 'ready';
    target.innerHTML =
      '<strong>Ready:</strong> Containers and objects are valid for the current packing rules.';
    return;
  }

  target.dataset.state = 'warning';
  target.innerHTML = `
    <strong>Review required:</strong> ${issues.length} issue(s) detected.
    <ul class="issue-list">
      ${issues
        .slice(0, 4)
        .map((issue) => {
          switch (issue.type) {
            case 'dimensions':
              return `<li>Object ${escapeHtml(
                issue.id
              )}: does not fit in any container type.</li>`;
            case 'weight':
              return `<li>Object ${escapeHtml(
                issue.id
              )}: exceeds all container weight limits.</li>`;
            case 'container':
              return `<li>${escapeHtml(issue.message)}</li>`;
            case 'config':
              return `<li>${escapeHtml(issue.message)}</li>`;
            default:
              return '<li>Unknown configuration issue.</li>';
          }
        })
        .join('')}
      ${
        issues.length > 4
          ? `<li>…and ${issues.length - 4} more issue(s).</li>`
          : ''
      }
    </ul>
  `;
}

async function fetchPacking() {
  if (!ensureConfigValidOrNotify()) {
    console.info('🚫 Packing operation aborted: configuration issues.');
    return;
  }
  try {
    const payload = {
      containers: config.containers.map((container) => ({
        name: container.name,
        dims: container.dims,
        max_weight: container.maxWeight,
      })),
      objects: config.objects,
      allow_rotations: config.allowRotations === true,
    };
    setStatus({
      mode: 'Batch',
      phase: 'Packing',
      level: 'info',
      message: 'Calculating an optimized batch packing result…',
      placedCount: 0,
      containerCount: 0,
    });
    updateUnplacedPanel([]);
    const response = await fetch('/pack', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(payload),
    });
    if (!response.ok) {
      const errorText = await response.text();
      throw new Error(errorText || `Request failed with status ${response.status}`);
    }
    packingResults = await response.json();
    console.log('✅ Server Response:', packingResults);
    const placedCount = Array.isArray(packingResults.results)
      ? packingResults.results.reduce(
          (sum, container) => sum + (container.placed?.length ?? 0),
          0
        )
      : 0;
    updateUnplacedPanel(packingResults.unplaced ?? []);
    if (
      Array.isArray(packingResults.unplaced) &&
      packingResults.unplaced.length
    ) {
      console.warn('⚠️ Unplaceable objects:', packingResults.unplaced);
      showToast(
        `${packingResults.unplaced.length} object(s) could not be packed. See the panel for details.`,
        'warning',
        5200
      );
    }
    currentContainerIndex = 0;
    showContainer(0);
    updateNavigationButtons();
    setStatus({
      mode: 'Batch',
      phase: packingResults.unplaced?.length ? 'Completed with warnings' : 'Completed',
      level: packingResults.unplaced?.length ? 'warning' : 'success',
      message: packingResults.is_complete
        ? 'Packing completed successfully.'
        : 'Packing finished, but some objects could not be placed.',
      placedCount,
      containerCount: packingResults.results?.length ?? 0,
    });
    if (!packingResults.unplaced?.length) {
      showToast('Packing completed successfully.', 'success');
    }
  } catch (error) {
    console.error('❌ Error:', error);
    setStatus({
      mode: 'Batch',
      phase: 'Failed',
      level: 'error',
      message: 'The server request failed. Check the backend and try again.',
    });
    showToast(error.message || 'Server not reachable.', 'error', 5200);
  }
}

function resolveContainerDims(container) {
  if (
    container &&
    Array.isArray(container.dims) &&
    container.dims.length === 3
  ) {
    return container.dims;
  }
  if (config.containers.length) {
    return config.containers[0].dims;
  }
  return [50, 50, 50];
}

function recomputeLiveDiagnosticsSummary() {
  const diagnosticsList = liveContainers
    .map((c) => c.diagnostics)
    .filter((diag) => diag && typeof diag === 'object');

  if (diagnosticsList.length === 0) {
    liveDiagnosticsSummary = null;
    return;
  }

  let maxImbalance = 0;
  let worstSupport = 100;
  let supportSum = 0;
  let supportCount = 0;

  diagnosticsList.forEach((diag) => {
    if (Number.isFinite(diag.imbalance_ratio)) {
      maxImbalance = Math.max(maxImbalance, diag.imbalance_ratio);
    }
    if (Number.isFinite(diag.minimum_support_percent)) {
      worstSupport = Math.min(worstSupport, diag.minimum_support_percent);
    }
    const samples = Array.isArray(diag.support_samples)
      ? diag.support_samples.filter((sample) =>
          Number.isFinite(sample?.support_percent)
        )
      : [];

    if (samples.length > 0) {
      samples.forEach((sample) => {
        supportSum += sample.support_percent;
        supportCount += 1;
      });
    } else if (Number.isFinite(diag.average_support_percent)) {
      supportSum += diag.average_support_percent;
      supportCount += 1;
    }
  });

  const averageSupport = supportCount > 0 ? supportSum / supportCount : 100;

  liveDiagnosticsSummary = {
    max_imbalance_ratio: maxImbalance,
    worst_support_percent: worstSupport,
    average_support_percent: averageSupport,
  };
}

function focusCameraOnDims(dims) {
  controls.target.set(dims[0] / 2, dims[2] / 2, dims[1] / 2);
}

function showContainer(index) {
  if (liveMode) {
    if (!liveContainers || index < 0 || index >= liveContainers.length) return;
    currentContainerIndex = index;
    const container = liveContainers[index];
    const dims = resolveContainerDims(container);
    visualizeContainer(container, dims);
    focusCameraOnDims(dims);
  } else {
    if (!packingResults || index < 0 || index >= packingResults.results.length)
      return;
    currentContainerIndex = index;
    const container = packingResults.results[index];
    const dims = resolveContainerDims(container);
    visualizeContainer(container, dims);
    focusCameraOnDims(dims);
  }
}

function updateNavigationButtons() {
  const count = liveMode
    ? liveContainers.length
    : packingResults
    ? packingResults.results.length
    : 0;
  document.getElementById('prevContainer').disabled =
    count === 0 || currentContainerIndex === 0;
  document.getElementById('nextContainer').disabled =
    count === 0 || currentContainerIndex === count - 1;
  document.getElementById('animateBtn').disabled = count === 0 || liveMode;
}

function toggleAnimation() {
  const animateBtn = document.getElementById('animateBtn');
  if (isAnimating) {
    clearInterval(animationInterval);
    isAnimating = false;
    animateBtn.textContent = '▶ Start Animation';
  } else {
    if (!packingResults) return;
    isAnimating = true;
    animateBtn.textContent = '⏸ Stop Animation';
    animationStep = 0;
    const container = packingResults.results[currentContainerIndex];
    const containerSize = resolveContainerDims(container);
    animationInterval = setInterval(() => {
      if (animationStep >= container.placed.length) animationStep = 0;
      animateContainer(container, containerSize, animationStep);
      animationStep++;
    }, DEFAULT_ANIMATION_DELAY_MS);
  }
}

// Event Listeners
document.getElementById('configBtn').addEventListener('click', openConfigModal);
document.querySelector('.close').addEventListener('click', closeConfigModal);
document.getElementById('addObjectBtn').addEventListener('click', addObject);
document
  .getElementById('addContainerTypeBtn')
  .addEventListener('click', addContainerType);
document.getElementById('saveConfigBtn').addEventListener('click', saveConfig);
const allowRotationsCheckbox = document.getElementById(
  'allowRotationsCheckbox'
);
if (allowRotationsCheckbox) {
  allowRotationsCheckbox.addEventListener('change', (event) => {
    config.allowRotations = !!event.target.checked;
    renderConfigValidationSummary();
    renderStatus();
  });
}

window.addEventListener('click', (event) => {
  const modal = document.getElementById('configModal');
  if (event.target === modal) {
    closeConfigModal();
  }
});

document.getElementById('runPacking').addEventListener('click', () => {
  if (isAnimating) toggleAnimation();
  liveMode = false;
  fetchPacking();
});
document.getElementById('runPackingLive').addEventListener('click', () => {
  if (isAnimating) toggleAnimation();
  startLivePacking();
});
document.getElementById('prevContainer').addEventListener('click', () => {
  if (isAnimating) toggleAnimation();
  showContainer(currentContainerIndex - 1);
  updateNavigationButtons();
});
document.getElementById('nextContainer').addEventListener('click', () => {
  if (isAnimating) toggleAnimation();
  showContainer(currentContainerIndex + 1);
  updateNavigationButtons();
});
document
  .getElementById('animateBtn')
  .addEventListener('click', toggleAnimation);
document.addEventListener('keydown', (event) => {
  const modalOpen =
    document.getElementById('configModal').style.display === 'block';
  const activeTag = document.activeElement?.tagName?.toLowerCase();
  const isTyping =
    activeTag === 'input' || activeTag === 'textarea' || modalOpen;

  if (event.key === 'Escape') {
    closeConfigModal();
    return;
  }

  if (isTyping) return;

  if (event.key === 'ArrowLeft') {
    event.preventDefault();
    if (!document.getElementById('prevContainer').disabled) {
      showContainer(currentContainerIndex - 1);
      updateNavigationButtons();
    }
  } else if (event.key === 'ArrowRight') {
    event.preventDefault();
    if (!document.getElementById('nextContainer').disabled) {
      showContainer(currentContainerIndex + 1);
      updateNavigationButtons();
    }
  } else if (event.code === 'Space') {
    event.preventDefault();
    if (!document.getElementById('animateBtn').disabled) {
      toggleAnimation();
    }
  } else if (event.key.toLowerCase() === 'b') {
    event.preventDefault();
    document.getElementById('runPacking').click();
  } else if (event.key.toLowerCase() === 'l') {
    event.preventDefault();
    document.getElementById('runPackingLive').click();
  } else if (event.key.toLowerCase() === 'c') {
    event.preventDefault();
    openConfigModal();
  }
});
window.addEventListener('resize', () => {
  camera.aspect = window.innerWidth / window.innerHeight;
  camera.updateProjectionMatrix();
  renderer.setSize(window.innerWidth, window.innerHeight);
});

function animate() {
  requestAnimationFrame(animate);
  controls.update();
  renderer.render(scene, camera);
}
animate();
renderConfigValidationSummary();
renderStatus();
updateUnplacedPanel([]);
console.log('🚀 3D Visualizer ready!');

// --- Live Modus (SSE) ---
function startLivePacking() {
  if (!ensureConfigValidOrNotify()) {
    console.info('🚫 Live packing aborted: configuration issues.');
    return;
  }
  liveMode = true;
  liveContainers = [];
  liveUnplaced = [];
  liveDiagnosticsSummary = null;
  currentContainerIndex = 0;
  updateUnplacedPanel([]);
  setStatus({
    mode: 'Live',
    phase: 'Streaming',
    level: 'info',
    message: 'Waiting for live packing events from the server…',
    placedCount: 0,
    containerCount: 0,
  });
  updateNavigationButtons();

  if (es) {
    es.close();
    es = null;
  }

  const payload = {
    containers: config.containers.map((container) => ({
      name: container.name,
      dims: container.dims,
      max_weight: container.maxWeight,
    })),
    objects: config.objects,
    allow_rotations: config.allowRotations === true,
  };

  fetch('/pack_stream', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(payload),
  })
    .then(async (resp) => {
      if (!resp.ok) {
        throw new Error(`Live packing request failed with status ${resp.status}`);
      }
      if (!resp.body) throw new Error('No stream response');
      const reader = resp.body.getReader();
      const decoder = new TextDecoder('utf-8');
      let buffer = '';
      while (true) {
        const { value, done } = await reader.read();
        if (done) break;
        buffer += decoder.decode(value, { stream: true });
        // SSE frames are separated by \n\n
        let parts = buffer.split(/\n\n/);
        buffer = parts.pop() || '';
        for (const part of parts) {
          const line = part.trim();
          if (!line) continue;
          // Expected: data: {json}
          const dataLine = line.startsWith('data:')
            ? line.slice(5).trim()
            : line;
          try {
            const evt = JSON.parse(dataLine);
            handleLiveEvent(evt);
          } catch (e) {
            console.warn('SSE parse warn:', e, line);
          }
        }
      }
      handleLiveEvent({ type: 'Finished' });
    })
    .catch((e) => {
      console.warn('POST /pack_stream error:', e);
      setStatus({
        mode: 'Live',
        phase: 'Failed',
        level: 'error',
        message: 'Live stream could not be started. Please verify the backend.',
      });
      showToast(e.message || 'Live stream could not be started.', 'error', 5200);
    });
}

function handleLiveEvent(evt) {
  switch (evt.type) {
    case 'ContainerStarted': {
      const dims = Array.isArray(evt.dims)
        ? evt.dims
        : resolveContainerDims(null);
      liveContainers.push({
        id: evt.id,
        total_weight: 0,
        placed: [],
        dims,
        max_weight: evt.max_weight,
        label: evt.label ?? null,
        template_id: evt.template_id ?? null,
        diagnostics: null,
      });
      currentContainerIndex = liveContainers.length - 1;
      visualizeContainer(liveContainers[currentContainerIndex], dims);
      focusCameraOnDims(dims);
      updateNavigationButtons();
      setStatus({
        mode: 'Live',
        phase: 'Streaming',
        level: 'info',
        message: `Container ${liveContainers.length} is now receiving objects.`,
        placedCount: liveContainers.reduce(
          (sum, container) => sum + container.placed.length,
          0
        ),
        containerCount: liveContainers.length,
      });
      break;
    }
    case 'ObjectPlaced': {
      const idx = evt.container_id - 1;
      while (liveContainers.length <= idx) {
        liveContainers.push({
          id: liveContainers.length + 1,
          total_weight: 0,
          placed: [],
          dims: resolveContainerDims(null),
          max_weight: evt.max_weight ?? null,
          label: evt.label ?? null,
          template_id: evt.template_id ?? null,
          diagnostics: null,
        });
      }
      const cont = liveContainers[idx];
      cont.placed.push({
        id: evt.id,
        pos: evt.pos,
        weight: evt.weight,
        dims: evt.dims,
      });
      cont.total_weight = evt.total_weight;
      if (idx === currentContainerIndex) {
        visualizeContainer(cont, resolveContainerDims(cont), evt.id);
        focusCameraOnDims(resolveContainerDims(cont));
      }
      updateNavigationButtons();
      setStatus({
        mode: 'Live',
        phase: 'Streaming',
        level: 'info',
        message: `Placed object ${evt.id} into container ${evt.container_id}.`,
        placedCount: liveContainers.reduce(
          (sum, container) => sum + container.placed.length,
          0
        ),
        containerCount: liveContainers.length,
      });
      break;
    }
    case 'ContainerDiagnostics': {
      const idx = evt.container_id - 1;
      const diagnostics = evt.diagnostics ?? null;
      if (idx >= 0 && idx < liveContainers.length && diagnostics) {
        liveContainers[idx].diagnostics = diagnostics;
        recomputeLiveDiagnosticsSummary();
        if (idx === currentContainerIndex) {
          updateStats(
            liveContainers[idx],
            resolveContainerDims(liveContainers[idx])
          );
        }
      }
      break;
    }
    case 'ObjectRejected': {
      liveUnplaced.push(evt);
      console.warn(
        `⚠️ Object ${evt.id} could not be packed (${evt.reason_text})`
      );
      updateUnplacedPanel(liveUnplaced);
      setStatus({
        mode: 'Live',
        phase: 'Warning',
        level: 'warning',
        message: `Object ${evt.id} could not be packed and was moved to the unplaced list.`,
        placedCount: liveContainers.reduce(
          (sum, container) => sum + container.placed.length,
          0
        ),
        containerCount: liveContainers.length,
      });
      updateNavigationButtons();
      break;
    }
    case 'Finished': {
      if (evt.diagnostics_summary) {
        liveDiagnosticsSummary = evt.diagnostics_summary;
      } else {
        recomputeLiveDiagnosticsSummary();
      }
      updateUnplacedPanel(liveUnplaced);
      if (liveContainers.length) {
        updateStats(
          liveContainers[currentContainerIndex],
          resolveContainerDims(liveContainers[currentContainerIndex])
        );
      }
      setStatus({
        mode: 'Live',
        phase: liveUnplaced.length ? 'Completed with warnings' : 'Completed',
        level: liveUnplaced.length ? 'warning' : 'success',
        message: liveUnplaced.length
          ? 'Live packing finished with some unplaced objects.'
          : 'Live packing finished successfully.',
        placedCount: liveContainers.reduce(
          (sum, container) => sum + container.placed.length,
          0
        ),
        containerCount: liveContainers.length,
      });
      showToast(
        liveUnplaced.length
          ? `${liveUnplaced.length} object(s) remained unpacked after the live run.`
          : 'Live packing finished successfully.',
        liveUnplaced.length ? 'warning' : 'success',
        5200
      );
      break;
    }
  }
}
