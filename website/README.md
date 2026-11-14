# Zapture landing website

Public site: https://zapture.pythonanywhere.com/

Buildless HTML, CSS, JavaScript, and a standard-library WSGI server. No application dependencies, credentials, analytics, or database. The recorder demo is a simulation and cannot capture a visitor's screen. CSS artwork scales to high-resolution displays and respects reduced motion.

Run locally from the repository root:

```sh
python -c "from wsgiref.simple_server import make_server; from website.wsgi import application; make_server('127.0.0.1',8080,application).serve_forever()"
```

Deploy files to `/home/Zapture/zapture-site/`; the PythonAnywhere WSGI configuration adds that directory to sys.path and imports `application` from `wsgi`. Set force HTTPS and reload. Never commit API tokens.

SEO: server-rendered content, canonical URLs, SoftwareApplication structured data, Open Graph/Twitter social image, robots.txt, XML sitemap, semantic sections and descriptive text. Submit the sitemap in Google Search Console/Bing Webmaster Tools after verifying ownership; indexing and rankings are not guaranteed. If moving to a custom domain, update all canonical, social, structured data, robots and sitemap URLs together.

Update release links, version labels, ZIP size and platform support when publishing a new desktop release. Settings features are Windows-specific. Linux/macOS remain experimental; no mobile or Wayland support is claimed.
