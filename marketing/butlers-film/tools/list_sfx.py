"""List Mixkit sound effects (title, duration, preview URL) for a few tag pages."""
import re
import sys
import urllib.request

TAGS = sys.argv[1:] or ["whoosh", "door", "stamp", "notification", "typewriter", "click", "clock", "swoosh"]
HEADERS = {"User-Agent": "Mozilla/5.0"}

for tag in TAGS:
    request = urllib.request.Request(f"https://mixkit.co/free-sound-effects/{tag}/", headers=HEADERS)
    try:
        page = urllib.request.urlopen(request).read().decode()
    except Exception as error:  # a missing tag page is fine; keep listing the rest
        print(tag, "ERROR", error)
        continue
    titles = re.findall(r'item-grid-card__title[^>]*>\s*([^<]+)', page)
    urls = re.findall(r'data-audio-player-preview-url-value="([^"]+)"', page)
    for title, url in list(zip(titles, urls))[:14]:
        print(f"{tag:13s} {title.strip():45s} {url}")
