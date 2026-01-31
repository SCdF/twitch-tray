# Assets

## Icon Conversion

The tray icons are embedded at compile time using Go's `embed` package.

### Requirements

- ImageMagick 7+ (`magick` command)

### Source Image

Place your source image as `original.png`. Recommended:
- 256x256 or larger
- PNG with transparency
- White or light-colored logo (for dark system trays)

### Converting Icons

From the `assets/` directory:

```bash
# Create the main 64x64 icon
magick original.png -resize 64x64 icon.png

# Create a dimmed version for unauthenticated state (40% opacity)
magick original.png -resize 64x64 -channel A -evaluate Multiply 0.4 +channel icon_grey.png
```

### After Conversion

Rebuild the application to embed the new icons:

```bash
cd /path/to/twitch-tray
go build ./cmd/twitch-tray
```

The icons are embedded via `assets.go` - no need to distribute separate image files.
