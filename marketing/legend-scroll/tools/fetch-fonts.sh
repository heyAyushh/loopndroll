#!/bin/bash
# Downloads OFL fonts used by the scroll trailer into the directory given as $1.
set -u
destination="$1"
mkdir -p "$destination"
base="https://raw.githubusercontent.com/google/fonts/main/ofl"
fetch() {
  curl -fsSL -o "$destination/$2" "$base/$1" && echo "ok $2 $(wc -c < "$destination/$2")" || echo "FAIL $1"
}
fetch "mashanzheng/MaShanZheng-Regular.ttf" "MaShanZheng-Regular.ttf"
fetch "mashanzheng/OFL.txt" "OFL-MaShanZheng.txt"
fetch "zhimangxing/ZhiMangXing-Regular.ttf" "ZhiMangXing-Regular.ttf"
fetch "zhimangxing/OFL.txt" "OFL-ZhiMangXing.txt"
fetch "cormorantgaramond/CormorantGaramond%5Bwght%5D.ttf" "CormorantGaramond.ttf"
fetch "cormorantgaramond/CormorantGaramond-Italic%5Bwght%5D.ttf" "CormorantGaramond-Italic.ttf"
fetch "cormorantgaramond/OFL.txt" "OFL-CormorantGaramond.txt"
