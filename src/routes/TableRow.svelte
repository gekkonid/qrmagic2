<script lang="ts">
  import type { ImageInfo } from '../types';
  import { openUrl } from '@tauri-apps/plugin-opener';

  let {
    img,
    index,
    excluded = $bindable(false),
    onOpenLightbox,
    onFillQr,
  }: {
    img: ImageInfo;
    index: number;
    excluded: boolean;
    onOpenLightbox: (index: number) => void;
    onFillQr: (index: number) => void;
  } = $props();

  function handleLocationClick(e: MouseEvent, lat: number, lon: number) {
    e.preventDefault();
    openUrl(`https://www.openstreetmap.org/?mlat=${lat.toFixed(6)}&mlon=${lon.toFixed(6)}#map=12/${lat.toFixed(6)}/${lon.toFixed(6)}`);
  }
</script>

<style>
  tr.missing-qr { background: #fff3cd; }
  tr.excluded { opacity: 0.4; }
  td { border: 1px solid #ddd; padding: 0.5rem; text-align: left; }
  td.exclude-cell { width: 1%; white-space: nowrap; }
  .exclude-btn { cursor: pointer; font-size: 1.2rem; opacity: 0.3; padding: 0.2rem 0.4rem; }
  .exclude-btn.active { opacity: 1; }
  .thumb-btn { background: none; border: none; padding: 0; cursor: pointer; }
  img.thumb { width: 80px; height: auto; }
  .filename { font-size: 0.7rem; color: #666; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 100px; }
  input.qr { width: 100%; }
</style>

<tr class:missing-qr={!img.qr_code.trim() && !excluded} class:excluded={excluded}>
  <td><button class="thumb-btn" tabindex="-1" onclick={() => onOpenLightbox(index)}><img class="thumb" src={img.thumbnail} alt={img.name} /></button><div class="filename">{img.name}</div></td>
  <td class="exclude-cell"><button class="exclude-btn" class:active={excluded} tabindex="-1" title="Exclude from export" onclick={() => excluded = !excluded}>🗑</button></td>
  <td>
    <input
      class="qr"
      bind:value={img.qr_code}
      placeholder="Enter QR…"
      onfocus={() => onFillQr(index)}
    />
  </td>
  <td>{img.date}</td>
  <td>
    {#if img.latitude !== null && img.longitude !== null}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <a
        href="https://www.openstreetmap.org/?mlat={img.latitude.toFixed(6)}&mlon={img.longitude.toFixed(6)}#map=12/{img.latitude.toFixed(6)}/{img.longitude.toFixed(6)}"
        tabindex="-1"
        onclick={(e) => handleLocationClick(e, img.latitude!, img.longitude!)}
        onkeydown={() => {}}
      >{img.latitude.toFixed(6)} {img.longitude.toFixed(6)}</a>
    {/if}
  </td>
</tr>
