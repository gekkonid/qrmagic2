<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { open } from '@tauri-apps/plugin-dialog';
  import type { ImageInfo } from '../types';

  let imageFolder = $state('');
  let outputFolder = $state('');
  let images = $state<ImageInfo[]>([]);

  // UI state
  let status = $state('');
  let loading = $state(false);
  let totalImages = $state(0);
  let processedCount = $state(0);
  let copyInsteadOfMove = $state(false);

  /** Prompt the user to select a folder that contains images. */
  async function selectImageFolder() {
    const selected = await open({ directory: true, multiple: false });
    if (selected === null) return;
    imageFolder = selected;
    await loadImages();
  }

  /** Prompt the user to select the destination folder. */
  async function selectOutputFolder() {
    const selected = await open({ directory: true, multiple: false });
    if (selected === null) return;
    outputFolder = selected;
  }

  /** Scan the selected folder, read metadata and QR codes one image at a time. */
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

      status = `Found ${paths.length} image(s). Processing…`;

      for (const p of paths) {
        try {
          const info = await invoke<ImageInfo>('process_image', { imagePath: p });
          images.push(info);
        } catch (e) {
          console.warn(`Skipping ${p}:`, e);
        }
        processedCount += 1;
      }

      status = `Done — processed ${images.length} image(s).`;
    } catch (e) {
      console.error('Error listing images:', e);
      status = `Error: ${e}`;
    } finally {
      loading = false;
    }
  }

  /** Send the final list to the backend for moving/copying. */
  async function exportImages() {
    if (!outputFolder) {
      alert('Please select an output folder first.');
      return;
    }
    const missing = images.filter(i => !i.qr_code.trim());
    if (missing.length) {
      alert(`There are ${missing.length} images without a QR code. Please fill them in.`);
      return;
    }

    try {
      await invoke('move_images', {
        images,
        outputDir: outputFolder,
        copyInsteadOfMove,
      });
      alert('Export completed!');
    } catch (e) {
      console.error('Export failed:', e);
      alert(`Export failed: ${e}`);
    }
  }

  function formatCoord(coord: number | null): string {
    return coord === null ? '' : coord.toFixed(6);
  }
</script>

<style>
  .container { padding: 1rem; font-family: sans-serif; }
  .toolbar { margin-bottom: 1rem; display: flex; align-items: center; gap: 0.5rem; flex-wrap: wrap; }
  table { width: 100%; border-collapse: collapse; }
  th, td { border: 1px solid #ddd; padding: 0.5rem; text-align: left; }
  th { background: #f4f4f4; }
  img.thumb { width: 80px; height: auto; }
  input.qr { width: 100%; }
  .progress-section { margin: 1rem 0; }
  progress { width: 100%; height: 1.5rem; }
  .status { margin: 0.5rem 0; color: #555; }
</style>

<div class="container">
  <h1>QR-Code Image Sorter</h1>

  <div class="toolbar">
    <button onclick={selectImageFolder} disabled={loading}>Choose Image Folder</button>
    <button onclick={selectOutputFolder} disabled={loading}>Choose Output Folder</button>
    <label>
      <input type="checkbox" bind:checked={copyInsteadOfMove} />
      Copy instead of move
    </label>
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
          <th>QR Code (editable)</th>
          <th>Date</th>
          <th>Lat</th>
          <th>Long</th>
          <th>Camera Serial</th>
        </tr>
      </thead>
      <tbody>
        {#each images as img}
          <tr>
            <td><img class="thumb" src={img.thumbnail} alt="thumb" /></td>
            <td><input class="qr" bind:value={img.qr_code} placeholder="Enter QR…" /></td>
            <td>{img.date}</td>
            <td>{formatCoord(img.latitude)}</td>
            <td>{formatCoord(img.longitude)}</td>
            <td>{img.camera_serial}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</div>
