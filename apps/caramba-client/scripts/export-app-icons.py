#!/usr/bin/env python3
"""Package the approved Caramba artwork into native icons (requires ImageMagick)."""
import json
from pathlib import Path
import subprocess

CLIENT = Path(__file__).resolve().parents[1]
SOURCE = CLIENT.parents[1] / 'docs/brand/caramba-icon.png'


def export(path, size):
    path.parent.mkdir(parents=True, exist_ok=True)
    subprocess.run(['magick', str(SOURCE), '-resize', f'{size}x{size}',
                    '-alpha', 'off', '-strip', f'PNG24:{path}'], check=True)


for platform in ('ios', 'macos'):
    catalog = CLIENT / platform / 'Runner/Assets.xcassets/AppIcon.appiconset'
    for image in json.loads((catalog / 'Contents.json').read_text())['images']:
        size = round(float(image['size'].split('x')[0]) * float(image['scale'][:-1]))
        export(catalog / image['filename'], size)

for density, size in {'mdpi': 48, 'hdpi': 72, 'xhdpi': 96, 'xxhdpi': 144, 'xxxhdpi': 192}.items():
    export(CLIENT / f'android/app/src/main/res/mipmap-{density}/ic_launcher.png', size)

export(CLIENT / 'assets/brand/caramba-connect.png', 256)
export(CLIENT / 'linux/icons/caramba-connect.png', 512)
subprocess.run(['magick', str(SOURCE), '-resize', '256x256', '-alpha', 'off',
                '-define', 'icon:auto-resize=256,128,64,48,32,24,16',
                str(CLIENT / 'windows/runner/resources/app_icon.ico')], check=True)
print('Exported native launcher icons and in-app brand asset.')
