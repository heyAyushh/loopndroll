"""Search the Internet Archive for public-domain audio items (prints id, title, year, licence)."""
import json
import sys
import urllib.parse
import urllib.request

query = " ".join(sys.argv[1:]) or 'joplin rag "piano roll"'
params = {
    "q": f"({query}) AND mediatype:audio",
    "fl[]": ["identifier", "title", "date", "licenseurl", "creator"],
    "rows": "25", "output": "json",
}
url = "https://archive.org/advancedsearch.php?" + urllib.parse.urlencode(params, doseq=True)
data = json.loads(urllib.request.urlopen(urllib.request.Request(url, headers={"User-Agent": "Mozilla/5.0"})).read())
for doc in data["response"]["docs"]:
    print(f"{doc.get('identifier', ''):45s} | {str(doc.get('title', ''))[:60]:60s} | {str(doc.get('date', ''))[:10]:10s} | {doc.get('licenseurl', '-')}")
