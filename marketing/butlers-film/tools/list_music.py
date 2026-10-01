"""List Mixkit music (name, genre, duration, file id) for tag pages given on the command line."""
import re
import sys
import urllib.request

HEADERS = {"User-Agent": "Mozilla/5.0"}
for tag in sys.argv[1:]:
    try:
        page = urllib.request.urlopen(urllib.request.Request(f"https://mixkit.co/free-stock-music/tag/{tag}/", headers=HEADERS)).read().decode()
    except Exception as error:  # a missing tag page is fine; keep listing the rest
        print(tag, "ERROR", error)
        continue
    items = re.findall(r'"name":"([^"]+)","genre":"([^"]+)","byArtist":"([^"]+)","duration":"([^"]+)","url":"([^"]+)"', page)
    for name, genre, artist, duration, url in items[:16]:
        print(f"{tag:10s} {name:34s} {genre:14s} {artist:22s} {duration:8s} {url.split('/')[-1]}")
