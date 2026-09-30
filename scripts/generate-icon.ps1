$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$source = Join-Path $projectRoot 'assets/ptools.png'
$destination = Join-Path $projectRoot 'assets/ptools.ico'
$sizes = @(16, 20, 24, 32, 40, 48, 64, 96, 128, 256)

Add-Type -AssemblyName System.Drawing
$image = [Drawing.Bitmap]::FromFile($source)
$stream = [IO.MemoryStream]::new()
$writer = [IO.BinaryWriter]::new($stream)
try {
    if ($image.Width -ne $image.Height) { throw 'Application icon source must be square' }
    $frames = [Collections.Generic.List[byte[]]]::new()
    foreach ($size in $sizes) {
        $bitmap = [Drawing.Bitmap]::new($size, $size, [Drawing.Imaging.PixelFormat]::Format32bppArgb)
        $graphics = [Drawing.Graphics]::FromImage($bitmap)
        $frame = [IO.MemoryStream]::new()
        $attributes = [Drawing.Imaging.ImageAttributes]::new()
        try {
            $graphics.CompositingMode = [Drawing.Drawing2D.CompositingMode]::SourceCopy
            $graphics.CompositingQuality = [Drawing.Drawing2D.CompositingQuality]::HighQuality
            $graphics.InterpolationMode = [Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
            $graphics.PixelOffsetMode = [Drawing.Drawing2D.PixelOffsetMode]::HighQuality
            $attributes.SetWrapMode([Drawing.Drawing2D.WrapMode]::TileFlipXY)
            $graphics.DrawImage($image, [Drawing.Rectangle]::new(0, 0, $size, $size),
                0, 0, $image.Width, $image.Height, [Drawing.GraphicsUnit]::Pixel, $attributes)
            $bitmap.Save($frame, [Drawing.Imaging.ImageFormat]::Png)
            $frames.Add($frame.ToArray())
        } finally {
            $attributes.Dispose()
            $frame.Dispose()
            $graphics.Dispose()
            $bitmap.Dispose()
        }
    }

    # ICO directory followed by lossless PNG frames with their original alpha.
    $writer.Write([uint16]0)
    $writer.Write([uint16]1)
    $writer.Write([uint16]$sizes.Count)
    $offset = 6 + 16 * $sizes.Count
    for ($i = 0; $i -lt $sizes.Count; $i++) {
        $dimension = if ($sizes[$i] -eq 256) { 0 } else { $sizes[$i] }
        $writer.Write([byte]$dimension)
        $writer.Write([byte]$dimension)
        $writer.Write([byte]0)
        $writer.Write([byte]0)
        $writer.Write([uint16]1)
        $writer.Write([uint16]32)
        $writer.Write([uint32]$frames[$i].Length)
        $writer.Write([uint32]$offset)
        $offset += $frames[$i].Length
    }
    foreach ($frame in $frames) { $writer.Write($frame) }
    $writer.Flush()
    [IO.File]::WriteAllBytes($destination, $stream.ToArray())
} finally {
    $writer.Dispose()
    $stream.Dispose()
    $image.Dispose()
}
Write-Output "Ready: $destination ($($sizes -join ', ') px)"
