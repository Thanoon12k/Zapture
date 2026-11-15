"""Zapture landing page with the owner's visitor-dashboard integration."""
from pathlib import Path
import os
import secrets
import mimetypes
from flask import Flask, Response, redirect, request
from visitor_dashboard.flask import VisitorDashboard

ROOT = Path(__file__).resolve().parent
DATA = ROOT.parent / 'zapture-data'
ROUTES = {'/':'index.html','/benchmarks/':'benchmarks/index.html','/privacy/':'privacy/index.html','/robots.txt':'robots.txt','/sitemap.xml':'sitemap.xml'}
ASSETS = {'/assets/'+p.name:'assets/'+p.name for p in (ROOT/'assets').iterdir() if p.is_file()}

def dashboard_password(data_dir):
    """An environment password or a private server-side file, never source control."""
    configured = os.environ.get('VISITS_PASSWORD')
    if configured:
        return configured
    path = data_dir / 'dashboard-password.txt'
    try:
        fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    except FileExistsError:
        return path.read_text().strip()
    else:
        password = secrets.token_urlsafe(24)
        with os.fdopen(fd,'w') as f:
            f.write(password+'\n')
        return password

def create_app(data_dir=None, password=None):
    data_dir = Path(data_dir or DATA)
    data_dir.mkdir(parents=True, exist_ok=True)
    app = Flask(__name__, static_folder=None)
    app.config.update(MAX_CONTENT_LENGTH=32768)
    VisitorDashboard(app, password=password or dashboard_password(data_dir),
                     db_path=data_dir/'visits.db', title='Zapture visitors',
                     tz='Asia/Baghdad', lang='en', notify='off', keep_days=180,
                     geo_online=False, geo_dir=data_dir/'geo', ignore=('/api','/assets'))

    @app.get('/')
    @app.get('/<path:unused>')
    def page(unused=''):
        if request.path == '/benchmarks':
            return redirect('/benchmarks/', code=301)
        if request.path == '/privacy':
            return redirect('/privacy/', code=301)
        relative = ROUTES.get(request.path) or ASSETS.get(request.path)
        if relative is None:
            return Response('<!doctype html><html lang="en"><title>Not found — Zapture</title><h1>Page not found</h1><a href="/">Back to Zapture</a></html>',status=404,mimetype='text/html')
        mime = mimetypes.guess_type(relative)[0] or 'application/octet-stream'
        return Response((ROOT/relative).read_bytes(), content_type=mime + ('; charset=utf-8' if mime.startswith('text/') else ''))

    @app.after_request
    def headers(response):
        response.headers['X-Content-Type-Options']='nosniff'
        response.headers['Referrer-Policy']='strict-origin-when-cross-origin'
        response.headers['X-Frame-Options']='DENY'
        response.headers['Permissions-Policy']='camera=(), microphone=(), display-capture=()'
        if request.path.startswith('/visits'):
            response.headers['X-Robots-Tag']='noindex, nofollow'
        if 'text/html' in response.content_type or request.path.startswith('/visits/api'):
            # The injected visit key and cookies must never be shared by a cache.
            response.headers['Cache-Control']='private, no-store'
        elif request.path.startswith('/assets/'):
            response.headers['Cache-Control']='public, max-age=3600'
        if response.status_code == 404:
            response.headers['X-Robots-Tag']='noindex'
        return response
    return app

application = create_app()
