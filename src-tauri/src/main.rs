#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use std::fmt;
use std::fs;
use std::path::Path;

use base64::Engine;
use exif::{In, Reader as ExifReader, Tag};
use serde::{Deserialize, Serialize};

#[derive(Debug)]
enum Error {
    Io(std::io::Error),
    Exif(exif::Error),
    Image(image::ImageError),
    Other(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "{}", e),
            Error::Exif(e) => write!(f, "{}", e),
            Error::Image(e) => write!(f, "{}", e),
            Error::Other(s) => write!(f, "{}", s),
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<exif::Error> for Error {
    fn from(e: exif::Error) -> Self {
        Error::Exif(e)
    }
}

impl From<image::ImageError> for Error {
    fn from(e: image::ImageError) -> Self {
        Error::Image(e)
    }
}

impl From<String> for Error {
    fn from(s: String) -> Self {
        Error::Other(s)
    }
}

impl Serialize for Error {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

/// Information about a single image that will be sent to the frontend.
#[derive(Debug, Serialize, Deserialize, Clone)]
struct ImageInfo {
    /// Full path to the original image file.
    path: String,
    /// File name only (used for display and later moving).
    name: String,
    /// Thumbnail as a base64-encoded PNG data URL.
    thumbnail: String,
    /// Detected QR code (empty string if none found).
    qr_code: String,
    /// Capture date in ISO-8601 format (or empty string).
    date: String,
    /// Latitude in decimal degrees (or null).
    latitude: Option<f64>,
    /// Longitude in decimal degrees (or null).
    longitude: Option<f64>,
    /// Camera serial number / unique ID (or empty).
    camera_serial: String,
}

const IMAGE_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "tif", "tiff", "bmp", "heif", "heic",
];

fn is_image_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| IMAGE_EXTENSIONS.iter().any(|&e| e.eq_ignore_ascii_case(ext)))
        .unwrap_or(false)
}

/// Recursively list all image file paths under a directory.
#[tauri::command]
async fn list_images(directory: String) -> Result<Vec<String>, Error> {
    let dir = Path::new(&directory);
    if !dir.is_dir() {
        return Err(Error::Other(format!("Not a directory: {}", directory)));
    }
    let mut results = Vec::new();
    collect_images(dir, &mut results)?;
    results.sort();
    Ok(results)
}

fn collect_images(dir: &Path, results: &mut Vec<String>) -> Result<(), Error> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_images(&path, results)?;
        } else if is_image_file(&path) {
            results.push(path.to_string_lossy().to_string());
        }
    }
    Ok(())
}

/// Process a **single** image and return its metadata. This command is intended to be
/// called repeatedly from the frontend so that a progress bar can be displayed.
#[tauri::command]
async fn process_image(image_path: String) -> Result<ImageInfo, Error> {
    let path = Path::new(&image_path);
    if !path.is_file() {
        return Err(Error::Other(format!("Path is not a file: {}", image_path)));
    }

    // ---- 1. Read EXIF metadata -------------------------------------------------
    let file = fs::File::open(path)?;
    let mut bufreader = std::io::BufReader::new(&file);
    let exif = ExifReader::new().read_from_container(&mut bufreader).ok();

    // Helper closure to fetch a tag as string
    let get_tag = |tag: Tag| {
        exif.as_ref()
            .and_then(|exif| exif.get_field(tag, In::PRIMARY))
            .map(|field| field.display_value().with_unit(exif.as_ref().unwrap()).to_string())
    };

    // Date
    let date = get_tag(Tag::DateTimeOriginal).unwrap_or_default();

    // Camera serial (many cameras store it under BodySerialNumber or SerialNumber)
    let camera_serial = get_tag(Tag::BodySerialNumber).unwrap_or_default();

    // GPS
    let (latitude, longitude) = if exif
        .as_ref()
        .and_then(|e| e.get_field(Tag::GPSLatitude, In::PRIMARY))
        .is_some()
    {
        // Helper to convert rational vec to decimal degrees
        let to_deg = |tag| {
            exif.as_ref()
                .and_then(|e| e.get_field(tag, In::PRIMARY))
                .and_then(|f| {
                    if let exif::Value::Rational(ref vec) = f.value {
                        if vec.len() == 3 {
                            return Some(vec[0].to_f64() + vec[1].to_f64() / 60.0 + vec[2].to_f64() / 3600.0);
                        }
                    }
                    None
                })
        };

        let mut lat = to_deg(Tag::GPSLatitude);
        let mut lon = to_deg(Tag::GPSLongitude);

        // Apply sign based on N/S/E/W
        if let Some(ref f) = exif
            .as_ref()
            .and_then(|e| e.get_field(Tag::GPSLatitudeRef, In::PRIMARY))
        {
            if f.value.display_as(Tag::GPSLatitudeRef).to_string() == "S" {
                lat = lat.map(|v| -v);
            }
        }
        if let Some(ref f) = exif
            .as_ref()
            .and_then(|e| e.get_field(Tag::GPSLongitudeRef, In::PRIMARY))
        {
            if f.value.display_as(Tag::GPSLongitudeRef).to_string() == "W" {
                lon = lon.map(|v| -v);
            }
        }

        (lat, lon)
    } else {
        (None, None)
    };

    // ---- 2. Generate thumbnail -------------------------------------------------
    let img = image::open(path)?;
    let thumbnail = img.thumbnail(120, 120);
    let mut thumb_buf = Vec::new();
    thumbnail
        .write_to(
            &mut std::io::Cursor::new(&mut thumb_buf),
            image::ImageFormat::Png,
        )?;
    let thumbnail_base64 = base64::engine::general_purpose::STANDARD.encode(&thumb_buf);
    let thumbnail_data_url = format!("data:image/png;base64,{}", thumbnail_base64);

    // ---- 3. Scan for QR code ---------------------------------------------------
    let decoder = bardecoder::default_decoder();
    let qr_codes = decoder.decode(&img);
    let qr_code = qr_codes
        .into_iter()
        .filter_map(|r| r.ok())
        .next()
        .unwrap_or_default();

    // ---- 4. Assemble result ----------------------------------------------------
    Ok(ImageInfo {
        path: image_path.clone(),
        name: path.file_name().unwrap().to_string_lossy().to_string(),
        thumbnail: thumbnail_data_url,
        qr_code,
        date,
        latitude,
        longitude,
        camera_serial,
    })
}

#[tauri::command]
async fn move_images(
    images: Vec<ImageInfo>,
    output_dir: String,
    copy_instead_of_move: bool,
) -> Result<(), Error> {
    let out_dir = Path::new(&output_dir);
    if !out_dir.is_dir() {
        return Err(Error::Other(format!(
            "Output directory does not exist: {}",
            output_dir
        )));
    }

    for img in images {
        // Skip entries without a QR code – UI should enforce filling them.
        if img.qr_code.trim().is_empty() {
            continue;
        }

        let target_dir = out_dir.join(&img.qr_code);
        fs::create_dir_all(&target_dir)?;

        let src_path = Path::new(&img.path);
        let dest_path = target_dir.join(&img.name);

        if copy_instead_of_move {
            fs::copy(&src_path, &dest_path)?;
        } else {
            fs::rename(&src_path, &dest_path)?;
        }
    }

    Ok(())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![list_images, process_image, move_images])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
