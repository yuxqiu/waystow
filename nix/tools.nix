# Command-line tools behind the built-in actions. All optional at runtime:
# an action whose tool is missing is hidden (`waystow doctor` lists them).
pkgs: with pkgs; [
  imagemagick
  ffmpeg
  qpdf
  ghostscript
  ocrmypdf
  tesseract
  exiftool
  libarchive # bsdtar
  jq
  qrencode
  poppler-utils # pdftoppm (thumbnails)
  img2pdf
  wl-clipboard
]
