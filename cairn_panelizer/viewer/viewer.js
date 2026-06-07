/* Cairn Trillium Dome viewer — 3D assembly + flat-pattern cut sheets.
 *
 * Reads window.CAIRN_VIEWER_DATA (written by cairn_panelizer.viewer_data) and
 * renders two synchronized views: a Three.js 3D dome colored by mold family,
 * and a 2D canvas of nested flat-pattern cut sheets with SVG export. Clicking
 * a panel in either view selects it in both and shows its schedule details.
 */
(function () {
  "use strict";

  const data = window.CAIRN_VIEWER_DATA;
  if (!data) {
    document.querySelector("main").innerHTML =
      '<p style="padding:1.5rem">No viewer data found. Run ' +
      '<code>python scripts/generate_trillium_dome.py</code> first — it writes ' +
      '<code>viewer/viewer_data.js</code> — then reload this page.</p>';
    return;
  }

  // ---------------------------------------------------------------------
  // Family color palette — golden-ratio HSV hashing, mirrors visualize.py
  // so the same family always reads as the same color across the toolchain.
  // ---------------------------------------------------------------------
  const familyColors = {};
  function familyColor(familyId) {
    if (familyId === null || familyId === undefined) return "#8a93a1";
    if (!(familyId in familyColors)) {
      const hue = (Object.keys(familyColors).length * 0.6180339887) % 1.0;
      familyColors[familyId] = hsvToHex(hue, 0.55, 0.92);
    }
    return familyColors[familyId];
  }
  function hsvToHex(h, s, v) {
    const i = Math.floor(h * 6);
    const f = h * 6 - i;
    const p = v * (1 - s);
    const q = v * (1 - f * s);
    const t = v * (1 - (1 - f) * s);
    let r, g, b;
    switch (i % 6) {
      case 0: r = v; g = t; b = p; break;
      case 1: r = q; g = v; b = p; break;
      case 2: r = p; g = v; b = t; break;
      case 3: r = p; g = q; b = v; break;
      case 4: r = t; g = p; b = v; break;
      default: r = v; g = p; b = q; break;
    }
    const toHex = (x) => Math.round(x * 255).toString(16).padStart(2, "0");
    return "#" + toHex(r) + toHex(g) + toHex(b);
  }
  // Pre-seed in family order so the legend and geometry agree from frame one.
  data.families.forEach(familyColor);

  const panelsById = {};
  data.panels.forEach((p) => { panelsById[p.panel_id] = p; });

  // ---------------------------------------------------------------------
  // Header stats + family legend
  // ---------------------------------------------------------------------
  const meta = data.meta;
  document.getElementById("meta-stats").textContent =
    `${meta.overall_width_mm}mm wide × ${meta.height_mm}mm tall · ${meta.lobe_count} lobes · ` +
    `${meta.fabricated_panel_count} panels · ${meta.family_count} mold families · ` +
    `${meta.mold_count} molds · ${meta.sheet_count} cut sheet(s)`;

  const legend = document.getElementById("family-legend");
  const LEGEND_LIMIT = 36;
  data.families.slice(0, LEGEND_LIMIT).forEach((fid) => {
    const row = document.createElement("div");
    row.className = "legend-row";
    row.innerHTML = `<span class="swatch" style="background:${familyColor(fid)}"></span>${fid}`;
    legend.appendChild(row);
  });
  if (data.families.length > LEGEND_LIMIT) {
    const more = document.createElement("div");
    more.className = "legend-more";
    more.textContent = `+ ${data.families.length - LEGEND_LIMIT} more families`;
    legend.appendChild(more);
  }

  // ---------------------------------------------------------------------
  // Shared selection -> info panel
  // ---------------------------------------------------------------------
  let onSelectPanel = null; // set once both views register their highlighters

  function selectPanel(panelId) {
    const panel = panelsById[panelId];
    if (!panel) return;
    showPanelDetails(panel);
    if (onSelectPanel) onSelectPanel(panel);
  }

  function showPanelDetails(panel) {
    const details = document.getElementById("panel-details");
    if (!panel) {
      details.innerHTML = "<p>Click a panel — in either view — to inspect it.</p>";
      return;
    }
    const edges = panel.edge_lengths_mm.map((e) => e.toFixed(0)).join(" / ");
    details.innerHTML = `
      <dl>
        <dt>Panel ID</dt><dd>${panel.panel_id}</dd>
        <dt>Type</dt><dd>${panel.panel_type}</dd>
        <dt>Mold family</dt><dd>${panel.family_id || "—"}</dd>
        <dt>Area</dt><dd>${panel.area_mm2.toFixed(0)} mm²</dd>
        <dt>Edge lengths</dt><dd>${edges} mm</dd>
        <dt>Centroid</dt><dd>${panel.centroid.map((c) => c.toFixed(0)).join(", ")} mm</dd>
        <dt>Neighbors (${panel.neighbor_ids.length})</dt><dd>${panel.neighbor_ids.join(", ") || "—"}</dd>
      </dl>`;
  }

  // ---------------------------------------------------------------------
  // 3D dome view
  // ---------------------------------------------------------------------
  function initDomeView() {
    const container = document.getElementById("dome-canvas");
    const fabricated = data.panels.filter((p) => !p.is_opening);

    const scene = new THREE.Scene();
    scene.background = new THREE.Color(0x12161c);

    const camera = new THREE.PerspectiveCamera(45, container.clientWidth / container.clientHeight, 1, 1e6);
    const renderer = new THREE.WebGLRenderer({ antialias: true });
    renderer.setPixelRatio(window.devicePixelRatio || 1);
    renderer.setSize(container.clientWidth, container.clientHeight);
    container.appendChild(renderer.domElement);

    const controls = new THREE.OrbitControls(camera, renderer.domElement);
    controls.enableDamping = true;
    controls.dampingFactor = 0.08;

    scene.add(new THREE.AmbientLight(0xffffff, 0.55));
    const sun = new THREE.DirectionalLight(0xffffff, 0.85);
    sun.position.set(meta.overall_width_mm, meta.overall_width_mm, meta.overall_width_mm);
    scene.add(sun);
    const fill = new THREE.DirectionalLight(0xffffff, 0.35);
    fill.position.set(-meta.overall_width_mm, meta.overall_width_mm * 0.5, -meta.overall_width_mm);
    scene.add(fill);

    // World convention here: dome model is X/Y-plane footprint with Z up;
    // three.js is Y-up, so we map model (x, y, z) -> scene (x, z, -y).
    const toScene = ([x, y, z]) => new THREE.Vector3(x, z, -y);

    const positions = new Float32Array(fabricated.length * 9);
    const colors = new Float32Array(fabricated.length * 9);
    fabricated.forEach((panel, i) => {
      const color = new THREE.Color(familyColor(panel.family_id));
      for (let v = 0; v < 3; v++) {
        const p = toScene(panel.vertices[v]);
        positions[i * 9 + v * 3 + 0] = p.x;
        positions[i * 9 + v * 3 + 1] = p.y;
        positions[i * 9 + v * 3 + 2] = p.z;
        colors[i * 9 + v * 3 + 0] = color.r;
        colors[i * 9 + v * 3 + 1] = color.g;
        colors[i * 9 + v * 3 + 2] = color.b;
      }
    });

    const geometry = new THREE.BufferGeometry();
    geometry.setAttribute("position", new THREE.BufferAttribute(positions, 3));
    geometry.setAttribute("color", new THREE.BufferAttribute(colors, 3));
    geometry.computeVertexNormals();

    const material = new THREE.MeshLambertMaterial({ vertexColors: true, side: THREE.DoubleSide });
    const mesh = new THREE.Mesh(geometry, material);
    scene.add(mesh);

    const seamGeometry = new THREE.WireframeGeometry(geometry);
    const seams = new THREE.LineSegments(
      seamGeometry,
      new THREE.LineBasicMaterial({ color: 0x05070a, transparent: true, opacity: 0.35 })
    );
    scene.add(seams);

    const highlightGeometry = new THREE.BufferGeometry();
    const highlight = new THREE.LineLoop(
      highlightGeometry,
      new THREE.LineBasicMaterial({ color: 0xffffff, linewidth: 2 })
    );
    highlight.visible = false;
    highlight.renderOrder = 10;
    scene.add(highlight);

    function highlightPanel(panel) {
      const pts = panel.vertices.map(toScene);
      pts.push(pts[0].clone());
      highlightGeometry.setFromPoints(pts);
      highlight.visible = true;
    }
    onSelectPanel = highlightPanel;

    const radius = meta.overall_width_mm;
    camera.position.set(radius * 0.85, radius * 0.7, radius * 0.85);
    controls.target.set(0, meta.height_mm * 0.4, 0);
    controls.update();

    const raycaster = new THREE.Raycaster();
    const pointer = new THREE.Vector2();
    renderer.domElement.addEventListener("click", (event) => {
      const rect = renderer.domElement.getBoundingClientRect();
      pointer.x = ((event.clientX - rect.left) / rect.width) * 2 - 1;
      pointer.y = -((event.clientY - rect.top) / rect.height) * 2 + 1;
      raycaster.setFromCamera(pointer, camera);
      const hits = raycaster.intersectObject(mesh);
      if (hits.length === 0 || hits[0].faceIndex === undefined) return;
      selectPanel(fabricated[hits[0].faceIndex].panel_id);
    });

    function onResize() {
      camera.aspect = container.clientWidth / container.clientHeight;
      camera.updateProjectionMatrix();
      renderer.setSize(container.clientWidth, container.clientHeight);
    }
    window.addEventListener("resize", onResize);

    (function animate() {
      requestAnimationFrame(animate);
      controls.update();
      renderer.render(scene, camera);
    })();
  }

  // ---------------------------------------------------------------------
  // Flat-pattern (nested cut sheet) view
  // ---------------------------------------------------------------------
  let currentSheetIndex = 0;
  let selectedPanelId = null;

  function initFlatView() {
    const select = document.getElementById("sheet-select");
    data.sheets.forEach((sheet, i) => {
      const opt = document.createElement("option");
      opt.value = String(i);
      opt.textContent = `${sheet.sheet_id} — ${sheet.panel_count} panels, ${sheet.utilization_pct.toFixed(1)}% utilized`;
      select.appendChild(opt);
    });
    select.addEventListener("change", () => {
      currentSheetIndex = parseInt(select.value, 10);
      drawFlatSheet();
    });
    document.getElementById("export-svg").addEventListener("click", exportCurrentSheetSvg);

    const previousHighlighter = onSelectPanel;
    onSelectPanel = (panel) => {
      if (previousHighlighter) previousHighlighter(panel);
      selectedPanelId = panel.panel_id;
      const sheetIndex = data.sheets.findIndex((s) => s.panels.some((p) => p.panel_id === panel.panel_id));
      if (sheetIndex >= 0 && sheetIndex !== currentSheetIndex) {
        currentSheetIndex = sheetIndex;
        select.value = String(sheetIndex);
      }
      drawFlatSheet();
    };

    drawFlatSheet();
  }

  function drawFlatSheet() {
    const sheet = data.sheets[currentSheetIndex];
    const canvas = document.getElementById("flat-canvas");
    const wrap = document.getElementById("flat-canvas-wrap");
    if (!sheet) {
      canvas.width = canvas.height = 0;
      return;
    }

    const scale = Math.max(
      0.05,
      Math.min((wrap.clientWidth - 32) / sheet.width_mm, (wrap.clientHeight - 32) / sheet.height_mm)
    );
    canvas.width = Math.round(sheet.width_mm * scale);
    canvas.height = Math.round(sheet.height_mm * scale);

    const ctx = canvas.getContext("2d");
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    ctx.strokeStyle = "#9aa5b1";
    ctx.lineWidth = 1;
    ctx.strokeRect(0.5, 0.5, canvas.width - 1, canvas.height - 1);

    sheet.panels.forEach((panel) => {
      const isSelected = panel.panel_id === selectedPanelId;
      ctx.beginPath();
      panel.outline_mm.forEach(([x, y], i) => {
        const px = x * scale;
        const py = canvas.height - y * scale;
        if (i === 0) ctx.moveTo(px, py); else ctx.lineTo(px, py);
      });
      ctx.closePath();
      ctx.fillStyle = familyColor(panel.family_id);
      ctx.globalAlpha = isSelected ? 1.0 : 0.8;
      ctx.fill();
      ctx.globalAlpha = 1.0;
      ctx.strokeStyle = isSelected ? "#ffffff" : "#22262e";
      ctx.lineWidth = isSelected ? 2.5 : 1;
      ctx.stroke();

      if (scale > 0.18) {
        const cx = panel.outline_mm.reduce((a, [x]) => a + x, 0) / panel.outline_mm.length;
        const cy = panel.outline_mm.reduce((a, [, y]) => a + y, 0) / panel.outline_mm.length;
        ctx.fillStyle = "#1b1f27";
        ctx.font = "9px sans-serif";
        ctx.textAlign = "center";
        ctx.textBaseline = "middle";
        ctx.fillText(panel.panel_id, cx * scale, canvas.height - cy * scale);
      }
    });

    canvas.onclick = (event) => {
      const rect = canvas.getBoundingClientRect();
      const mx = (event.clientX - rect.left) / scale;
      const my = (canvas.height - (event.clientY - rect.top)) / scale;
      const hit = sheet.panels.find((panel) => pointInPolygon([mx, my], panel.outline_mm));
      if (hit) selectPanel(hit.panel_id);
    };
  }

  function pointInPolygon([px, py], polygon) {
    let inside = false;
    for (let i = 0, j = polygon.length - 1; i < polygon.length; j = i++) {
      const [xi, yi] = polygon[i];
      const [xj, yj] = polygon[j];
      const crosses = (yi > py) !== (yj > py) &&
        px < ((xj - xi) * (py - yi)) / (yj - yi) + xi;
      if (crosses) inside = !inside;
    }
    return inside;
  }

  function exportCurrentSheetSvg() {
    const sheet = data.sheets[currentSheetIndex];
    if (!sheet) return;

    const parts = [
      `<svg xmlns="http://www.w3.org/2000/svg" width="${sheet.width_mm}" height="${sheet.height_mm}" ` +
        `viewBox="0 0 ${sheet.width_mm} ${sheet.height_mm}">`,
      `<rect x="0" y="0" width="${sheet.width_mm}" height="${sheet.height_mm}" fill="#ffffff" stroke="#888888"/>`,
    ];
    sheet.panels.forEach((panel) => {
      const pts = panel.outline_mm
        .map(([x, y]) => `${x.toFixed(2)},${(sheet.height_mm - y).toFixed(2)}`)
        .join(" ");
      const cx = panel.outline_mm.reduce((a, [x]) => a + x, 0) / panel.outline_mm.length;
      const cy = sheet.height_mm - panel.outline_mm.reduce((a, [, y]) => a + y, 0) / panel.outline_mm.length;
      parts.push(
        `<polygon points="${pts}" fill="${familyColor(panel.family_id)}" fill-opacity="0.85" ` +
          `stroke="#22262e" stroke-width="0.5"/>`
      );
      parts.push(
        `<text x="${cx.toFixed(2)}" y="${cy.toFixed(2)}" font-size="9" text-anchor="middle" ` +
          `dominant-baseline="middle" fill="#1b1f27">${panel.panel_id}</text>`
      );
    });
    parts.push("</svg>");

    const blob = new Blob([parts.join("\n")], { type: "image/svg+xml" });
    const url = URL.createObjectURL(blob);
    const link = document.createElement("a");
    link.href = url;
    link.download = `${sheet.sheet_id}_flat_pattern.svg`;
    link.click();
    URL.revokeObjectURL(url);
  }

  // ---------------------------------------------------------------------
  // View tab switching
  // ---------------------------------------------------------------------
  document.querySelectorAll("#view-tabs button").forEach((btn) => {
    btn.addEventListener("click", () => {
      document.querySelectorAll("#view-tabs button").forEach((b) => b.classList.remove("active"));
      document.querySelectorAll("main .view").forEach((v) => v.classList.remove("active"));
      btn.classList.add("active");
      document.getElementById(`${btn.dataset.view}-view`).classList.add("active");
      if (btn.dataset.view === "flat") drawFlatSheet();
    });
  });

  initDomeView();
  initFlatView();
  window.addEventListener("resize", () => drawFlatSheet());
})();
