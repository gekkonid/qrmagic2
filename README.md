# QRMagic2

An app that sorts images into folders based on QR codes found in them.

## Usage

1. Click "Choose Image Folder" to select a folder containing images (JPG, PNG, HEIC). Subfolders are included.
2. The app scans all images, extracts EXIF metadata, and attempts to decode any QR codes. A progress bar shows scan status.
3. Images are displayed in a table sorted by camera and date. Rows without a detected QR code are highlighted yellow.
4. Edit QR codes manually by clicking the text field. On focus, it auto-fills from the nearest neighbour taken within 30 seconds on the same camera.
5. Click "Auto-fill all" to propagate QR codes across all neighbouring images at once.
6. Click a thumbnail to open a full-size lightbox. Use left/right arrow keys to navigate, Escape to close.
7. Click "Choose Output Folder", then "Export" to sort images into subfolders named by their QR code. Check "Copy instead of move" to keep originals in place.

## Installation

Download from the [GitHub Releases page](https://github.com/gekkonid/qrmagic2/releases).
Available for Linux (appimage, extract and run), Windows (setup.exe installer),
and Mac OSX (DMG for Apple Silicon). Other OS/platforms should be possible,
just build from source or pester me and I'll make them.
