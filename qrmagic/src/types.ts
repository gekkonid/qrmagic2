/** TypeScript definition that mirrors the Rust `ImageInfo` struct. */
export interface ImageInfo {
  /** Full absolute path to the original image file. */
  path: string;
  /** File name only (e.g., "IMG_0012.jpg"). */
  name: string;
  /** Base64‑encoded PNG data URL for a 120×120 thumbnail. */
  thumbnail: string;
  /** Detected QR code string – may be empty if not found. */
  qr_code: string;
  /** Capture date in ISO‑8601 format (or empty string). */
  date: string;
  /** Latitude in decimal degrees (null if not present). */
  latitude: number | null;
  /** Longitude in decimal degrees (null if not present). */
  longitude: number | null;
  /** Camera serial / unique ID (or empty string). */
  camera_serial: string;
}
