"""Dependency-free, allowlisted static website for PythonAnywhere."""
from pathlib import Path
import mimetypes
import sqlite3
import json
from datetime import datetime, timezone
from http.cookies import SimpleCookie
ROOT = Path(__file__).resolve().parent
ROUTES = {'/':'index.html','/privacy/':'privacy/index.html','/robots.txt':'robots.txt','/sitemap.xml':'sitemap.xml'}
ASSETS = {'/assets/'+p.name: 'assets/'+p.name for p in (ROOT/'assets').iterdir() if p.is_file()}
DATA_PATH = ROOT.parent / 'zapture-data' / 'visits.sqlite3'
def visit_count(increment=False):
    DATA_PATH.parent.mkdir(parents=True, exist_ok=True)
    with sqlite3.connect(DATA_PATH, timeout=10) as db:
        db.execute('CREATE TABLE IF NOT EXISTS totals (id INTEGER PRIMARY KEY CHECK(id=1), visits INTEGER NOT NULL)')
        db.execute('INSERT OR IGNORE INTO totals VALUES (1, 0)')
        if increment:
            db.execute('UPDATE totals SET visits=visits+1 WHERE id=1')
        return db.execute('SELECT visits FROM totals WHERE id=1').fetchone()[0]

def counter_response(environ, start_response, path):
    method = environ.get('REQUEST_METHOD', 'GET')
    is_visit = path == '/api/visit'
    if (is_visit and method != 'POST') or (not is_visit and method not in ('GET', 'HEAD')):
        start_response('405 Method Not Allowed', [('Allow', 'POST' if is_visit else 'GET, HEAD'), ('Content-Length', '0')]); return [b'']
    # Only the landing page can increment; badge and stats requests are read-only.
    if is_visit and (environ.get('HTTP_X_ZAPTURE_VISIT') != '1' or environ.get('HTTP_ORIGIN') != 'https://zapture.pythonanywhere.com'):
        start_response('403 Forbidden', [('Content-Length', '0')]); return [b'']
    today = datetime.now(timezone.utc).date().isoformat()
    cookie = SimpleCookie()
    try: cookie.load(environ.get('HTTP_COOKIE', ''))
    except Exception: pass
    seen = cookie.get('zapture_visit')
    increment = is_visit and (seen is None or seen.value != today)
    headers = [('Cache-Control', 'no-store'), ('X-Content-Type-Options', 'nosniff')]
    try: count = visit_count(increment)
    except sqlite3.Error:
        start_response('503 Service Unavailable', [('Content-Length', '0'), ('Retry-After', '30')]); return [b'']
    if is_visit:
        headers.append(('Set-Cookie', 'zapture_visit=' + today + '; Max-Age=86400; Path=/; Secure; HttpOnly; SameSite=Lax'))
    if path == '/visitor-badge.svg':
        text = f'{count:,}'
        width = max(64, len(text)*8+20)
        body = (f'<svg xmlns="http://www.w3.org/2000/svg" width="{100+width}" height="24" role="img" aria-label="Website visits: {text}"><title>Website visits: {text}</title><rect width="{100+width}" height="24" rx="5" fill="#252923"/><path d="M100 0h{width-5}q5 0 5 5v14q0 5-5 5H100z" fill="#657b50"/><g fill="white" font-family="Verdana,Arial,sans-serif" font-size="11"><text x="50" y="16" text-anchor="middle">Website visits</text><text x="{100+width/2}" y="16" text-anchor="middle">{text}</text></g></svg>').encode()
        headers.append(('Content-Type','image/svg+xml; charset=utf-8'))
    else:
        body = json.dumps({'visits': count, 'metric': 'daily browser visits'}).encode()
        headers.append(('Content-Type','application/json'))
    headers.append(('Content-Length',str(len(body))))
    start_response('200 OK',headers)
    return [b'' if method == 'HEAD' else body]

def application(environ,start_response):
    path=environ.get('PATH_INFO','/')
    if path in ('/api/visit', '/api/stats', '/visitor-badge.svg'):
        return counter_response(environ, start_response, path)
    if environ.get('REQUEST_METHOD') not in ('GET','HEAD'):
        start_response('405 Method Not Allowed',[('Allow','GET, HEAD'),('Content-Length','0')]);return [b'']
    if path=='/privacy':
        start_response('301 Moved Permanently',[('Location','/privacy/'),('Content-Length','0')]);return [b'']
    relative=ROUTES.get(path) or ASSETS.get(path)
    if relative is None:
        body=b'<!doctype html><html lang="en"><title>Not found - Zapture</title><h1>Page not found</h1><a href="/">Back to Zapture</a></html>'
        start_response('404 Not Found',[('Content-Type','text/html; charset=utf-8'),('Content-Length',str(len(body))),('X-Robots-Tag','noindex')]);return [b'' if environ['REQUEST_METHOD']=='HEAD' else body]
    body=(ROOT/relative).read_bytes();mime=mimetypes.guess_type(relative)[0] or 'application/octet-stream'
    headers=[('Content-Type',mime+('; charset=utf-8' if mime.startswith('text/') else '')),('Content-Length',str(len(body))),('Cache-Control','public, max-age=3600' if path.startswith('/assets/') else 'public, max-age=300'),('X-Content-Type-Options','nosniff'),('Referrer-Policy','strict-origin-when-cross-origin'),('X-Frame-Options','DENY'),('Permissions-Policy','camera=(), microphone=(), display-capture=()')]
    start_response('200 OK',headers);return [b'' if environ['REQUEST_METHOD']=='HEAD' else body]
