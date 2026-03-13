Please make the framework of a qr-code image sorter using the Tauri api/gui
framework. It should: be given a list of images, read image metadata, sort
images by camera (unique ID/serial num) and date (oldest-newest). Then, scan
images for QR codes, and present a table of thumbnail, scanned qr code, date,
lat, long. The user should then fill any missing QR codes, and images should
then be either moved or copied to  $output_directory/$QRCODE/$image_name 

