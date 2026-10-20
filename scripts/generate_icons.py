#!/usr/bin/env python3
"""Generate S-Tools icon candidates and Tauri source assets."""

from __future__ import annotations

import math
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter, ImageFont

ROOT = Path(__file__).resolve().parents[1]
DESIGN = ROOT / "design" / "icon"
ICONS = ROOT / "src-tauri" / "icons"
SCALE = 4
SIZE = 1024


def cubic(p0, p1, p2, p3, steps=140):
    points = []
    for index in range(steps + 1):
        t = index / steps
        u = 1 - t
        x = u**3 * p0[0] + 3 * u * u * t * p1[0] + 3 * u * t * t * p2[0] + t**3 * p3[0]
        y = u**3 * p0[1] + 3 * u * u * t * p1[1] + 3 * u * t * t * p2[1] + t**3 * p3[1]
        points.append((x * SCALE, y * SCALE))
    return points


def s_centerline():
    return cubic((685, 286), (500, 175), (280, 280), (350, 445)) + cubic(
        (350, 445), (400, 566), (630, 548), (698, 682)
    ) + cubic((698, 682), (755, 803), (580, 878), (350, 820))[1:]


def rounded_gradient(size, stops, radius=224):
    canvas = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    gradient_line = Image.new("RGBA", (1, size))
    pixels = gradient_line.load()
    for y in range(size):
        value = y / size
        for index in range(len(stops) - 1):
            left_pos, left_color = stops[index]
            right_pos, right_color = stops[index + 1]
            if left_pos <= value <= right_pos:
                ratio = (value - left_pos) / max(0.0001, right_pos - left_pos)
                color = tuple(
                    round(left_color[channel] * (1 - ratio) + right_color[channel] * ratio)
                    for channel in range(4)
                )
                pixels[0, y] = color
                break
    gradient = gradient_line.resize((size, size))
    mask = Image.new("L", (size, size), 0)
    ImageDraw.Draw(mask).rounded_rectangle((0, 0, size - 1, size - 1), radius=radius, fill=255)
    canvas.alpha_composite(Image.composite(gradient, Image.new("RGBA", gradient.size), mask))
    return canvas


def draw_ribbon(image, color, width=178, extra_line=None):
    points = s_centerline()
    draw = ImageDraw.Draw(image)
    radius = width * SCALE / 2
    for point in points:
        draw.ellipse(
            (point[0] - radius, point[1] - radius, point[0] + radius, point[1] + radius),
            fill=color,
        )
    if extra_line:
        color_value, extra_width, start, end = extra_line
        segment = points[int(len(points) * start) : int(len(points) * end)]
        extra_radius = extra_width * SCALE / 2
        for point in segment:
            draw.ellipse(
                (
                    point[0] - extra_radius,
                    point[1] - extra_radius,
                    point[0] + extra_radius,
                    point[1] + extra_radius,
                ),
                fill=color_value,
            )


def candidate_a():
    image = rounded_gradient(
        SIZE * SCALE,
        [
            (0.0, (34, 35, 41, 255)),
            (0.58, (23, 24, 27, 255)),
            (1.0, (15, 16, 18, 255)),
        ],
    )
    glow = Image.new("RGBA", image.size, (0, 0, 0, 0))
    glow_draw = ImageDraw.Draw(glow)
    glow_draw.ellipse((520 * SCALE, 470 * SCALE, 1160 * SCALE, 1110 * SCALE), fill=(247, 147, 30, 54))
    glow = glow.filter(ImageFilter.GaussianBlur(110 * SCALE))
    image.alpha_composite(glow)
    draw_ribbon(image, (247, 244, 238, 255), extra_line=((247, 147, 30, 255), 68, 0.74, 1.0))
    outline = ImageDraw.Draw(image)
    outline.rounded_rectangle(
        (7 * SCALE, 7 * SCALE, 1017 * SCALE, 1017 * SCALE),
        radius=217 * SCALE,
        outline=(255, 255, 255, 18),
        width=12 * SCALE,
    )
    return image


def candidate_b():
    image = rounded_gradient(
        SIZE * SCALE,
        [
            (0.0, (75, 77, 255, 255)),
            (0.48, (118, 89, 246, 255)),
            (1.0, (177, 92, 255, 255)),
        ],
    )
    halo = Image.new("RGBA", image.size, (0, 0, 0, 0))
    halo_draw = ImageDraw.Draw(halo)
    halo_draw.ellipse((-180 * SCALE, -180 * SCALE, 760 * SCALE, 650 * SCALE), fill=(255, 255, 255, 58))
    halo = halo.filter(ImageFilter.GaussianBlur(120 * SCALE))
    image.alpha_composite(halo)
    shadow = Image.new("RGBA", image.size, (0, 0, 0, 0))
    draw_ribbon(shadow, (41, 19, 92, 115))
    shadow = shadow.filter(ImageFilter.GaussianBlur(26 * SCALE))
    image.alpha_composite(shadow)
    draw_ribbon(image, (242, 239, 255, 230), extra_line=((255, 255, 255, 210), 16, 0.0, 0.32))
    return image


def candidate_c():
    image = Image.new("RGBA", (SIZE * SCALE, SIZE * SCALE), (245, 241, 232, 255))
    draw_ribbon(image, (17, 18, 20, 255))
    return image


def save_candidate(image, name):
    image = image.resize((SIZE, SIZE), Image.Resampling.LANCZOS)
    ICONS.mkdir(parents=True, exist_ok=True)
    image.save(DESIGN / f"{name}.png")
    return image


def font(size):
    paths = [
        "/System/Library/Fonts/PingFang.ttc",
        "/System/Library/Fonts/Supplemental/Arial.ttf",
    ]
    for path in paths:
        if Path(path).exists():
            try:
                return ImageFont.truetype(path, size)
            except OSError:
                pass
    return ImageFont.load_default()


def contact_sheet(candidates):
    cell = 420
    sheet = Image.new("RGBA", (cell * 3 + 80, 620), (17, 18, 20, 255))
    draw = ImageDraw.Draw(sheet)
    labels = ["A · S-CORE", "B · S-GLASS", "C · S-MONO"]
    for index, image in enumerate(candidates):
        x = 20 + index * cell
        large = image.resize((360, 360), Image.Resampling.LANCZOS)
        sheet.alpha_composite(large, (x + 30, 60))
        draw.text((x + 30, 22), labels[index], fill=(247, 244, 238, 255), font=font(18))
        small_y = 480
        for size in (64, 32, 16):
            preview = image.resize((size, size), Image.Resampling.LANCZOS)
            sheet.alpha_composite(preview, (x + 42, small_y))
            draw.text(
                (x + 120, small_y + 2),
                f"{size}px",
                fill=(150, 150, 145, 255),
                font=font(12),
            )
            small_y += 48
    sheet.convert("RGB").save(DESIGN / "contact-sheet.png")


def tray_icon():
    size = 64
    source = s_centerline()
    # s_centerline 已经是 4 倍超采样坐标，这里必须除以 SIZE * SCALE。
    scale = size * 4 / (SIZE * SCALE)
    points = [(x * scale, y * scale) for x, y in source]
    width = max(4, round(160 * (size / SIZE) * 4))
    radius = max(3, round(width / 2))

    for file_name, color in (
        ("tray.png", (247, 147, 30, 255)),
        ("tray-template.png", (0, 0, 0, 255)),
    ):
        canvas = Image.new("RGBA", (size * 4, size * 4), (0, 0, 0, 0))
        draw = ImageDraw.Draw(canvas)
        for point in points:
            draw.ellipse(
                (point[0] - radius, point[1] - radius, point[0] + radius, point[1] + radius),
                fill=color,
            )
        canvas.resize((size, size), Image.Resampling.LANCZOS).save(ICONS / file_name)


def main():
    DESIGN.mkdir(parents=True, exist_ok=True)
    ICONS.mkdir(parents=True, exist_ok=True)
    images = [candidate_a(), candidate_b(), candidate_c()]
    finals = [
        save_candidate(images[0], "candidate-a-s-core"),
        save_candidate(images[1], "candidate-b-s-glass"),
        save_candidate(images[2], "candidate-c-s-mono"),
    ]
    contact_sheet(finals)
    finals[0].save(ICONS / "app-source.png")
    tray_icon()
    for size in (16, 32, 64, 128, 256, 512):
        finals[0].resize((size, size), Image.Resampling.LANCZOS).save(ICONS / f"{size}x{size}.png")


if __name__ == "__main__":
    main()
