"""Relocate Debian OBS's mux helper for this unprivileged cloud experiment."""
import argparse
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('root', type=Path, help='Benchmark root containing extracted root/usr')
args = parser.parse_args()
plugin = args.root / 'root/usr/lib/x86_64-linux-gnu/obs-plugins/obs-ffmpeg.so'
old = b'/usr/lib/x86_64-linux-gnu/obs-plugins/obs-ffmpeg/obs-ffmpeg-mux'
new = b'/tmp/zapture-obs-mux'
data = plugin.read_bytes()
if old in data:
    assert data.count(old) == 1, 'Unexpected plugin: multiple helper paths'
    plugin.write_bytes(data.replace(old, new + bytes(len(old) - len(new))))
    print('Relocated helper path; encoding code unchanged.')
elif new in data:
    print('Helper path already relocated.')
else:
    raise SystemExit('Unexpected plugin: helper path missing; refusing to modify it.')
