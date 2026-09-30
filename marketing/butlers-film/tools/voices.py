"""Render every spoken line with macOS `say` and print durations.

The film timeline is built around these durations, so re-run this whenever a line changes.
Output: build/voice/<id>.wav (48 kHz mono) and build/voice/durations.json.
"""
import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT_DIR = ROOT / "build" / "voice"
NARRATOR = ("Daniel", 150)   # deadpan British narrator, slowed down
FRIEND = ("Samantha", 170)   # breezy, just back from the beach

LINES = {
    "n_chair": (NARRATOR, "The developer had not left the chair in three days."),
    "n_butlers": (NARRATOR, "Behind him stood three of the finest butlers available."),
    "n_codex": (NARRATOR, "Codex."),
    "n_claude": (NARRATOR, "Claude Code."),
    "n_cursor": (NARRATOR, "Cursor."),
    "n_stopped": (NARRATOR, "They had done a great deal of work. Then they stopped. "
                            "They would not proceed without permission."),
    "n_question": (NARRATOR, "One of them had a question."),
    "n_beach": (NARRATOR, "On the fourth day, a friend returned from the beach."),
    "f_still": (FRIEND, "You're still here?"),
    "f_looper": (FRIEND, "Get Looper. It's an app on your Mac."),
    "f_done": (FRIEND, "It checks whether they're really done."),
    "f_back": (FRIEND, "Tests fail? It sends them back to work."),
    "f_phone": (FRIEND, "Real questions come to your phone. Anywhere."),
    "f_spaces": (FRIEND, "Spaces."),
    "f_no": (FRIEND, "Absolutely not."),
    "n_free": (NARRATOR, "The developer stood up."),
    "n_day": (NARRATOR, "It was, by all accounts, a lovely day."),
}


def render_line(line_id: str, voice: str, rate: int, text: str) -> float:
    aiff = OUT_DIR / f"{line_id}.aiff"
    wav = OUT_DIR / f"{line_id}.wav"
    subprocess.run(["say", "-v", voice, "-r", str(rate), "-o", str(aiff), text], check=True)
    # Trim leading/trailing silence so cue times land on the first syllable.
    subprocess.run(["ffmpeg", "-loglevel", "error", "-y", "-i", str(aiff), "-af",
                    "silenceremove=start_periods=1:start_threshold=-50dB,areverse,"
                    "silenceremove=start_periods=1:start_threshold=-50dB,areverse",
                    "-ar", "48000", "-ac", "1", str(wav)], check=True)
    aiff.unlink()
    probe = subprocess.run(["ffprobe", "-v", "error", "-show_entries", "format=duration",
                            "-of", "csv=p=0", str(wav)], capture_output=True, text=True, check=True)
    return round(float(probe.stdout), 3)


def main() -> None:
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    durations = {}
    for line_id, ((voice, rate), text) in LINES.items():
        durations[line_id] = {"duration": render_line(line_id, voice, rate, text), "text": text}
        print(f"{line_id:12s} {durations[line_id]['duration']:5.2f}s  {text}")
    (OUT_DIR / "durations.json").write_text(json.dumps(durations, indent=2))


if __name__ == "__main__":
    main()
