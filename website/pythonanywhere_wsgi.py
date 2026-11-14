"""Install as /var/www/zapture_pythonanywhere_com_wsgi.py.
The optional deployment bundle contains trusted, pinned dependencies.
"""
from pathlib import Path
import hashlib
import sys
import zipfile
import gzip
import shutil

ROOT = Path('/home/Zapture/zapture-site')
DATA = ROOT.parent / 'zapture-data'
archive = ROOT / 'dependencies.zip'
if archive.is_file():
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()[:16]
    vendor = DATA / ('vendor-' + digest)
    if not (vendor / '.ready').exists():
        vendor.mkdir(parents=True, exist_ok=True)
        with zipfile.ZipFile(archive) as bundle:
            bundle.extractall(vendor)
        (vendor / '.ready').touch()
    sys.path.insert(0, str(vendor))
geo_dir = DATA / 'geo'
geo_dir.mkdir(parents=True, exist_ok=True)
for kind in ('country', 'asn'):
    source = ROOT / ('dbip-' + kind + '-lite.mmdb.gz')
    dest = geo_dir / ('dbip-' + kind + '-lite.mmdb')
    if source.exists() and (not dest.exists() or source.stat().st_mtime > dest.stat().st_mtime):
        temp = dest.with_suffix('.tmp')
        with gzip.open(source,'rb') as incoming, temp.open('wb') as outgoing:
            shutil.copyfileobj(incoming, outgoing)
        temp.replace(dest)
sys.path.insert(0, str(ROOT))
from wsgi import application
