<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { readDir } from '@tauri-apps/plugin-fs';
  import { open } from '@tauri-apps/plugin-dialog';
  import type { ImageInfo } from '../types';

  let imageFolder: string = '';
  let outputFolder: string = '';
  let images: ImageInfo[] = [];

  // UI state
  let loading = false;          // true while processing the whole batch
  let totalImages = 0;          // total number of images to process
  let processedCount = 0;       // how many have been processed so far
  let copyInsteadOfMove = false;

  /** Prompt the user to select a folder that contains images. */
  async function selectImageFolder() {
    const selected = await open({ directory: true, multiple: false });
    if (Array.isArray(selected) || selected === null) return;
    imageFolder = selected;
    await loadImages();
  }

  /** Prompt the user to select the destination folder. */
  async function selectOutputFolder() {
    const selected = await open({ directory: true, multiple: false });
    if (Array.isArray(selected) || selected === null) return;
    outputFolder = selected;
  }

  /** Scan the selected folder, read metadata and QR codes **one image at a time**. */
  async function loadImages() {
    if (!imageFolder) return;
    loading = true;
    images = [];
    processedCount = 0;

    try {
      // Recursively collect image paths (basic filter for common extensions)
      const entries = await readDir(imageFolder);
      const paths = entries
        .filter(e => !e.children && /\.(jpe?g|png|tiff?|bmp|heif|heic)$/i.test(e.path))
        .map(e => e.path);

      totalImages = paths.length;

      // Process each image sequentially so the progress bar reflects real work.
      for (const p of paths) {
        const info = await invoke<ImageInfo>('process_image', { imagePath: p });
        images = [...images, info]; // trigger reactivity
        processedCount += 1;
      }
    } catch (e) {
      console.error('Error processing images:', e);
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
    // Ensure every row has a QR code – simple validation
    const missing = images.filter(i => !i.qr_code.trim());
    if (missing.length) {
      alert(`There are ${missing.length} images without a QR code. Please fill them in.`);
      return;
    }

    try {
      await invoke('move_images', {
        images,
        outputDir: outputFolder,
        copyInsteadOfMove: copyInsteadOfMove
      });
      alert('Export completed!');
    } catch (e) {
      console.error('Export failed:', e);
      alert('Export failed – see console for details.');
    }
  }

  /** Simple helper to format latitude/longitude. */
  function formatCoord(coord: number | null): string {
    return coord === null ? '' : coord.toFixed(6);
  }
</script>

<style>
  .container { padding: 1rem; font-family: sans-serif; }
  .toolbar { margin-bottom: 1rem; }
  button { margin-right: 0.5rem; }
  table { width: 100%; border-collapse: collapse; }
  th, td { border: 1px solid #ddd; padding: 0.5rem; text-align: left; }
  th { background: #f4f4f4; }
  img.thumb { width: 80px; height: auto; }
  input.qr { width: 100%; }
  progress { width: 100%; height: 1.5rem; margin-top: 0.5rem; }
</style>

<div class="container">
  <h1>QR‑Code Image Sorter</h1>

  <div class="toolbar">
    <button on:click={selectImageFolder}>📂 Choose Image Folder</button>
    <button on:click={selectOutputFolder}>📁 Choose Output Folder</button>
    <label>
      <input type="checkbox" bind:checked={copyInsteadOfMove} />
      Copy instead of move
    </label>
    <button on:click={exportImages} disabled={loading || !images.length}>🚚 Export</button>
  </div>

  {#if loading}
    <p>Processing images… please wait.</p>
    <progress max={totalImages} value={processedCount}></progress>
  {:else if images.length}
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
  {:else}
    <p>No images loaded yet.</p>
  {/if}
</div>
