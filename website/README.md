# Zapture website

Public: https://zapture.pythonanywhere.com/
Private visitor dashboard: https://zapture.pythonanywhere.com/visits

Responsive HTML/CSS/JavaScript with a Flask server and [visitor-dashboard](https://github.com/Thanoon12k/visitor-dashboard), the owner's analytics project. The recorder demo is a simulation. No screen/microphone capture is requested by the website.

## Run locally

```sh
pip install -r website/requirements.txt
flask --app website.wsgi:application run
```

Set `VISITS_PASSWORD` in the environment. If absent, a strong random password is generated once in `zapture-data/dashboard-password.txt`, outside all public routes. The file is created with owner-only permissions. Do not commit passwords or the database.

## PythonAnywhere

Upload this directory to `/home/Zapture/zapture-site/`. Install requirements or provide a trusted `dependencies.zip` containing the pinned Python 3.10 dependency tree. Use `pythonanywhere_wsgi.py` as `/var/www/zapture_pythonanywhere_com_wsgi.py`, enable forced HTTPS, and reload. The deployment bundle is extracted into a content-versioned directory outside public routes. Refresh the bundle when requirements change.

The dashboard password is in `/home/Zapture/zapture-data/dashboard-password.txt`, viewable only through the authenticated PythonAnywhere Files interface. Alternatively set `VISITS_PASSWORD` and reload. Never display this file in public routes or logs.

Persistent analytics: `/home/Zapture/zapture-data/visits.db`. Keep this file across deployments. The earlier temporary simple counter database is not used and has not been deleted.

## Analytics configuration

- English dashboard at `/visits`, Arabic available through its language switch.
- Baghdad time, 180-day retention, password-protected stats and CSV export.
- First-party returning-visitor cookie, devices, referrers, duration, scroll and click tracking.
- Local DB-IP country and ASN databases. Approximate countries/providers; city-level data is not configured. Online IP lookups and Telegram notifications are disabled.
- DB-IP Lite data: https://db-ip.com/ (CC BY 4.0). Current deployment uses November 2026. Update the compressed country/ASN files periodically, then reload to replace the local databases.
- HTML uses `private, no-store` to avoid caching per-visit tokens. Dashboard endpoints are noindex and excluded from the sitemap. Static files and dashboard requests are not tracked.
- Log in with “Don't count my visits from this device” enabled to exclude your browser.

The former public counter and README badge were removed because the requested integration is the private visitor-dashboard.

## SEO and maintenance

Server-rendered content, canonical URLs, SoftwareApplication structured data, social preview image, robots.txt, sitemap.xml, and semantic headings. Submit the sitemap to Google Search Console and Bing Webmaster Tools after verifying ownership; rankings and indexing are not guaranteed. Update canonical/social/sitemap URLs together when moving to a custom domain.

The free PythonAnywhere app initially expires December 14, 2026; extend it from the Web tab before expiry. Update release URLs, version labels, size, and platform notes when publishing a new desktop release. Windows is primary; Linux/macOS remain experimental.

## Checks

`python -m unittest website.test_dashboard`
