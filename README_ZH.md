[English](README.md) | 简体中文

# SVG2Image

一个跨平台命令行程序，将 SVG 转换为 WebP、PNG 或 JPG 图像。程序会在渲染前使用系统字体解析 SVG 文本并将其转为路径，避免生成图像中的文本渲染差异。发布包为自包含的单个可执行程序，不需要另外安装浏览器或图像转换工具。

## 下载

| Platform | Architecture | Package | Hash |
| --- | --- | --- | --- |
| Linux | x86_64 | [Download](https://github.com/Joey-Kot/SVG2Image/releases/download/Latest/svg2image-linux-x86_64.tar.gz) | [sha256](https://github.com/Joey-Kot/SVG2Image/releases/download/Latest/svg2image-linux-x86_64.tar.gz.sha256) |
| Linux | arm64 | [Download](https://github.com/Joey-Kot/SVG2Image/releases/download/Latest/svg2image-linux-arm64.tar.gz) | [sha256](https://github.com/Joey-Kot/SVG2Image/releases/download/Latest/svg2image-linux-arm64.tar.gz.sha256) |
| Windows | x86_64 | [Download](https://github.com/Joey-Kot/SVG2Image/releases/download/Latest/svg2image-windows-x86_64.zip) | [sha256](https://github.com/Joey-Kot/SVG2Image/releases/download/Latest/svg2image-windows-x86_64.zip.sha256) |
| Windows | arm64 | [Download](https://github.com/Joey-Kot/SVG2Image/releases/download/Latest/svg2image-windows-arm64.zip) | [sha256](https://github.com/Joey-Kot/SVG2Image/releases/download/Latest/svg2image-windows-arm64.zip.sha256) |
| macOS | x86_64 | [Download](https://github.com/Joey-Kot/SVG2Image/releases/download/Latest/svg2image-macos-x86_64.tar.gz) | [sha256](https://github.com/Joey-Kot/SVG2Image/releases/download/Latest/svg2image-macos-x86_64.tar.gz.sha256) |
| macOS | arm64 | [Download](https://github.com/Joey-Kot/SVG2Image/releases/download/Latest/svg2image-macos-arm64.tar.gz) | [sha256](https://github.com/Joey-Kot/SVG2Image/releases/download/Latest/svg2image-macos-arm64.tar.gz.sha256) |

## 使用方法

```text
svg2image --file <SVG> --size <宽:高> [--format <格式>] [--quality <质量>]
```

- `--file`：必填，输入 SVG 文件。
- `--size`：必填，输出图像的像素宽高。将任意一边设为 `-1`，程序会根据 SVG 原始宽高比自动计算，例如 `1000:-1` 或 `-1:1000`。支持 `:`、`@` 和 `*` 三种分隔符。仍兼容两个正整数，此时会输出指定的精确尺寸，宽高比不同时可能拉伸；宽和高不能同时为 `-1`。计算后的单边最大为 65,535 像素，总像素数不能超过 64,000,000；WebP 的单边还不能超过 16,383 像素。
- `--format`：可选，支持 `webp`、`png`、`jpg`，默认为 `webp`。`jpeg` 可作为 `jpg` 的别名。
- `--quality`：可选，范围为 `0.0` 到 `1.0`，默认为 `0.85`。该参数控制 WebP 和 JPG 的有损编码质量；PNG 为无损格式，因此不改变 PNG 画质。

示例：

```bash
svg2image --file image.svg --size '1000:-1'
svg2image --file image.svg --size '-1@1000' --format png
svg2image --file image.svg --size '1600*-1' --format jpg --quality 0.9
```

也可以运行 `svg2image --help` 查看帮助。

## 输出规则

输入文件为 `/path/image.svg` 时，程序会在同目录输出：

```text
image.webp
```

输出扩展名由 `--format` 自动确定。SVG 文本会先使用本机可用的系统字体解析，再转为矢量路径后栅格化，避免后续环节出现文本渲染差异。如果某段文本声明的字体族列表在本机均无法匹配，程序会在标准错误中汇总列出这些字体族组合并发出警告，但仍按现有字体回退规则继续转换；相关文本可能显示异常或缺失。

如果同名输出文件已经存在，程序会报错并停止，避免覆盖已有文件。

## 本地编译

需要 Rust `1.92` 或更高版本：

```bash
cargo build --release
```

编译结果位于 `target/release/svg2image`；Windows 下为 `target/release/svg2image.exe`。
