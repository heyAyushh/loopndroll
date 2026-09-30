"""Download the Mixkit sound effects used in the film into build/sfx/<name>.mp3.

Mixkit Sound Effects Free License. Names map to the events in tools/mix.py.
"""
import urllib.request
from pathlib import Path

SFX = {
    "sweep": 166,        # Fast small sweep transition (whips, snap zooms, title lift)
    "whoosh": 1490,      # Fast whoosh transition
    "swoosh": 1461,      # Short wind swoosh (flying cards)
    "door": 195,         # Creaky door open
    "stamp": 1384,       # Typewriter single mechanical hit (rubber stamp)
    "stamp2": 1365,      # Mechanical typewriter hit
    "pop": 2354,         # Message pop alert (notifications)
    "click": 1117,       # Classic click (taps)
    "typing": 1372,      # Old typewriter typing
    "laptop": 2531,      # Typing on a laptop keyboard
    "tick": 1060,        # Wall clock tick tock
    "creak": 337,        # Wooden floorboard creak (the developer stands)
    "correct": 2870,     # Correct answer tone (tests pass)
    "bell": 1368,        # Typewriter return bell
}
OUT = Path(__file__).resolve().parent.parent / "build" / "sfx"
OUT.mkdir(parents=True, exist_ok=True)
for name, sfx_id in SFX.items():
    path = OUT / f"{name}.mp3"
    if not path.exists():
        request = urllib.request.Request(f"https://assets.mixkit.co/active_storage/sfx/{sfx_id}/{sfx_id}-preview.mp3",
                                         headers={"User-Agent": "Mozilla/5.0"})
        path.write_bytes(urllib.request.urlopen(request).read())
    print(name, path.stat().st_size)

MUSIC = OUT.parent / "music" / "secret-garden.mp3"   # "Secret Garden", Eugenio Mininni, Mixkit Stock Music Free License
MUSIC.parent.mkdir(parents=True, exist_ok=True)
if not MUSIC.exists():
    request = urllib.request.Request("https://assets.mixkit.co/music/595/595.mp3", headers={"User-Agent": "Mozilla/5.0"})
    MUSIC.write_bytes(urllib.request.urlopen(request).read())
print("music", MUSIC.stat().st_size)
