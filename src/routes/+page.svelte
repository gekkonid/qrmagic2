<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { ask, open } from '@tauri-apps/plugin-dialog';
  import type { ImageInfo } from '../types';

  let imageFolder = $state('');
  let outputFolder = $state('');
  let images = $state<ImageInfo[]>([]);
  let excluded = $state<boolean[]>([]);

  let status = $state('');
  let loading = $state(false);
  let totalImages = $state(0);
  let processedCount = $state(0);
  let copyInsteadOfMove = $state(false);

  // Lightbox state
  let lightboxSrc = $state('');
  let lightboxAlt = $state('');
  let lightboxIndex = $state(-1);

  async function selectImageFolder() {
    const selected = await open({ directory: true, multiple: false });
    if (selected === null) return;
    imageFolder = selected;
    await loadImages();
  }

  async function selectOutputFolder() {
    const selected = await open({ directory: true, multiple: false });
    if (selected === null) return;
    outputFolder = selected;
  }

  async function loadImages() {
    if (!imageFolder) return;
    loading = true;
    images = [];
    processedCount = 0;
    totalImages = 0;
    status = 'Scanning for images…';

    try {
      const paths = await invoke<string[]>('list_images', { directory: imageFolder });
      totalImages = paths.length;

      if (paths.length === 0) {
        status = `No images found in ${imageFolder}`;
        return;
      }

      status = `Found ${paths.length} image(s). Scanning all images...`;

      // Listen for per-image progress events from the parallel backend
      const unlisten = await listen<ImageInfo>('image-processed', () => {
        processedCount += 1;
      });

      // Backend processes all images in parallel and returns the full list
      const results = await invoke<ImageInfo[]>('process_images', { paths });
      unlisten();

      // Backend returns results sorted by camera_hash then date
      images = results;
      excluded = new Array(results.length).fill(false);

      status = `Done — processed ${images.length} image(s).`;
    } catch (e) {
      console.error('Error processing images:', e);
      status = `Error: ${e}`;
    } finally {
      loading = false;
    }
  }

  async function exportImages() {
    if (!outputFolder) {
      alert('Please select an output folder first.');
      return;
    }
    const toExport = images.filter((_, i) => !excluded[i]);
    const missing = toExport.filter(i => !i.qr_code.trim());
    if (missing.length) {
      const proceed = await ask(
        `There are ${missing.length} images without a QR code. Press No to abort export and fill them in, or Yes to continue.`,
        { title: 'Missing QR codes', kind: 'warning' }
      );
      if (!proceed) return;
    }

    try {
      await invoke('move_images', {
        images: toExport,
        outputDir: outputFolder,
        copyInsteadOfMove,
      });
      alert('Export completed!');
    } catch (e) {
      console.error('Export failed:', e);
      alert(`Export failed: ${e}`);
    }
  }

  function focusOnMount(node: HTMLElement) { node.focus(); }

  // --- Lightbox ---
  async function openLightbox(index: number) {
    lightboxIndex = index;
    const img = images[index];
    lightboxAlt = img.name;
    lightboxSrc = img.thumbnail;
    const fullSrc = await invoke<string>('load_full_image', { imagePath: img.path });
    // Only update if we're still viewing the same image
    if (lightboxIndex === index) {
      lightboxSrc = fullSrc;
    }
  }

  function closeLightbox() {
    lightboxSrc = '';
    lightboxIndex = -1;
  }

  function lightboxKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') { closeLightbox(); return; }
    if (e.key === 'ArrowLeft' && lightboxIndex > 0) {
      openLightbox(lightboxIndex - 1);
    } else if (e.key === 'ArrowRight' && lightboxIndex < images.length - 1) {
      openLightbox(lightboxIndex + 1);
    }
  }

  // --- QR fill from neighbour ---
  function parseDate(d: string): number | null {
    if (!d) return null;
    // EXIF dates look like "2024-01-15 13:45:02" or "2024:01:15 13:45:02"
    const normalized = d.replace(/^(\d{4}):(\d{2}):(\d{2})/, '$1-$2-$3');
    const ms = Date.parse(normalized);
    return isNaN(ms) ? null : ms;
  }

  function isNeighbour(a: ImageInfo, b: ImageInfo): boolean {
    if (a.camera_hash !== b.camera_hash) return false;
    const dateA = parseDate(a.date);
    const dateB = parseDate(b.date);
    if (dateA === null || dateB === null) return false;
    return Math.abs(dateA - dateB) <= 30_000;
  }

  function fillQrFromNeighbour(index: number) {
    if (images[index].qr_code.trim()) return;

    // Check previous neighbour
    if (index > 0 && images[index - 1].qr_code.trim() && isNeighbour(images[index], images[index - 1])) {
      images[index].qr_code = images[index - 1].qr_code;
      return;
    }

    // Check next neighbour
    if (index < images.length - 1 && images[index + 1].qr_code.trim() && isNeighbour(images[index], images[index + 1])) {
      images[index].qr_code = images[index + 1].qr_code;
    }
  }

  function autoFillAll() {
    // Forward pass: propagate from earlier images to later ones
    for (let i = 0; i < images.length; i++) {
      if (images[i].qr_code.trim()) continue;
      if (i > 0 && images[i - 1].qr_code.trim() && isNeighbour(images[i], images[i - 1])) {
        images[i].qr_code = images[i - 1].qr_code;
      }
    }
    // Backward pass: propagate from later images to earlier ones
    for (let i = images.length - 2; i >= 0; i--) {
      if (images[i].qr_code.trim()) continue;
      if (images[i + 1].qr_code.trim() && isNeighbour(images[i], images[i + 1])) {
        images[i].qr_code = images[i + 1].qr_code;
      }
    }
  }
</script>

<style>
  .container { padding: 1rem; font-family: sans-serif; }
  .toolbar { margin-bottom: 1rem; display: flex; align-items: center; gap: 0.5rem; flex-wrap: wrap; }
  table { width: 100%; border-collapse: collapse; }
  th, td { border: 1px solid #ddd; padding: 0.5rem; text-align: left; }
  th { background: #f4f4f4; }
  img.thumb { width: 80px; height: auto; }
  .thumb-btn { background: none; border: none; padding: 0; cursor: pointer; }
  input.qr { width: 100%; }
  .progress-section { margin: 1rem 0; }
  progress { width: 100%; height: 1.5rem; }
  .status { margin: 0.5rem 0; color: #555; }
  tr.missing-qr { background: #fff3cd; }
  tr.excluded { opacity: 0.4; }
  td.exclude-cell { width: 1%; white-space: nowrap; }
  .exclude-btn { cursor: pointer; font-size: 1.2rem; opacity: 0.3; padding: 0.2rem 0.4rem; }
  .exclude-btn.active { opacity: 1; }

  /* Lightbox */
  .lightbox-overlay {
    position: fixed; inset: 0;
    background: rgba(0, 0, 0, 0.85);
    display: flex; align-items: center; justify-content: center;
    z-index: 1000; cursor: pointer;
  }
  .lightbox-overlay img {
    max-width: 90vw; max-height: 90vh;
    object-fit: contain;
    border-radius: 4px;
    box-shadow: 0 0 40px rgba(0,0,0,0.5);
  }
</style>

<!-- Lightbox -->
{#if lightboxSrc}
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div class="lightbox-overlay" onclick={closeLightbox} onkeydown={lightboxKeydown} role="button" tabindex="0" use:focusOnMount>
    <img src={lightboxSrc} alt={lightboxAlt} />
  </div>
{/if}

<div class="container">
  <h1>QRMagic2</h1>
  <p>Gekkonid Scientific</p>

  <div class="toolbar">
    <button onclick={selectImageFolder} disabled={loading}>Choose Image Folder</button>
    <button onclick={selectOutputFolder}>Choose Output Folder</button>
    <label>
      <input type="checkbox" bind:checked={copyInsteadOfMove} />
      Copy instead of move
    </label>
    <button onclick={autoFillAll} disabled={loading || !images.length}>Auto-fill all</button>
    <button onclick={exportImages} disabled={loading || !images.length}>Export</button>
  </div>

  {#if imageFolder}
    <p class="status">Source: {imageFolder}</p>
  {/if}
  {#if outputFolder}
    <p class="status">Destination: {outputFolder}</p>
  {/if}

  {#if status}
    <p class="status">{status}</p>
  {/if}

  {#if loading && totalImages > 0}
    <div class="progress-section">
      <progress max={totalImages} value={processedCount}></progress>
      <p>{processedCount} / {totalImages}</p>
    </div>
  {/if}

  {#if images.length > 0}
    <table>
      <thead>
        <tr>
          <th>Thumb</th>
          <th></th>
          <th>QR Code (editable)</th>
          <th>Date</th>
          <th>Location</th>
        </tr>
      </thead>
      <tbody>
        {#each images as img, i}
          <tr class:missing-qr={!img.qr_code.trim() && !excluded[i]} class:excluded={excluded[i]}>
            <td><button class="thumb-btn" tabindex="-1" onclick={() => openLightbox(i)}><img class="thumb" src={img.thumbnail} alt={img.name} /></button></td>
            <td class="exclude-cell"><button class="exclude-btn" class:active={excluded[i]} tabindex="-1" title="Exclude from export" onclick={() => excluded[i] = !excluded[i]}>🗑</button></td>
            <td>
              <input
                class="qr"
                bind:value={img.qr_code}
                placeholder="Enter QR…"
                onfocus={() => fillQrFromNeighbour(i)}
              />
            </td>
            <td>{img.date}</td>
            <td>
              {#if img.latitude !== null && img.longitude !== null}
                <a href="https://www.openstreetmap.org/#map=12/{img.latitude.toFixed(6)}/{img.longitude.toFixed(6)}" target="_blank">{img.latitude.toFixed(6)} {img.longitude.toFixed(6)}</a>
              {/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</div>
