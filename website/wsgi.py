"""Dependency-free, allowlisted static website for PythonAnywhere."""
from pathlib import Path
import mimetypes
ROOT = Path(__file__).resolve().parent
ROUTES = {'/':'index.html','/privacy/':'privacy/index.html','/robots.txt':'robots.txt','/sitemap.xml':'sitemap.xml'}
ASSETS = {'/assets/'+p.name: 'assets/'+p.name for p in (ROOT/'assets').iterdir() if p.is_file()}
def application(environ,start_response):
    path=environ.get('PATH_INFO','/')
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
