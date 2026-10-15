#!/usr/bin/env python3
"""Renders the README screenshots from dist/ with demo data, in light and dark.

Usage: python3 docs/screenshots/make.py [scenario ...]

Needs Chrome or Chromium. The pages run against mock-tauri.js instead of the
app, so no real accounts, senders or codes end up in the images.
"""
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
OUT = ROOT / "docs" / "images"

# name: (page, width, height)
SHOTS = {
    "popup-code": ("code.html#code", 400, 236),
    "popup-link": ("code.html#link", 400, 220),
    "accounts": ("index.html#accounts", 820, 560),
    "rules": ("index.html#rules", 820, 560),
    "rule-editor": ("index.html#rule-editor", 820, 1000),
    "catalog": ("index.html#catalog", 820, 760),
    "log": ("index.html#log", 820, 760),
}
THEMES = {"light": [], "dark": ["--force-dark-mode", "--blink-settings=preferredColorScheme=0"]}


def browser() -> str:
    for name in ("google-chrome", "chromium", "chromium-browser"):
        if path := shutil.which(name):
            return path
    sys.exit("Chrome or Chromium not found")


def prepare(site: Path) -> None:
    shutil.copytree(ROOT / "dist", site)
    shutil.copy(HERE / "mock-tauri.js", site)
    catalog = (ROOT / "catalog" / "catalog.json").read_text()
    inject = f'<meta charset="utf-8">\n<script>window.__DEMO_CATALOG__ = {catalog}</script>\n<script src="mock-tauri.js"></script>'
    for page in ("index.html", "code.html"):
        html = (site / page).read_text()
        (site / page).write_text(html.replace('<meta charset="utf-8">', inject, 1))


def shoot(chrome: str, site: Path, name: str, theme: str) -> Path:
    page, width, height = SHOTS[name]
    file, scenario = page.split("#")
    out = OUT / f"{name}-{theme}.png"
    subprocess.run(
        [chrome, "--headless=new", "--disable-gpu", "--hide-scrollbars", "--lang=en-US",
         "--force-device-scale-factor=2", "--virtual-time-budget=3000", "--allow-file-access-from-files",
         f"--window-size={width},{height}", f"--screenshot={out}", *THEMES[theme], f"{(site / file).as_uri()}#{scenario}"],
        check=True, capture_output=True, env={**os.environ, "LANG": "en_US.UTF-8", "LC_ALL": "en_US.UTF-8"},
    )
    return out


def main() -> None:
    names = sys.argv[1:] or list(SHOTS)
    chrome = browser()
    OUT.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as tmp:
        site = Path(tmp) / "site"
        prepare(site)
        for name in names:
            for theme in THEMES:
                print(shoot(chrome, site, name, theme).relative_to(ROOT))


if __name__ == "__main__":
    main()
