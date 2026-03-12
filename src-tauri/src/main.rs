//! Tauri backend for the QR‑code image sorter.
//! Provides two commands:
//! 1. `process_images` – reads metadata, scans QR codes and returns a list of ImageInfo.
//! 2. `move_images`   – moves/copies images into `$output_dir/<qr_code>/<original_name>`.

#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use std::path::{Path, PathBuf};
use std::fs;
use std::io::Write;

use serde::Serialize;
use tauri::api::dialog::FileDialogBuilder;
use tauri::{Manager, State};

use exif::{Reader as ExifReader, Tag, In};
use image::GenericImageView;
use bardecoder::default_decoder;

/// Information about a single image that will be sent to the frontend.
#[derive(Debug, Serialize, Clone)]
struct ImageInfo {
    /// Full path to the original image file.
    path: String,
    /// File name only (used for display and later moving).
    name: String,
    /// Thumbnail as a base64‑encoded PNG data URL.
    thumbnail: String,
    /// Detected QR code (empty string if none found).
    qr_code: String,
    /// Capture date in ISO‑8601 format (or empty if missing).
    date: String,
    /// Latitude in decimal degrees (or null).
    latitude: Option<f64>,
    /// Longitude in decimal degrees (or null).
    longitude: Option<f64>,
    /// Camera serial number / unique ID (or empty).
    camera_serial: String,
}

/// Shared configuration (e.g., whether to copy instead of move).
struct Config {
    copy_instead_of_move: bool,
}

#[tauri::command]
async fn process_images(
    image_paths: Vec<String>,
) -> Result<Vec<ImageInfo>, String> {
    let mut results = Vec::new();

    for img_path in image_paths {
        let path = Path::new(&img_path);
        if !path.is_file() {
            continue;
        }

        // ---- 1. Read EXIF metadata -------------------------------------------------
        let file = fs::File::open(path).map_err(|e| format!("Failed to open {}: {}", img_path, e))?;
        let mut bufreader = std::io::BufReader::new(&file);
        let exif = ExifReader::new()
            .read_from_container(&mut bufreader)
            .ok();

        // Helper closures
        let get_tag = |tag| {
            exif.as_ref()
                .and_then(|exif| exif.get_field(tag, In::PRIMARY))
                .map(|field| field.display_value().with_unit(exif).to_string())
        };

        // Date
        let date = get_tag(Tag::DateTimeOriginal).unwrap_or_default();

        // Camera serial (many cameras store it under Tag::BodySerialNumber or Tag::SerialNumber)
        let camera_serial = get_tag(Tag::BodySerialNumber)
            .or_else(|| get_tag(Tag::SerialNumber))
            .unwrap_or_default();

        // GPS
        let (latitude, longitude) = if let Some(gps) = exif.as_ref().and_then(|e| e.get_field(Tag::GPSLatitude, In::PRIMARY)) {
            // Use the `exif` crate's helper to convert to decimal degrees
            let lat = exif
                .as_ref()
                .and_then(|e| e.get_field(Tag::GPSLatitude, In::PRIMARY))
                .and_then(|f| f.value.get_rational_vec().ok())
                .and_then(|vec| {
                    if vec.len() == 3 {
                        Some(
                            (vec[0].to_f64() + vec[1].to_f64() / 60.0 + vec[2].to_f64() / 3600.0)
                        )
                    } else {
                        None
                    }
                });

            let lon = exif
                .as_ref()
                .and_then(|e| e.get_field(Tag::GPSLongitude, In::PRIMARY))
                .and_then(|f| f.value.get_rational_vec().ok())
                .and_then(|vec| {
                    if vec.len() == 3 {
                        Some(
                            (vec[0].to_f64() + vec[1].to_f64() / 60.0 + vec[2].to_f64() / 3600.0)
                        )
                    } else {
                        None
                    }
                });

            // Apply sign based on N/S/E/W
            let lat = match exif
                .as_ref()
                .and_then(|e| e.get_field(Tag::GPSLatitudeRef, In::PRIMARY))
                .map(|f| f.value.display_as(Tag::GPSLatitudeRef).to_string())
                .as_deref()
            {
                Some("S") => lat.map(|v| -v),
                _ => lat,
            };

            let lon = match exif
                .as_ref()
                .and_then(|e| e.get_field(Tag::GPSLongitudeRef, In::PRIMARY))
                .map(|f| f.value.display_as(Tag::GPSLongitudeRef).to_string())
                .as_deref()
            {
                Some("W") => lon.map(|v| -v),
                _ => lon,
            };

            (lat, lon)
        } else {
            (None, None)
        };

        // ---- 2. Generate thumbnail -------------------------------------------------
        let img = image::open(path).map_err(|e| format!("Failed to load image {}: {}", img_path, e))?;
        let thumbnail = img.thumbnail(120, 120);
        let mut thumb_buf = Vec::new();
        thumbnail
            .write_to(&mut std::io::Cursor::new(&mut thumb_buf), image::ImageOutputFormat::Png)
            .map_err(|e| format!("Failed to encode thumbnail: {}", e))?;
        let thumbnail_base64 = base64::encode(&thumb_buf);
        let thumbnail_data_url = format!("data:image/png;base64,{}", thumbnail_base64);

        // ---- 3. Scan for QR code ---------------------------------------------------
        let decoder = default_decoder();
        let gray = img.to_luma8();
        let qr_codes = decoder.decode(&gray);
        let qr_code = qr_codes
            .into_iter()
            .next()
            .map(|bytes| String::from_utf8_lossy(&bytes).to_string())
            .unwrap_or_default();

        // ---- 4. Assemble result ----------------------------------------------------
        results.push(ImageInfo {
            path: img_path.clone(),
            name: path.file_name().unwrap().to_string_lossy().to_string(),
            thumbnail: thumbnail_data_url,
            qr_code,
            date,
            latitude,
            longitude,
            camera_serial,
        });
    }

    // Sort by camera serial then by date (oldest → newest)
    results.sort_by(|a, b| {
        let cam_cmp = a.camera_serial.cmp(&b.camera_serial);
        if cam_cmp == std::cmp::Ordering::Equal {
            a.date.cmp(&b.date)
        } else {
            cam_cmp
        }
    });

    Ok(results)
}

#[tauri::command]
async fn move_images(
    images: Vec<ImageInfo>,
    output_dir: String,
    copy_instead_of_move: bool,
) -> Result<(), String> {
    let out_dir = Path::new(&output_dir);
    if !out_dir.is_dir() {
        return Err(format!("Output directory does not exist: {}", output_dir));
    }

    for img in images {
        // If the QR code is still empty, skip – the UI should force the user to fill it.
        if img.qr_code.trim().is_empty() {
            continue;
        }

        let target_dir = out_dir.join(&img.qr_code);
        fs::create_dir_all(&target_dir)
            .map_err(|e| format!("Failed to create dir {:?}: {}", target_dir, e))?;

        let src_path = Path::new(&img.path);
        let dest_path = target_dir.join(&img.name);

        if copy_instead_of_move {
            fs::copy(&src_path, &dest_path)
                .map_err(|e| format!("Failed to copy {:?} → {:?}: {}", src_path, dest_path, e))?;
        } else {
            fs::rename(&src_path, &dest_path)
                .map_err(|e| format!("Failed to move {:?} → {:?}: {}", src_path, dest_path, e))?;
        }
    }

    Ok(())
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![process_images, move_images])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
