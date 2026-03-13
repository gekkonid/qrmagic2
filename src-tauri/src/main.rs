#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use std::collections::hash_map::DefaultHasher;
use std::fmt;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::Path;

use base64::Engine;
use exif::{In, Reader as ExifReader, Tag};
use image::{DynamicImage, GenericImageView};
use rayon::prelude::*;
use rxing::{BarcodeFormat, Reader};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

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

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ImageInfo {
    path: String,
    name: String,
    thumbnail: String,
    qr_code: String,
    date: String,
    latitude: Option<f64>,
    longitude: Option<f64>,
    camera_hash: String,
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

/// Try to decode a QR code from a DynamicImage using rxing (Rust port of ZXing).
/// Returns the first successfully decoded string, or None.
fn try_decode_qr(img: &DynamicImage) -> Option<String> {
    let hints = rxing::DecodeHints {
        PossibleFormats: Some([BarcodeFormat::QR_CODE].into_iter().collect()),
        TryHarder: Some(true),
        ..Default::default()
    };
    let source = rxing::BufferedImageLuminanceSource::new(img.clone());
    let mut bitmap = rxing::BinaryBitmap::new(rxing::common::HybridBinarizer::new(source));
    let mut reader = rxing::MultiFormatReader::default();
    match reader.decode_with_hints(&mut bitmap, &hints) {
        Ok(result) => {
            let text = result.getText().to_string();
            if text.is_empty() { None } else { Some(text) }
        }
        Err(_) => None,
    }
}

/// Scale a DynamicImage by a factor (e.g. 0.5 = half size).
fn scale_image(img: &DynamicImage, factor: f64) -> DynamicImage {
    let (w, h) = img.dimensions();
    let nw = (w as f64 * factor).round() as u32;
    let nh = (h as f64 * factor).round() as u32;
    img.resize(nw, nh, image::imageops::FilterType::Lanczos3)
}

/// Apply PIL-style sharpness enhancement.
/// factor < 1 = blur, factor = 1 = original, factor > 1 = sharpen.
/// Blends between a blurred version and the original: result = blurred + factor * (original - blurred)
fn adjust_sharpness(img: &DynamicImage, factor: f64) -> DynamicImage {
    if (factor - 1.0).abs() < 0.01 {
        return img.clone();
    }
    // Use a mild Gaussian blur as the "smoothed" base (PIL uses a 3x3 smooth kernel)
    let blurred = img.blur(1.5);
    let orig_buf = img.to_rgba8();
    let blur_buf = blurred.to_rgba8();
    let (w, h) = orig_buf.dimensions();
    let mut out = image::RgbaImage::new(w, h);
    for (x, y, orig_px) in orig_buf.enumerate_pixels() {
        let blur_px = blur_buf.get_pixel(x, y);
        let mut channels = [0u8; 4];
        for c in 0..4 {
            let blended = blur_px[c] as f64 + factor * (orig_px[c] as f64 - blur_px[c] as f64);
            channels[c] = blended.round().clamp(0.0, 255.0) as u8;
        }
        out.put_pixel(x, y, image::Rgba(channels));
    }
    DynamicImage::ImageRgba8(out)
}

/// Apply autocontrast: linearly stretch pixel values so that the darkest maps to 0
/// and the lightest to 255, per channel.
fn autocontrast(img: &DynamicImage) -> DynamicImage {
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    // Find per-channel min/max (RGB only, skip alpha)
    let mut mins = [255u8; 3];
    let mut maxs = [0u8; 3];
    for px in rgba.pixels() {
        for c in 0..3 {
            mins[c] = mins[c].min(px[c]);
            maxs[c] = maxs[c].max(px[c]);
        }
    }
    let mut out = image::RgbaImage::new(w, h);
    for (x, y, px) in rgba.enumerate_pixels() {
        let mut channels = [0u8; 4];
        for c in 0..3 {
            let range = maxs[c] as f64 - mins[c] as f64;
            if range > 0.0 {
                channels[c] = (((px[c] as f64 - mins[c] as f64) / range) * 255.0).round() as u8;
            } else {
                channels[c] = px[c];
            }
        }
        channels[3] = px[3]; // preserve alpha
        out.put_pixel(x, y, image::Rgba(channels));
    }
    DynamicImage::ImageRgba8(out)
}

/// Multi-preprocessing QR decode pipeline from the blog post.
/// Tries scales [0.5, 0.2, 0.1], and for each scale tries:
///   1. Plain scaled image
///   2. Sharpness variants [0.1, 0.5, 2.0]
///   3. Autocontrast
/// Scale 1.0 (full resolution) is skipped — the blog post showed it has the
/// worst success rate AND it is by far the slowest (rxing TryHarder on a 24MP
/// image can take minutes per attempt).
/// Returns the first successfully decoded QR code, or empty string.
fn decode_qr_with_preprocessing(img: &DynamicImage) -> String {
    let scalars: &[f64] = &[0.5, 0.2, 0.1];
    let sharpness_factors: &[f64] = &[0.1, 0.5, 2.0];

    for &scalar in scalars {
        let scaled = scale_image(img, scalar);

        // Try plain scaled
        if let Some(qr) = try_decode_qr(&scaled) {
            return qr;
        }

        // Try sharpness/blur variants
        for &sharpness in sharpness_factors {
            let adjusted = adjust_sharpness(&scaled, sharpness);
            if let Some(qr) = try_decode_qr(&adjusted) {
                return qr;
            }
        }

        // Try autocontrast
        let contrasted = autocontrast(&scaled);
        if let Some(qr) = try_decode_qr(&contrasted) {
            return qr;
        }
    }

    String::new()
}

/// Check if a file extension is HEIC/HEIF.
fn is_heic(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("heic") || e.eq_ignore_ascii_case("heif"))
        .unwrap_or(false)
}

/// Open a HEIC/HEIF file and decode it to a DynamicImage using heic-decoder (pure Rust).
fn open_heic(path: &Path) -> Result<DynamicImage, Error> {
    let data = fs::read(path)?;
    let output = heic_decoder::DecoderConfig::new()
        .decode(&data, heic_decoder::PixelLayout::Rgba8)
        .map_err(|e| Error::Other(format!("Failed to decode HEIC {}: {}", path.display(), e)))?;
    let width = output.width;
    let height = output.height;
    let rgba_image = image::RgbaImage::from_raw(width, height, output.data)
        .ok_or_else(|| Error::Other("Failed to construct RGBA image from HEIC data".into()))?;
    Ok(DynamicImage::ImageRgba8(rgba_image))
}

/// Open any supported image file (including HEIC/HEIF).
fn open_image(path: &Path) -> Result<DynamicImage, Error> {
    if is_heic(path) {
        open_heic(path)
    } else {
        Ok(image::open(path)?)
    }
}

fn process_single_image(image_path: &str) -> Result<ImageInfo, Error> {
    let path = Path::new(image_path);
    if !path.is_file() {
        return Err(Error::Other(format!("Path is not a file: {}", image_path)));
    }

    // ---- 1. Read EXIF metadata -------------------------------------------------
    let file = fs::File::open(path)?;
    let mut bufreader = std::io::BufReader::new(&file);
    let exif = ExifReader::new().read_from_container(&mut bufreader).ok();

    let get_tag = |tag: Tag| {
        exif.as_ref()
            .and_then(|exif| exif.get_field(tag, In::PRIMARY))
            .map(|field| field.display_value().with_unit(exif.as_ref().unwrap()).to_string())
    };

    let date = get_tag(Tag::DateTimeOriginal).unwrap_or_default();

    // Build a camera identity hash from EXIF fields that are constant per device
    let camera_hash = {
        let mut hasher = DefaultHasher::new();
        get_tag(Tag::Make).unwrap_or_default().hash(&mut hasher);
        get_tag(Tag::Model).unwrap_or_default().hash(&mut hasher);
        get_tag(Tag::BodySerialNumber).unwrap_or_default().hash(&mut hasher);
        get_tag(Tag::LensModel).unwrap_or_default().hash(&mut hasher);
        get_tag(Tag::LensSerialNumber).unwrap_or_default().hash(&mut hasher);
        get_tag(Tag::PixelXDimension).unwrap_or_default().hash(&mut hasher);
        get_tag(Tag::PixelYDimension).unwrap_or_default().hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    };

    // GPS
    let (latitude, longitude) = if exif
        .as_ref()
        .and_then(|e| e.get_field(Tag::GPSLatitude, In::PRIMARY))
        .is_some()
    {
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
    let img = open_image(path)?;
    let thumbnail = img.thumbnail(120, 120);
    let mut thumb_buf = Vec::new();
    thumbnail
        .write_to(
            &mut std::io::Cursor::new(&mut thumb_buf),
            image::ImageFormat::Png,
        )?;
    let thumbnail_base64 = base64::engine::general_purpose::STANDARD.encode(&thumb_buf);
    let thumbnail_data_url = format!("data:image/png;base64,{}", thumbnail_base64);

    // ---- 3. Scan for QR code (multi-preprocessing pipeline) --------------------
    let qr_code = decode_qr_with_preprocessing(&img);

    Ok(ImageInfo {
        path: image_path.to_string(),
        name: path.file_name().unwrap().to_string_lossy().to_string(),
        thumbnail: thumbnail_data_url,
        qr_code,
        date,
        latitude,
        longitude,
        camera_hash,
    })
}

/// Process all images in parallel. Emits "image-progress" events for progress
/// (for both successes and failures so the count always reaches total).
#[tauri::command]
async fn process_images(app: AppHandle, paths: Vec<String>) -> Result<Vec<ImageInfo>, Error> {
    let (tx, rx) = std::sync::mpsc::channel::<Option<ImageInfo>>();
    let app_clone = app.clone();

    let forwarder = std::thread::spawn(move || {
        let mut results = Vec::new();
        for item in rx {
            // Emit a progress tick for every image (success or failure)
            let _ = app_clone.emit("image-processed", ());
            if let Some(info) = item {
                results.push(info);
            }
        }
        results
    });

    paths.par_iter().for_each(|p| {
        let result = process_single_image(p);
        match result {
            Ok(info) => { let _ = tx.send(Some(info)); }
            Err(e) => {
                eprintln!("Skipping {}: {}", p, e);
                let _ = tx.send(None);
            }
        }
    });
    drop(tx);

    let mut results = forwarder.join().unwrap();
    results.sort_by(|a, b| a.camera_hash.cmp(&b.camera_hash).then(a.date.cmp(&b.date)));
    Ok(results)
}

/// Load a full-size image as a base64 data URL for the lightbox.
/// HEIC/HEIF files are converted to JPEG since browsers can't display them natively.
#[tauri::command]
async fn load_full_image(image_path: String) -> Result<String, Error> {
    let path = Path::new(&image_path);

    if is_heic(path) {
        let img = open_heic(path)?;
        let mut buf = Vec::new();
        img.write_to(
            &mut std::io::Cursor::new(&mut buf),
            image::ImageFormat::Jpeg,
        )?;
        let b64 = base64::engine::general_purpose::STANDARD.encode(&buf);
        return Ok(format!("data:image/jpeg;base64,{}", b64));
    }

    let data = fs::read(path)?;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("jpeg")
        .to_lowercase();
    let mime = match ext.as_str() {
        "png" => "image/png",
        "tif" | "tiff" => "image/tiff",
        "bmp" => "image/bmp",
        _ => "image/jpeg",
    };
    let b64 = base64::engine::general_purpose::STANDARD.encode(&data);
    Ok(format!("data:{};base64,{}", mime, b64))
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
        .invoke_handler(tauri::generate_handler![list_images, process_images, load_full_image, move_images])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
