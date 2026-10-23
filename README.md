English | [简体中文](README_ZH.md)

# SVG2Image

A cross-platform command-line utility that converts SVG files to WebP, PNG, or JPG images. SVG text is resolved with system fonts and converted to paths before rendering, preventing text-rendering differences in the generated image. Release packages are self-contained single executables and do not require a separate installation of a browser or image-conversion tool.

## Downloads

| Platform | Architecture | Package | Hash |
| --- | --- | --- | --- |
| Linux | x86_64 | [Download](https://github.com/Joey-Kot/SVG2Image/releases/download/Latest/svg2image-linux-x86_64.tar.gz) | [sha256](https://github.com/Joey-Kot/SVG2Image/releases/download/Latest/svg2image-linux-x86_64.tar.gz.sha256) |
| Linux | arm64 | [Download](https://github.com/Joey-Kot/SVG2Image/releases/download/Latest/svg2image-linux-arm64.tar.gz) | [sha256](https://github.com/Joey-Kot/SVG2Image/releases/download/Latest/svg2image-linux-arm64.tar.gz.sha256) |
| Windows | x86_64 | [Download](https://github.com/Joey-Kot/SVG2Image/releases/download/Latest/svg2image-windows-x86_64.zip) | [sha256](https://github.com/Joey-Kot/SVG2Image/releases/download/Latest/svg2image-windows-x86_64.zip.sha256) |
| Windows | arm64 | [Download](https://github.com/Joey-Kot/SVG2Image/releases/download/Latest/svg2image-windows-arm64.zip) | [sha256](https://github.com/Joey-Kot/SVG2Image/releases/download/Latest/svg2image-windows-arm64.zip.sha256) |
| macOS | x86_64 | [Download](https://github.com/Joey-Kot/SVG2Image/releases/download/Latest/svg2image-macos-x86_64.tar.gz) | [sha256](https://github.com/Joey-Kot/SVG2Image/releases/download/Latest/svg2image-macos-x86_64.tar.gz.sha256) |
| macOS | arm64 | [Download](https://github.com/Joey-Kot/SVG2Image/releases/download/Latest/svg2image-macos-arm64.tar.gz) | [sha256](https://github.com/Joey-Kot/SVG2Image/releases/download/Latest/svg2image-macos-arm64.tar.gz.sha256) |

## Usage

```text
svg2image --file <SVG> --size <WIDTH:HEIGHT> [--format <FORMAT>] [--quality <QUALITY>]
```

- `--file`: Required. The input SVG file.
- `--size`: Required. The output pixel width and height. Set either side to `-1` to calculate it automatically from the SVG aspect ratio, for example `1000:-1` or `-1:1000`. The separators `:`, `@`, and `*` are all accepted. Supplying two positive values remains supported and produces that exact size, which may stretch the SVG; both sides cannot be `-1`. Each resolved side can be at most 65,535 pixels and the image can contain at most 64,000,000 pixels in total; WebP additionally limits each side to 16,383 pixels.
- `--format`: Optional. Supported formats are `webp`, `png`, and `jpg`. The default is `webp`; `jpeg` is accepted as an alias for `jpg`.
- `--quality`: Optional. A value from `0.0` to `1.0`, with a default of `0.85`. This option controls the lossy encoding quality for WebP and JPG. PNG is lossless, so this option does not affect PNG image quality.

Examples:

```bash
svg2image --file image.svg --size '1000:-1'
svg2image --file image.svg --size '-1@1000' --format png
svg2image --file image.svg --size '1600*-1' --format jpg --quality 0.9
```

Run `svg2image --help` to view the command-line help.

## Output Rules

For an input file at `/path/image.svg`, the program writes the output file next to it:

```text
image.webp
```

The extension is automatically selected from `--format`. SVG text is resolved from available system fonts and converted to vector paths before the SVG is rasterized. This avoids downstream text-rendering differences. If none of the font families declared for a text span can be matched locally, the program writes a consolidated warning listing the unresolved font-family combinations to standard error, then continues with the existing fallback behavior; affected text may render incorrectly or be omitted.

If an output file with the same name already exists, the program reports an error and stops to avoid overwriting it.

## Building Locally

Rust `1.99.1` or later is required; the local toolchain and CI default to `1.99.1`:

```bash
cargo build --locked --release
```

The resulting executable is located at `target/release/svg2image`, or `target/release/svg2image.exe` on Windows.
