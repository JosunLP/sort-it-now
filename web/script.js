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

// Konfigurierbare Parameter
let config = {
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

function computeNextObjectId() {
  return (
    config.objects.reduce((max, obj) => {
      const id = Number.isFinite(obj.id) ? obj.id : 0;
      return id > max ? id : max;
    }, 0) + 1
  );
}

function dimsAlmostEqual(a, b, epsilon = 1e-9) {
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
      w <= containerDims[0] + 1e-6 &&
      d <= containerDims[1] + 1e-6 &&
      h <= containerDims[2] + 1e-6
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

function drawBox(obj, color, opacity = 1.0) {
  const [x, y, z] = obj.pos;
  const [dx, dy, dz] = obj.dims;
  const geometry = new THREE.BoxGeometry(dx, dz, dy);
  const material = new THREE.MeshStandardMaterial({
    color,
    opacity,
    transparent: opacity < 1.0,
    metalness: 0.3,
    roughness: 0.7,
  });
  const cube = new THREE.Mesh(geometry, material);
  cube.position.set(x + dx / 2, z + dz / 2, y + dy / 2);
  scene.add(cube);
}

const COLOR_PALETTE = [
  0xff5555, 0x55ff55, 0x5555ff, 0xffcc00, 0x00ffff, 0xff00ff, 0xffff55,
  0xaa55ff, 0x55ffaa, 0xff7755, 0x77ff55, 0x5577ff, 0xffaa00, 0x00aaff,
  0xaa00ff, 0x55aaff, 0xaaff55, 0xff55aa, 0x55aaff, 0xffaa55, 0x55ffaa,
];

function visualizeContainer(container, containerDims) {
  clearScene();
  drawContainerFrame(...containerDims);
  const sortedObjects = [...container.placed].sort(
    (a, b) => a.pos[2] - b.pos[2]
  );
  sortedObjects.forEach((obj, i) =>
    drawBox(obj, COLOR_PALETTE[i % COLOR_PALETTE.length], 1.0)
  );
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
      i === step ? 0.7 : 1.0
    )
  );
  updateStats(container, containerDims, step + 1);
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
    : `${liveMode ? 'Live-Container' : 'Container'} ${
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
    <p><strong>Schwerpunkt-Abstand:</strong> ${offsetText}</p>
    <p><strong>Unterstützung:</strong> Ø ${formatPlainPercent(
      diagnostics.average_support_percent
    )} · min ${formatPlainPercent(diagnostics.minimum_support_percent)}</p>
  `
    : '';

  const summaryHtml = summary
    ? `
      <hr />
      <p><strong>Diagnostik (gesamt):</strong></p>
      <p>Max. Ungleichgewicht: ${formatPercent(summary.max_imbalance_ratio)}</p>
      <p>Unterstützung Ø / min: ${formatPlainPercent(
        summary.average_support_percent
      )} · ${formatPlainPercent(summary.worst_support_percent)}</p>
    `
    : '';

  document.getElementById('stats').innerHTML = `
    <h3>${containerTitle} / ${
    liveMode
      ? liveContainers.length || 1
      : packingResults
      ? packingResults.results.length
      : 1
  }
    </h3>
    <p><strong>Abmessungen:</strong> ${dims.join(' × ')}</p>
    <p><strong>Objekte:</strong> ${objectCount} / ${container.placed.length}</p>
    <p><strong>Gewicht:</strong> ${totalWeight.toFixed(2)} kg${
    maxWeight ? ` / ${maxWeight} kg` : ''
  }</p>
    <p><strong>Auslastung:</strong> ${utilization}%</p>
    ${
      unplacedCount > 0
        ? `<p><strong>Nicht verpackt:</strong> ${unplacedCount}</p>`
        : ''
    }
    ${diagnosticsHtml}
    ${summaryHtml}
  `;
}

// Konfiguration Management
function openConfigModal() {
  const modal = document.getElementById('configModal');
  modal.style.display = 'block';

  renderContainerTypesList();
  renderObjectsList();

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
        <h4>Verpackungstyp ${index + 1}</h4>
        <button class="btn-remove" onclick="removeContainerType(${index})">🗑️ Entfernen</button>
      </div>
      <div class="form-group">
        <label>Bezeichnung</label>
        <input type="text" value="${
          entry.name ?? ''
        }" placeholder="Optional" maxlength="60"
               oninput="updateContainerType(${index}, 'name', null, this.value)" />
      </div>
      <div class="form-group">
        <label>Dimensionen (B × T × H)</label>
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
        <label>Maximales Gewicht (kg)</label>
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
      alert('Bitte gib eine positive Zahl für die Dimension an.');
      renderContainerTypesList();
      return;
    }
    entry.dims[subIndex] = value;
  } else if (field === 'maxWeight') {
    const value = parseFloat(rawValue);
    if (!Number.isFinite(value) || value <= 0) {
      alert('Bitte gib ein positives Maximalgewicht an.');
      renderContainerTypesList();
      return;
    }
    entry.maxWeight = value;
  } else if (field === 'name') {
    const value = rawValue.trim();
    entry.name = value.length > 0 ? value : null;
  }
};

window.removeContainerType = function (index) {
  config.containers.splice(index, 1);
  renderContainerTypesList();
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
}

function renderObjectsList() {
  const container = document.getElementById('objectsList');
  container.innerHTML = config.objects
    .map(
      (obj, index) => `
    <div class="object-item">
      <div class="object-header">
        <h4>Objekt ${obj.id}</h4>
        <button class="btn-remove" onclick="removeObject(${index})">🗑️ Entfernen</button>
      </div>
      <div class="form-group">
        <label>Dimensionen (B × T × H)</label>
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
        <label>Gewicht (kg)</label>
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
      alert('Bitte gib eine positive Zahl für die Objekt-Dimension an.');
      renderObjectsList();
      return;
    }
    config.objects[index].dims[subIndex] = numValue;
  } else if (field === 'weight') {
    const numValue = parseFloat(value);
    if (!Number.isFinite(numValue) || numValue <= 0) {
      alert('Bitte gib ein positives Objektgewicht an.');
      renderObjectsList();
      return;
    }
    config.objects[index].weight = numValue;
  }
};

window.removeObject = function (index) {
  config.objects.splice(index, 1);
  renderObjectsList();
};

function addObject() {
  const newId = computeNextObjectId();
  config.objects.push({
    id: newId,
    dims: [20, 20, 10],
    weight: 25,
  });
  renderObjectsList();
}

function saveConfig() {
  if (config.containers.length === 0) {
    alert('Mindestens ein Verpackungstyp ist erforderlich!');
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
    alert('Bitte prüfe Dimensionen und Gewichte der Verpackungstypen.');
    return;
  }

  if (config.objects.length === 0) {
    alert('Mindestens ein Objekt erforderlich!');
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
    alert('Bitte prüfe Dimensionen und Gewicht der Objekte.');
    return;
  }

  closeConfigModal();
  console.log('✅ Konfiguration gespeichert:', config);
}

function describeContainerType(container, index) {
  const name = container.name?.trim();
  const label = name && name.length ? name : `Verpackungstyp ${index + 1}`;
  return `${label} (${container.dims.join(' × ')} | ${container.maxWeight}kg)`;
}

function collectConfigIssues() {
  const issues = [];

  if (config.containers.length === 0) {
    issues.push({
      type: 'config',
      message: 'Es ist kein Verpackungstyp definiert.',
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
        )} hat ungültige Dimensionen.`,
      });
    }

    if (!Number.isFinite(container.maxWeight) || container.maxWeight <= 0) {
      issues.push({
        type: 'container',
        index,
        message: `${describeContainerType(
          container,
          index
        )} hat ein ungültiges Maximalgewicht.`,
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
          ? 'Kein Verpackungstyp bietet ausreichend Platz, selbst mit Rotation.'
          : 'Kein Verpackungstyp bietet ausreichend Platz.',
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
        details: 'Kein Verpackungstyp trägt das Objektgewicht.',
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

  const message =
    '⚠️ Die aktuelle Konfiguration enthält Probleme:\n\n' +
    issues
      .map((issue) => {
        switch (issue.type) {
          case 'dimensions':
            return `Objekt ${issue.id}: passt in keinen Verpackungstyp (${issue.details}).`;
          case 'weight':
            return `Objekt ${issue.id}: überschreitet alle Maximalgewichte (${issue.details}).`;
          case 'container':
            return `Verpackung: ${issue.message}`;
          case 'config':
            return issue.message;
          default:
            return 'Unbekanntes Problem in der Konfiguration.';
        }
      })
      .join('\n') +
    '\n\nBitte passe Container oder Objekte an. Möchtest du trotzdem mit der Berechnung fortfahren?';

  return window.confirm(message);
}

async function fetchPacking() {
  if (!ensureConfigValidOrNotify()) {
    console.info('🚫 Pack-Vorgang abgebrochen: Konfigurationsprobleme.');
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
    const response = await fetch('http://localhost:8080/pack', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(payload),
    });
    packingResults = await response.json();
    console.log('✅ Server Response:', packingResults);
    if (
      Array.isArray(packingResults.unplaced) &&
      packingResults.unplaced.length
    ) {
      console.warn('⚠️ Nicht platzierbare Objekte:', packingResults.unplaced);
      alert(
        `⚠️ Warnung: ${packingResults.unplaced.length} Objekt(e) konnten nicht verpackt werden!\n\n` +
          packingResults.unplaced
            .map((u) => `Objekt ${u.id}: ${u.reason}`)
            .join('\n')
      );
    }
    currentContainerIndex = 0;
    showContainer(0);
    updateNavigationButtons();
  } catch (error) {
    console.error('❌ Fehler:', error);
    alert('Server nicht erreichbar!');
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
    animateBtn.textContent = '▶ Animation starten';
  } else {
    if (!packingResults) return;
    isAnimating = true;
    animateBtn.textContent = '⏸ Animation stoppen';
    animationStep = 0;
    const container = packingResults.results[currentContainerIndex];
    const containerSize = resolveContainerDims(container);
    animationInterval = setInterval(() => {
      if (animationStep >= container.placed.length) animationStep = 0;
      animateContainer(container, containerSize, animationStep);
      animationStep++;
    }, 800);
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
console.log('🚀 3D Visualizer bereit!');

// --- Live Modus (SSE) ---
function startLivePacking() {
  if (!ensureConfigValidOrNotify()) {
    console.info('🚫 Live-Pack-Vorgang abgebrochen: Konfigurationsprobleme.');
    return;
  }
  liveMode = true;
  liveContainers = [];
  liveUnplaced = [];
  liveDiagnosticsSummary = null;
  currentContainerIndex = 0;
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

  fetch('http://localhost:8080/pack_stream', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(payload),
  })
    .then(async (resp) => {
      if (!resp.body) throw new Error('Keine Stream-Response');
      const reader = resp.body.getReader();
      const decoder = new TextDecoder('utf-8');
      let buffer = '';
      while (true) {
        const { value, done } = await reader.read();
        if (done) break;
        buffer += decoder.decode(value, { stream: true });
        // SSE frames sind durch \n\n getrennt
        let parts = buffer.split(/\n\n/);
        buffer = parts.pop() || '';
        for (const part of parts) {
          const line = part.trim();
          if (!line) continue;
          // Erwartet: data: {json}
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
    .catch((e) => console.warn('POST /pack_stream Fehler:', e));
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
        visualizeContainer(cont, resolveContainerDims(cont));
        focusCameraOnDims(resolveContainerDims(cont));
      }
      updateNavigationButtons();
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
        `⚠️ Objekt ${evt.id} konnte nicht verpackt werden (${evt.reason_text})`
      );
      updateNavigationButtons();
      break;
    }
    case 'Finished': {
      if (typeof evt.unplaced === 'number' && evt.unplaced > 0) {
        console.warn(`⚠️ Gesamt unverpackt: ${evt.unplaced}`);
        alert(
          `⚠️ Warnung: ${evt.unplaced} Objekt(e) konnten nicht verpackt werden!\n\n` +
            liveUnplaced
              .map((u) => `Objekt ${u.id}: ${u.reason_text}`)
              .join('\n')
        );
      }
      if (evt.diagnostics_summary) {
        liveDiagnosticsSummary = evt.diagnostics_summary;
      } else {
        recomputeLiveDiagnosticsSummary();
      }
      if (liveContainers.length) {
        updateStats(
          liveContainers[currentContainerIndex],
          resolveContainerDims(liveContainers[currentContainerIndex])
        );
      }
      break;
    }
  }
}
